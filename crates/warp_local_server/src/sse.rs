//! Encodes `ResponseEvent` protobufs into the SSE frames the Warp client
//! expects: each event's `data` is the base64url-encoded protobuf, terminated
//! by a blank line. The client decodes `message.data` (trimmed of quotes) as
//! base64url then `ResponseEvent::decode`.

use base64::Engine as _;
use prost::Message as _;
use uuid::Uuid;
use warp_multi_agent_api as api;

/// Serializes a `ResponseEvent` into one SSE `data:` frame.
pub fn frame(event: &api::ResponseEvent) -> String {
    let bytes = event.encode_to_vec();
    // The client decodes with the padded URL-safe alphabet
    // (`base64::prelude::BASE64_URL_SAFE`), so encode with padding to match.
    let encoded = base64::engine::general_purpose::URL_SAFE.encode(bytes);
    format!("data: {encoded}\n\n")
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
#[path = "sse_tests.rs"]
mod tests;
