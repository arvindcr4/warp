//! anthropic-bridge: a small HTTP service that exposes an OpenAI-compatible
//! Chat Completions surface and forwards translated requests to an
//! Anthropic-compatible Messages endpoint.
//!
//! Warp's backend only speaks the OpenAI Chat Completions API to custom
//! endpoints. To use an Anthropic-compatible endpoint (e.g. a MiniMax Token
//! Plan at `https://api.minimax.io/anthropic`), the Warp client registers the
//! endpoint's base URL as `{bridge}/a/{base64url(target)}`; Warp's backend
//! then POSTs OpenAI-format requests to
//! `{bridge}/a/{base64url(target)}/chat/completions`, and this service
//! forwards translated Anthropic-format requests to `{target}/v1/messages`
//! using the caller's own API key (the bridge stores no credentials).
//!
//! Usage: `anthropic-bridge [bind-addr]` (default `127.0.0.1:8744`, also
//! configurable via `BRIDGE_BIND`). Run it behind an HTTPS reverse proxy on a
//! host reachable from the internet, since Warp's backend is the caller.

mod translate;

use std::sync::OnceLock;

use axum::body::{Body, Bytes};
use axum::extract::Path;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine as _;
use futures::StreamExt;
use serde_json::{json, Value};

use crate::translate::{
    anthropic_to_openai_error, anthropic_to_openai_response, openai_to_anthropic, SseTranslator,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Decodes the URL-safe base64 path segment back into the target base URL.
fn decode_target(encoded: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
        .ok()?;
    let target = String::from_utf8(bytes).ok()?;
    (target.starts_with("https://") || target.starts_with("http://"))
        .then(|| target.trim_end_matches('/').to_string())
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let body = json!({
        "error": {"message": message, "type": "bridge_error", "code": status.as_u16()}
    });
    (status, Json(body)).into_response()
}

async fn handle_get(Path((_target, rest)): Path<(String, String)>) -> Response {
    // Some OpenAI-compatible clients probe the models list; answer with an
    // empty list rather than an error since the bridge is target-agnostic.
    if rest == "models" || rest.ends_with("/models") {
        Json(json!({"object": "list", "data": []})).into_response()
    } else {
        error_response(StatusCode::NOT_FOUND, "not found")
    }
}

async fn handle_post(
    Path((target, rest)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !rest.ends_with("chat/completions") {
        return error_response(
            StatusCode::NOT_FOUND,
            "unsupported path; only chat/completions is bridged",
        );
    }
    let Some(target) = decode_target(&target) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid target: expected URL-safe base64 of an http(s) base URL",
        );
    };
    let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return error_response(StatusCode::UNAUTHORIZED, "missing Authorization header");
    };
    let token = auth.strip_prefix("Bearer ").unwrap_or(&auth).to_owned();

    let Ok(request_body) = serde_json::from_slice::<Value>(&body) else {
        return error_response(StatusCode::BAD_REQUEST, "request body is not valid JSON");
    };
    let anthropic_body = match openai_to_anthropic(&request_body) {
        Ok(body) => body,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, &message),
    };
    let stream = request_body["stream"].as_bool() == Some(true);

    let upstream = http_client()
        .post(format!("{target}/v1/messages"))
        .header(header::AUTHORIZATION, &auth)
        .header("x-api-key", &token)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&anthropic_body)
        .send()
        .await;
    let upstream = match upstream {
        Ok(response) => response,
        Err(e) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("upstream request failed: {e}"),
            )
        }
    };

    let status = upstream.status();
    if !status.is_success() {
        let body: Value = upstream.json().await.unwrap_or_else(|_| json!({}));
        let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        return (
            status_code,
            Json(anthropic_to_openai_error(&body, status.as_u16())),
        )
            .into_response();
    }

    if stream {
        let mut translator = SseTranslator::new();
        let mut upstream_stream = upstream.bytes_stream();
        let body_stream = async_stream::stream! {
            while let Some(chunk) = upstream_stream.next().await {
                let Ok(bytes) = chunk else { break };
                let text = String::from_utf8_lossy(&bytes).into_owned();
                for frame in translator.push(&text) {
                    yield Ok::<_, std::convert::Infallible>(Bytes::from(frame));
                }
            }
        };
        match Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(body_stream))
        {
            Ok(response) => response,
            Err(e) => error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to build streaming response: {e}"),
            ),
        }
    } else {
        match upstream.json::<Value>().await {
            Ok(body) => Json(anthropic_to_openai_response(&body)).into_response(),
            Err(e) => error_response(
                StatusCode::BAD_GATEWAY,
                &format!("invalid upstream response: {e}"),
            ),
        }
    }
}

async fn healthz() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bind = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("BRIDGE_BIND").ok())
        .unwrap_or_else(|| "127.0.0.1:8744".to_string());

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/a/{target}/{*rest}", get(handle_get).post(handle_post));

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!("anthropic-bridge listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
