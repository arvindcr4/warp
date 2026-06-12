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
    let provider = resolve_provider(&req).expect("should resolve");
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
