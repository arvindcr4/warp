//! Reconstructs an OpenAI-format chat history and resolves the target provider
//! from a decoded Warp `Request`.

use serde_json::{json, Value};
use warp_multi_agent_api as api;

use crate::tools;

/// The resolved upstream LLM provider for a request: an OpenAI-compatible
/// Chat Completions endpoint plus the model slug to call.
#[derive(Debug, Clone)]
pub struct Provider {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// Resolves the provider/model for this request.
///
/// The Warp client ships the user's configured custom endpoints in
/// `settings.custom_model_providers`, and the selected model's `config_key` in
/// `settings.model_config.base`. We find the provider+model whose `config_key`
/// matches and use its `base_url`/`api_key`. Falls back to env overrides
/// (`WARP_MAX_BASE_URL`, `WARP_MAX_API_KEY`, `WARP_MAX_MODEL`) so the server is
/// usable even before a custom endpoint is configured.
pub fn resolve_provider(
    request: &api::Request,
    auth_header_api_key: Option<String>,
) -> Option<Provider> {
    let settings = request.settings.as_ref();
    let selected = settings
        .and_then(|s| s.model_config.as_ref())
        .map(|m| m.base.clone())
        .unwrap_or_default();

    if let Some(providers) = settings.and_then(|s| s.custom_model_providers.as_ref()) {
        for provider in &providers.providers {
            for model in &provider.models {
                if model.config_key == selected && !selected.is_empty() {
                    return Some(Provider {
                        base_url: provider.base_url.clone(),
                        api_key: provider.api_key.clone(),
                        model: model.slug.clone(),
                    });
                }
            }
        }
        // No exact config_key match: if there's exactly one custom model, use it.
        if providers.providers.len() == 1 && providers.providers[0].models.len() == 1 {
            let provider = &providers.providers[0];
            let model = &provider.models[0];
            return Some(Provider {
                base_url: provider.base_url.clone(),
                api_key: provider.api_key.clone(),
                model: model.slug.clone(),
            });
        }
    }

    // Env fallback.
    let base_url = std::env::var("WARP_MAX_BASE_URL")
        .ok()
        .unwrap_or_else(|| "https://api.minimax.chat/v1".to_string());
    let api_key = auth_header_api_key
        .unwrap_or_else(|| std::env::var("WARP_MAX_API_KEY").unwrap_or_default());
    let model = std::env::var("WARP_MAX_MODEL").unwrap_or_else(|_| {
        if selected.is_empty() {
            "gpt-4o".to_string()
        } else {
            selected.clone()
        }
    });
    Some(Provider {
        base_url,
        api_key,
        model,
    })
}

/// The task id that response messages should be added to.
///
/// The Warp client registers a streaming "exchange" keyed by the task id that
/// holds the user's input, and requires the server's `AddMessagesToTask` to use
/// that exact id (otherwise the response is dropped with `ExchangeNotFound`).
/// So we prefer the last task that contains a `UserQuery`/`ToolCallResult`
/// (the active exchange's task), then fall back to the last task, then the
/// first task.
pub fn target_task_id(request: &api::Request) -> String {
    let tasks = request
        .task_context
        .as_ref()
        .map(|tc| tc.tasks.as_slice())
        .unwrap_or(&[]);

    // Last task whose messages include a user query or tool result.
    let active = tasks.iter().rev().find(|task| {
        task.messages.iter().any(|m| {
            matches!(
                m.message,
                Some(api::message::Message::UserQuery(_))
                    | Some(api::message::Message::ToolCallResult(_))
            )
        })
    });

    active
        .or_else(|| tasks.last())
        .or_else(|| tasks.first())
        .map(|t| t.id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

/// Echoes the conversation id from the request metadata, or empty to let the
/// caller mint a fresh one for a new conversation.
pub fn conversation_id(request: &api::Request) -> String {
    request
        .metadata
        .as_ref()
        .map(|m| m.conversation_id.clone())
        .unwrap_or_default()
}

/// Reconstructs the OpenAI-format `messages` array from the request's task
/// history plus the new turn in `input`.
///
/// Consecutive assistant tool calls are merged into a single assistant message
/// with a `tool_calls` array, and any buffered tool calls are flushed
/// immediately before a tool result — this preserves the strict ordering
/// providers like MiniMax enforce ("tool call result must follow tool call").
pub fn reconstruct_messages(request: &api::Request) -> Vec<Value> {
    let mut builder = MessageBuilder::default();

    if let Some(tc) = request.task_context.as_ref() {
        for task in &tc.tasks {
            for message in &task.messages {
                append_history_message(message, &mut builder);
            }
        }
    }

    append_input(request, &mut builder);
    let messages = builder.finish();
    sanitize_tool_pairing(messages)
}

/// Enforces the provider invariant that every assistant `tool_calls` entry has
/// exactly one matching `tool` result and vice versa. Drops assistant tool
/// calls whose result is missing and orphan tool results whose call is missing
/// (which otherwise trigger MiniMax's "tool call and result not match" (2013)).
fn sanitize_tool_pairing(messages: Vec<Value>) -> Vec<Value> {
    use std::collections::HashSet;

    let call_ids: HashSet<String> = messages
        .iter()
        .filter_map(|m| m["tool_calls"].as_array())
        .flatten()
        .filter_map(|c| c["id"].as_str().map(str::to_owned))
        .collect();
    let result_ids: HashSet<String> = messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .filter_map(|m| m["tool_call_id"].as_str().map(str::to_owned))
        .collect();
    let valid: HashSet<&String> = call_ids.intersection(&result_ids).collect();

    let mut out = Vec::with_capacity(messages.len());
    for mut message in messages {
        if message["role"] == "tool" {
            // Drop orphan tool results.
            let keep = message["tool_call_id"]
                .as_str()
                .is_some_and(|id| valid.contains(&id.to_string()));
            if keep {
                out.push(message);
            }
            continue;
        }
        if message["tool_calls"].is_array() {
            // Retain only tool calls that have a matching result.
            let kept: Vec<Value> = message["tool_calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| {
                    c["id"]
                        .as_str()
                        .is_some_and(|id| valid.contains(&id.to_string()))
                })
                .cloned()
                .collect();
            let has_content = message["content"].as_str().is_some_and(|s| !s.is_empty());
            if kept.is_empty() {
                // No surviving tool calls: keep as a content message, or drop.
                if has_content {
                    out.push(json!({"role": "assistant", "content": message["content"].clone()}));
                }
                continue;
            }
            message["tool_calls"] = Value::Array(kept);
            out.push(message);
            continue;
        }
        out.push(message);
    }
    out
}

/// Accumulates OpenAI-format chat messages, buffering assistant tool calls so
/// that parallel/consecutive calls collapse into one assistant message and tool
/// results always follow it directly.
#[derive(Default)]
struct MessageBuilder {
    messages: Vec<Value>,
    pending_tool_calls: Vec<Value>,
    seen_tool_results: std::collections::HashSet<String>,
}

impl MessageBuilder {
    fn flush_tool_calls(&mut self) {
        if !self.pending_tool_calls.is_empty() {
            self.messages.push(json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": std::mem::take(&mut self.pending_tool_calls),
            }));
        }
    }

