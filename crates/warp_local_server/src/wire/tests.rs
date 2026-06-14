use base64::prelude::{Engine as _, BASE64_URL_SAFE};
use prost::Message as _;

use super::*;

/// Decodes a frame exactly as the client does: strip the `data: ` prefix and
/// trailing blank line, trim quotes, base64url(padded)-decode, then
/// `ResponseEvent::decode`.
fn decode_like_client(frame: &str) -> api::ResponseEvent {
    let data = frame
        .strip_prefix("data: ")
        .unwrap()
        .trim_end()
        .trim_matches('"');
    let bytes = BASE64_URL_SAFE.decode(data).expect("client base64 decode");
    api::ResponseEvent::decode(bytes.as_slice()).expect("client proto decode")
}

#[test]
fn init_frame_round_trips_through_client_decode() {
    let event = init("conv-1".to_string(), "run-1".to_string());
    let decoded = decode_like_client(&frame(&event));
    match decoded.r#type {
        Some(api::response_event::Type::Init(i)) => {
            assert_eq!(i.conversation_id, "conv-1");
            assert_eq!(i.run_id, "run-1");
            assert!(!i.request_id.is_empty());
        }
        other => panic!("expected Init, got {other:?}"),
    }
}

#[test]
fn client_actions_frame_round_trips() {
    let msg = api::Message {
        id: "m1".to_string(),
        message: Some(api::message::Message::AgentOutput(
            api::message::AgentOutput {
                text: "hello world".to_string(),
            },
        )),
        ..Default::default()
    };
    let event = client_actions(vec![add_messages("task-1".to_string(), vec![msg])]);
    let decoded = decode_like_client(&frame(&event));
    let Some(api::response_event::Type::ClientActions(actions)) = decoded.r#type else {
        panic!("expected ClientActions");
    };
    let Some(api::client_action::Action::AddMessagesToTask(add)) = &actions.actions[0].action
    else {
        panic!("expected AddMessagesToTask");
    };
    assert_eq!(add.task_id, "task-1");
    let Some(api::message::Message::AgentOutput(ao)) = &add.messages[0].message else {
        panic!("expected AgentOutput");
    };
    assert_eq!(ao.text, "hello world");
}

#[test]
fn finished_done_frame_round_trips() {
    let decoded = decode_like_client(&frame(&finished_done()));
    let Some(api::response_event::Type::Finished(f)) = decoded.r#type else {
        panic!("expected Finished");
    };
    assert!(matches!(
        f.reason,
        Some(api::response_event::stream_finished::Reason::Done(_))
    ));
}

#[test]
fn turn_frames_ok_as_init_actions_finished() {
    let events = turn(
        "c".to_string(),
        "r".to_string(),
        Ok(vec![add_messages("t".to_string(), vec![])]),
    );
    assert_eq!(events.len(), 3);
    assert!(matches!(
        events[0].r#type,
        Some(api::response_event::Type::Init(_))
    ));
    assert!(matches!(
        events[1].r#type,
        Some(api::response_event::Type::ClientActions(_))
    ));
    assert!(matches!(
        events[2].r#type,
        Some(api::response_event::Type::Finished(_))
    ));
}

#[test]
fn turn_frames_err_as_init_then_finished_error() {
    let events = turn("c".to_string(), "r".to_string(), Err("boom".to_string()));
    assert_eq!(events.len(), 2);
    let Some(api::response_event::Type::Finished(f)) = &events[1].r#type else {
        panic!("expected Finished");
    };
    let Some(api::response_event::stream_finished::Reason::InternalError(e)) = &f.reason else {
        panic!("expected InternalError");
    };
    assert_eq!(e.message, "boom");
}

#[test]
fn decode_request_extracts_bearer_key() {
    let bytes = api::Request::default().encode_to_vec();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Authorization", "Bearer sk-xyz".parse().unwrap());
    let (_req, key) = decode_request(&bytes, &headers).expect("decodes");
    assert_eq!(key.as_deref(), Some("sk-xyz"));
}

#[test]
fn decode_request_without_auth_header_yields_no_key() {
    let bytes = api::Request::default().encode_to_vec();
    let headers = axum::http::HeaderMap::new();
    let (_req, key) = decode_request(&bytes, &headers).expect("decodes");
    assert!(key.is_none());
}
