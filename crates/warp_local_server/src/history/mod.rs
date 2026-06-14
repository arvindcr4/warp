//! Reconstructs an OpenAI-format chat history from a decoded Warp `Request`,
//! and reads the routing ids (task, conversation) the response must echo back.

use serde_json::{json, Value};
use warp_multi_agent_api as api;

use crate::tools;

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
/// The returned transcript is guaranteed provider-valid: assistant tool calls
/// are paired with their results, results follow their call directly, and any
/// unpaired call or orphan result is dropped — the whole reason this module
/// exists (it avoids MiniMax error 2013, "tool call and result not match").
/// That guarantee lives in one place: [`MessageBuilder`], whose `finish`
/// reconciles pairing as its last step.
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
    builder.finish()
}

/// Accumulates OpenAI-format chat messages and produces a provider-valid
/// transcript. It buffers assistant tool calls so parallel/consecutive calls
/// collapse into one assistant message and tool results always follow it
/// directly (adjacency), and its `finish` reconciles pairing so unpaired calls
/// and orphan results are dropped (completeness). Adjacency and completeness —
/// the full MiniMax-2013 invariant — therefore have a single owner.
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

    /// Flushes any buffered tool calls and returns the paired history. This is
    /// the builder's single guarantee: every assistant `tool_calls` entry has a
    /// matching `tool` result and vice versa, so callers never need a separate
    /// sanitize pass.
    fn finish(mut self) -> Vec<Value> {
        self.flush_tool_calls();
        reconcile_pairing(self.messages)
    }
}

/// Enforces the provider invariant that every assistant `tool_calls` entry has
/// exactly one matching `tool` result and vice versa. Drops assistant tool
/// calls whose result is missing and orphan tool results whose call is missing
/// (which otherwise trigger MiniMax's "tool call and result not match" (2013)).
///
/// Private to this module: callers reach it only through
/// [`MessageBuilder::finish`], so the invariant has a single entry point.
fn reconcile_pairing(messages: Vec<Value>) -> Vec<Value> {
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
mod tests;
