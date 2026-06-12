//! warp-max-server: a local agent backend that speaks Warp's multi-agent
//! protocol and drives the user's own LLM keys directly — no Warp account, no
//! Warp cloud. Point a "Warp Max" client at `http://localhost:8765` and every
//! agent request runs on the endpoint/key the user configured in settings
//! (shipped inside each request's `custom_model_providers`).
//!
//! The protocol is a stateless request/response loop: the client POSTs a
//! protobuf `Request` (full conversation history + the new turn) and we stream
//! back base64url-encoded `ResponseEvent` protobufs over SSE. Client-side tool
//! calls (run command, read/apply files) are executed by the client, which
//! then re-POSTs with the results — so we only do one provider turn per request.

use axum::body::{Body, Bytes};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use prost::Message as _;
use serde_json::json;
use warp_local_server::{run_turn, sse};
use warp_multi_agent_api as api;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bind = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("WARP_MAX_BIND").ok())
        .unwrap_or_else(|| "127.0.0.1:8765".to_string());

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/ai/multi-agent", post(handle_multi_agent))
        .route("/agent-mode-evals/multi-agent", post(handle_multi_agent))
        .route("/graphql/v2", post(handle_graphql))
        .fallback(handle_fallback);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("warp-max-server listening on http://{bind}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// The core agent endpoint: decode the protobuf `Request`, run one provider
/// turn, and stream the resulting `ResponseEvent`s back as SSE frames.
async fn handle_multi_agent(body: Bytes) -> Response {
    let request = match api::Request::decode(body.as_ref()) {
        Ok(request) => request,
        Err(e) => {
            return sse_response(vec![
                sse::frame(&sse::init(
                    uuid::Uuid::new_v4().to_string(),
                    uuid::Uuid::new_v4().to_string(),
                )),
                sse::frame(&sse::finished_error(format!(
                    "failed to decode request protobuf: {e}"
                ))),
            ]);
        }
    };

    let client = reqwest::Client::new();
    let events = run_turn(&client, request).await;
    let frames = events.iter().map(sse::frame).collect();
    sse_response(frames)
}

/// Minimal GraphQL surface. The client polls a few queries (model choices,
/// conversation lists, usage); answer with benign empty payloads so nothing
/// errors. AI inference does not depend on these.
async fn handle_graphql(body: Bytes) -> Response {
    let query = String::from_utf8_lossy(&body);
    let data = if query.contains("FeatureModelChoices") || query.contains("featureModelChoices") {
        json!({"data": {"featureModelChoices": null}})
    } else if query.contains("Conversations") || query.contains("conversations") {
        json!({"data": {"listAiConversations": {"conversations": []}}})
    } else {
        json!({"data": null})
    };
    Json(data).into_response()
}

/// Everything else the client might call (REST conversation CRUD, telemetry,
/// etc.) gets an empty-object 200 so the client treats it as a successful no-op.
async fn handle_fallback() -> Response {
    Json(json!({})).into_response()
}

/// Builds an SSE response from pre-rendered frames.
fn sse_response(frames: Vec<String>) -> Response {
    let body: String = frames.concat();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
