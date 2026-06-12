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
pub fn resolve_provider(request: &api::Request) -> Option<Provider> {
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
        if let Some(provider) = providers.providers.first() {
            if let Some(model) = provider.models.first() {
                return Some(Provider {
                    base_url: provider.base_url.clone(),
                    api_key: provider.api_key.clone(),
                    model: model.slug.clone(),
                });
            }
        }
    }

    // Env fallback.
    let base_url = std::env::var("WARP_MAX_BASE_URL").ok()?;
    let api_key = std::env::var("WARP_MAX_API_KEY").unwrap_or_default();
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

/// The task id that response messages should be added to. Prefers the last
/// task in the request (the active one); mints a fallback if none exist.
pub fn target_task_id(request: &api::Request) -> String {
    request
        .task_context
        .as_ref()
        .and_then(|tc| tc.tasks.last())
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
pub fn reconstruct_messages(request: &api::Request) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut seen_tool_results: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Some(tc) = request.task_context.as_ref() {
        for task in &tc.tasks {
            for message in &task.messages {
                append_history_message(message, &mut messages, &mut seen_tool_results);
            }
        }
    }

    append_input(request, &mut messages, &seen_tool_results);
    messages
}

fn append_history_message(
    message: &api::Message,
    out: &mut Vec<Value>,
    seen_tool_results: &mut std::collections::HashSet<String>,
) {
    use api::message::Message as M;
    let Some(inner) = message.message.as_ref() else {
        return;
    };
    match inner {
        M::UserQuery(uq) => push_user(out, &uq.query),
        M::AgentOutput(ao) => {
            if !ao.text.is_empty() {
                out.push(json!({"role": "assistant", "content": ao.text}));
            }
        }
        M::ToolCall(tc) => {
            if let Some(call) = tools::warp_tool_call_to_openai(tc) {
                out.push(json!({
                    "role": "assistant",
                    "content": Value::Null,
                    "tool_calls": [call],
                }));
            }
        }
        M::ToolCallResult(tr) => {
            seen_tool_results.insert(tr.tool_call_id.clone());
            out.push(json!({
                "role": "tool",
                "tool_call_id": tr.tool_call_id,
                "content": tools::history_tool_result_to_text(tr),
            }));
        }
        _ => {}
    }
}

fn append_input(
    request: &api::Request,
    out: &mut Vec<Value>,
    seen_tool_results: &std::collections::HashSet<String>,
) {
    let Some(input) = request.input.as_ref() else {
        return;
    };
    let Some(api::request::input::Type::UserInputs(user_inputs)) = input.r#type.as_ref() else {
        return;
    };
    for user_input in &user_inputs.inputs {
        use api::request::input::user_inputs::user_input::Input as I;
        match user_input.input.as_ref() {
            Some(I::UserQuery(uq)) => {
                let already = matches!(out.last(), Some(last) if last["role"] == "user" && last["content"] == uq.query);
                if !already {
                    push_user(out, &uq.query);
                }
            }
            Some(I::ToolCallResult(tr)) => {
                if !seen_tool_results.contains(&tr.tool_call_id) {
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": tr.tool_call_id,
                        "content": tools::input_tool_result_to_text(tr),
                    }));
                }
            }
            _ => {}
        }
    }
}

fn push_user(out: &mut Vec<Value>, text: &str) {
    if text.is_empty() {
        return;
    }
    out.push(json!({"role": "user", "content": text}));
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
