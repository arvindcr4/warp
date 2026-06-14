//! The Warp wire contract, in one place: the byte-level agreement with the
//! client for both directions of a multi-agent request.
//!
//! Inbound, [`decode_request`] turns the request body + `Authorization` header
//! into a protobuf `Request` and the optional Bearer key. Outbound,
//! [`encode_events`] renders `ResponseEvent`s into the SSE frames the client
//! expects: each event's `data` is the base64url-encoded protobuf, terminated by
//! a blank line. The client decodes `message.data` (trimmed of quotes) as
//! base64url then `ResponseEvent::decode`. The event constructors below are the
//! vocabulary the encoder serializes, and [`turn`] owns their ordering.

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use prost::Message as _;
use uuid::Uuid;
use warp_multi_agent_api as api;

/// The maximum request body we accept. Agent requests carry the full
/// conversation history plus attached context and routinely exceed axum's 2 MB
/// default; the default limit rejects them mid-upload, which the client surfaces
/// as "Warp lost connection while receiving the agent response".
pub const MAX_BODY_BYTES: usize = 256 * 1024 * 1024;

/// Decodes a multi-agent request off the wire: the protobuf `Request` body plus
/// the optional Bearer API key from the `Authorization` header. On a malformed
/// body, returns a ready-to-send SSE error response instead of the request, so
/// the caller never touches framing.
// The `Err` carries a built `Response` on purpose — it is returned to the
// client immediately, so boxing it to shrink the variant would only add a heap
// allocation on the error path.
#[allow(clippy::result_large_err)]
pub fn decode_request(
    body: &[u8],
    headers: &HeaderMap,
) -> Result<(api::Request, Option<String>), Response> {
    let auth_header_api_key = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.to_string());
    match api::Request::decode(body) {
        Ok(request) => Ok((request, auth_header_api_key)),
        Err(e) => Err(encode_events(&[
            init(Uuid::new_v4().to_string(), Uuid::new_v4().to_string()),
            finished_error(format!("failed to decode request protobuf: {e}")),
        ])),
    }
}

/// Renders streamed `ResponseEvent`s into an SSE response the Warp client can
/// consume: each event base64url-encoded as one `data:` frame.
pub fn encode_events(events: &[api::ResponseEvent]) -> Response {
    let body: String = events.iter().map(frame).collect();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Serializes a `ResponseEvent` into one SSE `data:` frame.
pub fn frame(event: &api::ResponseEvent) -> String {
    let bytes = event.encode_to_vec();
    // The client decodes with the padded URL-safe alphabet
    // (`base64::prelude::BASE64_URL_SAFE`), so encode with padding to match.
    let encoded = base64::engine::general_purpose::URL_SAFE.encode(bytes);
    format!("data: {encoded}\n\n")
}

/// Frames one agent turn into the ordered events the client expects: always
/// `Init` first, then either the turn's `ClientAction`s followed by
/// `Finished(Done)`, or `Finished(InternalError)` on `Err`. Centralizing the
/// ordering here means callers produce a `Result` and can't emit an
/// out-of-order or unterminated stream.
pub fn turn(
    conversation_id: String,
    run_id: String,
    result: Result<Vec<api::ClientAction>, String>,
) -> Vec<api::ResponseEvent> {
    let init = init(conversation_id, run_id);
    match result {
        Ok(actions) => vec![init, client_actions(actions), finished_done()],
        Err(message) => vec![init, finished_error(message)],
    }
}

/// The first event of every stream: assigns/echoes the conversation id and a
/// fresh request id. `conversation_id` should be stable across a conversation;
/// echo back the one from the request when present, else mint a new one.
pub fn init(conversation_id: String, run_id: String) -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::Init(
            api::response_event::StreamInit {
                conversation_id,
                request_id: Uuid::new_v4().to_string(),
                run_id,
            },
        )),
    }
}

/// Wraps a batch of `ClientAction`s into a `ResponseEvent`.
pub fn client_actions(actions: Vec<api::ClientAction>) -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::ClientActions(
            api::response_event::ClientActions { actions },
        )),
    }
}

/// The terminal event for a gracefully finished stream.
pub fn finished_done() -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::Finished(
            api::response_event::StreamFinished {
                reason: Some(api::response_event::stream_finished::Reason::Done(
                    api::response_event::stream_finished::Done {},
                )),
                ..Default::default()
            },
        )),
    }
}

/// A terminal event signaling an internal error, surfaced to the user.
pub fn finished_error(message: String) -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::Finished(
            api::response_event::StreamFinished {
                reason: Some(api::response_event::stream_finished::Reason::InternalError(
                    api::response_event::stream_finished::InternalError { message },
                )),
                ..Default::default()
            },
        )),
    }
}

/// Builds an `AddMessagesToTask` client action.
pub fn add_messages(task_id: String, messages: Vec<api::Message>) -> api::ClientAction {
    api::ClientAction {
        action: Some(api::client_action::Action::AddMessagesToTask(
            api::client_action::AddMessagesToTask { task_id, messages },
        )),
    }
}

/// Builds a `CreateTask` client action for a new root task. For a new
/// conversation the client sends no tasks and expects the server to create the
/// root task; the client then re-keys its pending exchange to this task id, so
/// subsequent `AddMessagesToTask` with the same id renders correctly.
pub fn create_task(task_id: String) -> api::ClientAction {
    api::ClientAction {
        action: Some(api::client_action::Action::CreateTask(
            api::client_action::CreateTask {
                task: Some(api::Task {
                    id: task_id,
                    ..Default::default()
                }),
            },
        )),
    }
}

#[cfg(test)]
mod tests;
