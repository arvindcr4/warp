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

    if !status.is_success() {
        let message = payload["error"]["message"]
            .as_str()
            .unwrap_or("provider returned an error");
        return Err(anyhow!("provider error ({}): {message}", status.as_u16()));
    }

    let message = &payload["choices"][0]["message"];
    let text = message["content"].as_str().unwrap_or_default().to_string();

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

    Ok(ProviderTurn { text, tool_calls })
}
