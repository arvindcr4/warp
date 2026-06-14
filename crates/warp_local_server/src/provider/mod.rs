//! Calls an OpenAI-compatible Chat Completions endpoint (MiniMax `/v1`,
//! OpenAI, OpenRouter, etc.) and returns the assistant's turn.
//!
//! v1 is non-streaming: we request the full completion and map it to Warp
//! response events at once. Token streaming can be layered on later via
//! `AppendToMessageContent`.

use anyhow::{anyhow, Context};
use serde_json::{json, Value};
use warp_multi_agent_api as api;

/// The resolved upstream LLM provider for a request: an OpenAI-compatible
/// Chat Completions endpoint plus the model slug to call.
#[derive(Debug, Clone)]
pub struct Provider {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// Why a request could not be served against any provider. Each variant renders
/// its own user-facing message via [`ConfigError::message`], so the caller maps
/// the error straight to a finished-error event without knowing the details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The resolved endpoint has a blank base URL.
    EmptyBaseUrl,
    /// No API key is available for the resolved endpoint/model.
    EmptyApiKey { base_url: String, model: String },
}

impl ConfigError {
    /// The message shown to the user when configuration is unusable.
    pub fn message(&self) -> String {
        match self {
            ConfigError::EmptyBaseUrl => "Configured endpoint has an empty URL.".to_string(),
            ConfigError::EmptyApiKey { base_url, model } => format!(
                "No API key for the selected model. Make sure the model you picked is your \
                 custom endpoint (e.g. MiniMax Token Plan) configured in Settings → AI → Custom \
                 inference with a non-empty key — or run `warp login --api-key sk-...`. \
                 (Resolved endpoint: {base_url}, model: {model}.)"
            ),
        }
    }
}

/// Resolves the provider/model for this request, validated and ready to call.
///
/// The Warp client ships the user's configured custom endpoints in
/// `settings.custom_model_providers`, and the selected model's `config_key` in
/// `settings.model_config.base`. We find the provider+model whose `config_key`
/// matches and use its `base_url`/`api_key`. Falls back to env overrides
/// (`WARP_MAX_BASE_URL`, `WARP_MAX_API_KEY`, `WARP_MAX_MODEL`) so the server is
/// usable even before a custom endpoint is configured.
///
/// Returns `Err(ConfigError)` — carrying the user-facing message — when the
/// resolved endpoint has no URL or no key, so the caller never inspects the
/// resolved fields itself.
pub fn resolve(
    request: &api::Request,
    auth_header_api_key: Option<String>,
) -> Result<Provider, ConfigError> {
    let provider = resolve_candidate(request, auth_header_api_key);
    if provider.base_url.trim().is_empty() {
        return Err(ConfigError::EmptyBaseUrl);
    }
    if provider.api_key.trim().is_empty() {
        return Err(ConfigError::EmptyApiKey {
            base_url: provider.base_url,
            model: provider.model,
        });
    }
    Ok(provider)
}

/// Picks the candidate provider (custom endpoint match, single-custom fallback,
/// then env defaults) without validating it; [`resolve`] applies validation.
fn resolve_candidate(request: &api::Request, auth_header_api_key: Option<String>) -> Provider {
    let settings = request.settings.as_ref();
    let selected = settings
        .and_then(|s| s.model_config.as_ref())
        .map(|m| m.base.clone())
        .unwrap_or_default();

    if let Some(providers) = settings.and_then(|s| s.custom_model_providers.as_ref()) {
        for provider in &providers.providers {
            for model in &provider.models {
                if model.config_key == selected && !selected.is_empty() {
                    return Provider {
                        base_url: provider.base_url.clone(),
                        api_key: provider.api_key.clone(),
                        model: model.slug.clone(),
                    };
                }
            }
        }
        // No exact config_key match: if there's exactly one custom model, use it.
        if providers.providers.len() == 1 && providers.providers[0].models.len() == 1 {
            let provider = &providers.providers[0];
            let model = &provider.models[0];
            return Provider {
                base_url: provider.base_url.clone(),
                api_key: provider.api_key.clone(),
                model: model.slug.clone(),
            };
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
    Provider {
        base_url,
        api_key,
        model,
    }
}

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

    crate::trace::provider_response(&endpoint, status.as_u16(), &payload);

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

    let tool_calls: Vec<(String, String, String)> = message["tool_calls"]
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

    crate::trace::provider_parsed(text.len(), tool_calls.len());

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

#[cfg(test)]
mod tests;
