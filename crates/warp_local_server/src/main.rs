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
use axum::extract::DefaultBodyLimit;
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
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/ai/multi-agent", post(handle_multi_agent))
        .route("/agent-mode-evals/multi-agent", post(handle_multi_agent))
        .route("/graphql/v2", post(handle_graphql))
        .fallback(handle_fallback)
        // Agent requests carry the full conversation history plus attached
        // context and routinely exceed axum's 2 MB default body limit; the
        // limit rejects them mid-upload, which the client surfaces as
        // "Warp lost connection while receiving the agent response".
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024));

    // An explicit bind (CLI arg or WARP_MAX_BIND) uses a single address.
    // Otherwise bind BOTH loopback families on port 8765 so the client's
    // `localhost` connects regardless of whether it resolves to 127.0.0.1 or
    // ::1 (macOS resolves localhost to both; hyper may try ::1 first).
    let explicit_bind = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("WARP_MAX_BIND").ok());

    if let Some(bind) = explicit_bind {
        let listener = tokio::net::TcpListener::bind(&bind).await?;
        eprintln!("warp-max-server listening on http://{bind}");
        axum::serve(listener, app).await?;
        return Ok(());
    }

    let v4 = tokio::net::TcpListener::bind("127.0.0.1:8765").await?;
    eprintln!("warp-max-server listening on http://127.0.0.1:8765");
    let mut tasks = Vec::new();
    {
        let app = app.clone();
        tasks.push(tokio::spawn(async move { axum::serve(v4, app).await }));
    }
    // IPv6 loopback is best-effort: skip if the system has IPv6 disabled.
    match tokio::net::TcpListener::bind("[::1]:8765").await {
        Ok(v6) => {
            eprintln!("warp-max-server listening on http://[::1]:8765");
            let app = app.clone();
            tasks.push(tokio::spawn(async move { axum::serve(v6, app).await }));
        }
        Err(e) => eprintln!("warp-max-server: skipping IPv6 loopback ([::1]:8765): {e}"),
    }
    for task in tasks {
        let _ = task.await;
    }
    Ok(())
}

/// The core agent endpoint: decode the protobuf `Request`, run one provider
/// turn, and stream the resulting `ResponseEvent`s back as SSE frames.
async fn handle_multi_agent(headers: axum::http::HeaderMap, body: Bytes) -> Response {
    let auth_header_api_key = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.to_string());
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
    let events = run_turn(&client, request, auth_header_api_key).await;
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
    } else if query.contains("GetRequestLimitInfo") || query.contains("getRequestLimitInfo") {
        json!({"data": {"user": {
            "__typename": "UserOutput",
            "principalType": "USER",
            "apiKeyOwnerType": null,
            "user": {
                "__typename": "User",
                "workspaces": [],
                "requestLimitInfo": {
                    "__typename": "RequestLimitInfo",
                    "isUnlimited": true,
                    "requestsUsedSinceLastRefresh": 0,
                    "requestLimit": 999999,
                    "nextRefreshTime": "2099-01-01T00:00:00Z",
                    "requestLimitRefreshDuration": "MONTHLY"
                },
                "bonusGrants": []
            }
        }}})
    } else if query.contains("GetUser") || query.contains("getUser") {
        json!({"data": {"user": {
            "__typename": "UserOutput",
            "principalType": "USER",
            "apiKeyOwnerType": null,
            "user": {
                "__typename": "User",
                "isOnboarded": true,
                "isOnWorkDomain": false,
                "globalSkills": [],
                "experiments": [],
                "anonymousUserInfo": null,
                "llms": {
                    "__typename": "FeatureModelChoice",
                    "agentMode": { "defaultId": "", "choices": [], "preferredCodexModelId": null },
                    "planning": { "defaultId": "", "choices": [], "preferredCodexModelId": null },
                    "coding": { "defaultId": "", "choices": [], "preferredCodexModelId": null },
                    "cliAgent": { "defaultId": "", "choices": [], "preferredCodexModelId": null },
                    "computerUseAgent": { "defaultId": "", "choices": [], "preferredCodexModelId": null }
                },
                "profile": {
                    "__typename": "FirebaseProfile",
                    "uid": "warp-max-user",
                    "displayName": "Warp Max User",
                    "email": "warp-max@localhost",
                    "needsSsoLink": false,
                    "photoUrl": null
                }
            }
        }}})
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
