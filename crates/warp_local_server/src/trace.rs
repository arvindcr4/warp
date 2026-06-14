//! Observability for one agent turn, behind a single seam. Each function
//! decides whether and how to emit — some always log to stderr, the verbose
//! dumps are gated by the `WARP_MAX_DEBUG` env var — so `run_turn` and
//! `provider::call` read as a pipeline instead of interleaving logging with
//! logic.

use serde_json::Value;
use warp_multi_agent_api as api;

use crate::mcp::McpRegistry;
use crate::provider::Provider;

fn debug_enabled() -> bool {
    std::env::var("WARP_MAX_DEBUG").is_ok()
}

/// Logs the resolved provider endpoint/model.
///
/// The API key length is intentionally NOT logged: it would be a side-channel
/// that aids key fingerprinting and rotation tracking. If such a value is ever
/// needed, gate it behind `debug_enabled()` (the `WARP_MAX_DEBUG` switch) AND
/// wrap it in `safe_info!` from `warp_core::safe_log` so the dogfood/release
/// distinction still applies.
pub fn resolved_provider(provider: &Provider, custom_provider_count: usize) {
    eprintln!(
        "warp-max-server: resolved base_url={} model={} custom_providers={}",
        provider.base_url, provider.model, custom_provider_count
    );
}

/// Logs the reconstructed message shape (role sequence, tool-call counts).
pub fn message_shape(messages: &[Value]) {
    let shape: Vec<String> = messages
        .iter()
        .map(|m| {
            let role = m["role"].as_str().unwrap_or("?");
            if let Some(tc) = m["tool_calls"].as_array() {
                format!("{role}[tc:{}]", tc.len())
            } else if role == "tool" {
                format!("tool({})", m["tool_call_id"].as_str().unwrap_or(""))
            } else {
                role.to_string()
            }
        })
        .collect();
    eprintln!("warp-max-server: messages=[{}]", shape.join(", "));
}

/// Verbose per-turn dump (only under `WARP_MAX_DEBUG`).
pub fn turn_dump(request: &api::Request, task_id: &str, provider: &Provider, history_msgs: usize) {
    if !debug_enabled() {
        return;
    }
    let tasks_dump: Vec<String> = request
        .task_context
        .as_ref()
        .map(|tc| {
            tc.tasks
                .iter()
                .map(|t| format!("{}#msgs={}", t.id, t.messages.len()))
                .collect()
        })
        .unwrap_or_default();
    let input_kind = request
        .input
        .as_ref()
        .and_then(|i| i.r#type.as_ref())
        .map(|t| format!("{t:?}").chars().take(40).collect::<String>())
        .unwrap_or_else(|| "none".to_string());
    eprintln!(
        "warp-max-server: turn task_id={} base_url={} model={} history_msgs={} tasks=[{}] input={}",
        task_id,
        provider.base_url,
        provider.model,
        history_msgs,
        tasks_dump.join(", "),
        input_kind
    );
}

/// Logs how many MCP tools were advertised, if any.
pub fn mcp_advertised(mcp: &McpRegistry) {
    if !mcp.is_empty() {
        eprintln!(
            "warp-max-server: advertising {} MCP tool(s)",
            mcp.tools.len()
        );
    }
}

/// Logs a failed provider call.
pub fn provider_call_failed(error: &anyhow::Error) {
    eprintln!("warp-max-server: provider call failed: {error:#}");
}

/// Verbose dump of the raw provider response (only under `WARP_MAX_DEBUG`).
pub fn provider_response(endpoint: &str, status: u16, payload: &Value) {
    if !debug_enabled() {
        return;
    }
    eprintln!(
        "warp-max-server: provider {} -> status {} body {}",
        endpoint,
        status,
        truncate(&payload.to_string(), 4000)
    );
}

/// Verbose dump of the parsed turn (only under `WARP_MAX_DEBUG`).
pub fn provider_parsed(text_len: usize, tool_calls: usize) {
    if !debug_enabled() {
        return;
    }
    eprintln!("warp-max-server: parsed text_len={text_len} tool_calls={tool_calls}");
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…[truncated]")
    }
}
