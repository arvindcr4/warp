//! End-to-end test: a real `run_turn` against a mock OpenAI-compatible
//! provider, verifying the full decode → infer → `ResponseEvent` pipeline,
//! including text output and a tool call.

use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use warp_local_server::run_turn;
use warp_multi_agent_api as api;

async fn mock_completion() -> Json<serde_json::Value> {
    Json(json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Sure — let me list the files.",
                "tool_calls": [{
                    "id": "call_a",
                    "type": "function",
                    "function": {
                        "name": "run_shell_command",
                        "arguments": "{\"command\":\"ls -la\",\"is_read_only\":true}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 8, "total_tokens": 18}
    }))
}

async fn spawn_mock_provider() -> String {
    let app = Router::new().route("/chat/completions", post(mock_completion));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn request_with_user_query(base_url: &str, query: &str) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: "task-1".to_string(),
                messages: vec![api::Message {
                    id: "m1".to_string(),
                    message: Some(api::message::Message::UserQuery(api::message::UserQuery {
                        query: query.to_string(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }),
        settings: Some(api::request::Settings {
            model_config: Some(api::request::settings::ModelConfig {
                base: "cfg-1".to_string(),
                ..Default::default()
            }),
            custom_model_providers: Some(api::request::settings::CustomModelProviders {
                providers: vec![
                    api::request::settings::custom_model_providers::CustomModelProvider {
                        base_url: base_url.to_string(),
                        api_key: "sk-mock".to_string(),
                        models: vec![
                            api::request::settings::custom_model_providers::CustomModel {
                                slug: "MiniMax-M3".to_string(),
                                config_key: "cfg-1".to_string(),
                            },
                        ],
                    },
                ],
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn run_turn_streams_text_and_tool_call() {
    let base_url = spawn_mock_provider().await;
    let client = reqwest::Client::new();
    let request = request_with_user_query(&base_url, "list the files here");

    let events = run_turn(&client, request).await;
    assert_eq!(events.len(), 3, "expected Init, ClientActions, Finished");

    assert!(matches!(
        events[0].r#type,
        Some(api::response_event::Type::Init(_))
    ));

    let Some(api::response_event::Type::ClientActions(actions)) = &events[1].r#type else {
        panic!("expected ClientActions");
    };
    let Some(api::client_action::Action::AddMessagesToTask(add)) = &actions.actions[0].action
    else {
        panic!("expected AddMessagesToTask");
    };
    assert_eq!(add.task_id, "task-1");
    assert_eq!(add.messages.len(), 2, "expected text + tool call");

    let Some(api::message::Message::AgentOutput(ao)) = &add.messages[0].message else {
        panic!("expected AgentOutput first");
    };
    assert_eq!(ao.text, "Sure — let me list the files.");

    let Some(api::message::Message::ToolCall(tc)) = &add.messages[1].message else {
        panic!("expected ToolCall second");
    };
    let Some(api::message::tool_call::Tool::RunShellCommand(cmd)) = &tc.tool else {
        panic!("expected run_shell_command");
    };
    assert_eq!(cmd.command, "ls -la");
    assert!(cmd.is_read_only);

    let Some(api::response_event::Type::Finished(f)) = &events[2].r#type else {
        panic!("expected Finished");
    };
    assert!(matches!(
        f.reason,
        Some(api::response_event::stream_finished::Reason::Done(_))
    ));
}

#[tokio::test]
async fn run_turn_without_provider_emits_error_finish() {
    let client = reqwest::Client::new();
    let events = run_turn(&client, api::Request::default()).await;
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[1].r#type,
        Some(api::response_event::Type::Finished(_))
    ));
}