    fn push_user(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.flush_tool_calls();
        let dup = matches!(self.messages.last(), Some(last) if last["role"] == "user" && last["content"] == text);
        if !dup {
            self.messages.push(json!({"role": "user", "content": text}));
        }
    }

    fn push_assistant_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.flush_tool_calls();
        self.messages
            .push(json!({"role": "assistant", "content": text}));
    }

    fn add_tool_call(&mut self, call: Value) {
        self.pending_tool_calls.push(call);
    }

    fn push_tool_result(&mut self, tool_call_id: &str, content: String) {
        if self.seen_tool_results.contains(tool_call_id) {
            return;
        }
        self.seen_tool_results.insert(tool_call_id.to_string());
        self.flush_tool_calls();
        self.messages.push(json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": content,
        }));
    }

    fn finish(mut self) -> Vec<Value> {
        self.flush_tool_calls();
        self.messages
    }
}

fn append_history_message(message: &api::Message, builder: &mut MessageBuilder) {
    use api::message::Message as M;
    let Some(inner) = message.message.as_ref() else {
        return;
    };
    match inner {
        M::UserQuery(uq) => builder.push_user(&uq.query),
        M::AgentOutput(ao) => builder.push_assistant_text(&ao.text),
        M::ToolCall(tc) => {
            if let Some(call) = tools::warp_tool_call_to_openai(tc) {
                builder.add_tool_call(call);
            }
        }
        M::ToolCallResult(tr) => {
            builder.push_tool_result(&tr.tool_call_id, tools::history_tool_result_to_text(tr));
        }
        _ => {}
    }
}

fn append_input(request: &api::Request, builder: &mut MessageBuilder) {
    let Some(input) = request.input.as_ref() else {
        return;
    };
    let Some(api::request::input::Type::UserInputs(user_inputs)) = input.r#type.as_ref() else {
        return;
    };
    for user_input in &user_inputs.inputs {
        use api::request::input::user_inputs::user_input::Input as I;
        match user_input.input.as_ref() {
            Some(I::UserQuery(uq)) => builder.push_user(&uq.query),
            Some(I::ToolCallResult(tr)) => {
                builder.push_tool_result(&tr.tool_call_id, tools::input_tool_result_to_text(tr));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
