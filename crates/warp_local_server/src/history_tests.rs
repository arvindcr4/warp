use super::*;
use warp_multi_agent_api as api;

fn user_message(text: &str) -> api::Message {
    api::Message {
        id: uuid::Uuid::new_v4().to_string(),
        message: Some(api::message::Message::UserQuery(api::message::UserQuery {
            query: text.to_string(),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn agent_message(text: &str) -> api::Message {
    api::Message {
        id: uuid::Uuid::new_v4().to_string(),
        message: Some(api::message::Message::AgentOutput(
            api::message::AgentOutput {
                text: text.to_string(),
            },
        )),
        ..Default::default()
    }
}

fn tool_call_message(tool_call_id: &str, command: &str) -> api::Message {
    api::Message {
        id: uuid::Uuid::new_v4().to_string(),
        message: Some(api::message::Message::ToolCall(api::message::ToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool: Some(api::message::tool_call::Tool::RunShellCommand(
                api::message::tool_call::RunShellCommand {
                    command: command.to_string(),
                    ..Default::default()
                },
            )),
        })),
        ..Default::default()
    }
}

fn tool_result_message(tool_call_id: &str, output: &str) -> api::Message {
    api::Message {
        id: uuid::Uuid::new_v4().to_string(),
        message: Some(api::message::Message::ToolCallResult(
            api::message::ToolCallResult {
                tool_call_id: tool_call_id.to_string(),
                result: Some(api::message::tool_call_result::Result::RunShellCommand(
                    api::RunShellCommandResult {
                        result: Some(api::run_shell_command_result::Result::CommandFinished(
                            api::ShellCommandFinished {
                                output: output.to_string(),
                                exit_code: 0,
                                ..Default::default()
                            },
                        )),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

fn request_with_messages(messages: Vec<api::Message>) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: "task-1".to_string(),
                messages,
                ..Default::default()
            }],
        }),
        ..Default::default()
    }
}

#[test]
fn parallel_tool_calls_merge_and_results_follow() {
    // Two assistant tool calls in a row, then their two results — as Warp
    // stores parallel tool calls. Must collapse to one assistant message with
    // both tool_calls, immediately followed by the two tool results.
    let req = request_with_messages(vec![
        user_message("do two things"),
        tool_call_message("call_1", "ls"),
        tool_call_message("call_2", "pwd"),
        tool_result_message("call_1", "file.txt"),
        tool_result_message("call_2", "/home"),
    ]);
    let msgs = reconstruct_messages(&req);

    assert_eq!(msgs[0]["role"], "user");
    // One merged assistant message carrying both tool calls.
    assert_eq!(msgs[1]["role"], "assistant");
    let calls = msgs[1]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2, "both tool calls merged into one message");
    // Both results immediately follow, in order.
    assert_eq!(msgs[2]["role"], "tool");
    assert_eq!(msgs[2]["tool_call_id"], "call_1");
    assert_eq!(msgs[3]["role"], "tool");
    assert_eq!(msgs[3]["tool_call_id"], "call_2");
    assert_eq!(msgs.len(), 4);
}

#[test]
fn tool_result_immediately_follows_assistant_tool_call() {
    // The single-call sequential case must keep result adjacent to its call.
    let req = request_with_messages(vec![
        user_message("hi"),
        tool_call_message("c1", "ls"),
        tool_result_message("c1", "out"),
        agent_message("done"),
    ]);
    let msgs = reconstruct_messages(&req);
    // Find the assistant-with-tool_calls message; the next must be its result.
    let idx = msgs
        .iter()
        .position(|m| m["tool_calls"].is_array())
        .expect("assistant tool_calls message present");
    assert_eq!(msgs[idx + 1]["role"], "tool");
    assert_eq!(msgs[idx + 1]["tool_call_id"], "c1");
}

#[test]
fn reconstructs_user_and_assistant_turns_in_order() {
    let req = request_with_messages(vec![
        user_message("hi"),
        agent_message("hello there"),
        user_message("write a test"),
    ]);
    let msgs = reconstruct_messages(&req);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "hi");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[1]["content"], "hello there");
    assert_eq!(msgs[2]["content"], "write a test");
}

#[test]
fn target_task_id_uses_last_task() {
    let req = request_with_messages(vec![user_message("hi")]);
    assert_eq!(target_task_id(&req), "task-1");
}

#[test]
fn resolve_provider_matches_config_key() {
    let req = api::Request {
        settings: Some(api::request::Settings {
            model_config: Some(api::request::settings::ModelConfig {
                base: "cfg-key-123".to_string(),
                ..Default::default()
            }),
            custom_model_providers: Some(api::request::settings::CustomModelProviders {
                providers: vec![
                    api::request::settings::custom_model_providers::CustomModelProvider {
                        base_url: "https://api.minimax.io/v1".to_string(),
                        api_key: "sk-cp-test".to_string(),
                        models: vec![
                            api::request::settings::custom_model_providers::CustomModel {
                                slug: "MiniMax-M3".to_string(),
                                config_key: "cfg-key-123".to_string(),
                            },
                        ],
                    },
                ],
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let provider = resolve_provider(&req, None).expect("should resolve");
    assert_eq!(provider.base_url, "https://api.minimax.io/v1");
    assert_eq!(provider.api_key, "sk-cp-test");
    assert_eq!(provider.model, "MiniMax-M3");
}

#[test]
fn input_user_query_not_duplicated_when_already_in_history() {
    let mut req = request_with_messages(vec![user_message("only once")]);
    req.input = Some(api::request::Input {
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs {
                inputs: vec![api::request::input::user_inputs::UserInput {
                    input: Some(
                        api::request::input::user_inputs::user_input::Input::UserQuery(
                            api::request::input::UserQuery {
                                query: "only once".to_string(),
                                ..Default::default()
                            },
                        ),
                    ),
                }],
            },
        )),
        ..Default::default()
    });
    let msgs = reconstruct_messages(&req);
    let user_count = msgs.iter().filter(|m| m["role"] == "user").count();
    assert_eq!(user_count, 1, "user query should not be duplicated");
}
