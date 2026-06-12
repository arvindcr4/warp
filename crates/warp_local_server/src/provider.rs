//! Calls an OpenAI-compatible Chat Completions endpoint (MiniMax `/v1`,
//! OpenAI, OpenRouter, etc.) and returns the assistant's turn.
//!
//! v1 is non-streaming: we request the full completion and map it to Warp
//! response events at once. Token streaming can be layered on later via
//! `AppendToMessageContent`.

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

use crate::history::Provider;

/// One assistant turn from the provider.
#[derive(Debug, Default)]
pub struct ProviderTurn {
    pub text: String,
    /// (tool_call_id, function_name, raw_json_arguments)
    pub tool_calls: Vec<(String, String, String)>,
}

/// Issues a Chat Completions request and parses the first choice.
pub async fn call(
    client: &reqwest::Client,
    provider: &Provider,
    messages: Vec<Value>,
    tools: Vec<Value>,
) -> anyhow::Result<ProviderTurn> {
    let endpoint = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": provider.model,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "stream": false,
    });

    let response = client
        .post(&endpoint)
        .bearer_auth(&provider.api_key)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("request to {endpoint} failed"))?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .context("provider response was not valid JSON")?;

    let debug = std::env::var("WARP_MAX_DEBUG").is_ok();
    if debug {
        eprintln!(
            "warp-max-server: provider {} -> status {} body {}",
            endpoint,
            status.as_u16(),
            truncate(&payload.to_string(), 4000)
        );
    }

    if !status.is_success() {
        let message = payload["error"]["message"]
            .as_str()
            .unwrap_or("provider returned an error");
        return Err(anyhow!("provider error ({}): {message}", status.as_u16()));
    }

    let message = &payload["choices"][0]["message"];
    let mut text = extract_text(&message["content"]);
    // Some reasoning models put the user-facing answer only in
    // `reasoning_content` when no separate content is produced.
    if text.trim().is_empty() {
        text = message["reasoning_content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
    }
    // MiniMax M-series embed chain-of-thought as <think>...</think> inside the
    // content. Strip it so only the user-facing answer is shown.
    text = strip_think_blocks(&text);

    let tool_calls = message["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .filter(|c| c["type"] == "function" || c.get("function").is_some())
                .map(|c| {
                    let id = c["id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                    let name = c["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let arguments = c["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_string();
                    (id, name, arguments)
                })
                .collect()
        })
        .unwrap_or_default();

    if debug {
        eprintln!(
            "warp-max-server: parsed text_len={} tool_calls={}",
            text.len(),
            (&tool_calls as &Vec<(String, String, String)>).len()
        );
    }

    Ok(ProviderTurn { text, tool_calls })
}

/// Extracts assistant text from an OpenAI `message.content`, which may be a
/// plain string or an array of content parts (`{type:"text", text:"..."}`).
fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…[truncated]")
    }
}

/// Removes `<think>...</think>` reasoning blocks from model output, returning
/// the trimmed user-facing answer. Handles an unclosed `<think>` (drops to end)
/// and multiple blocks.
fn strip_think_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("<think>") {
        out.push_str(&rest[..open]);
        match rest[open..].find("</think>") {
            Some(close) => rest = &rest[open + close + "</think>".len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}
