//! The agent tool set: OpenAI function schemas advertised to the model, plus
//! translation between the model's tool calls and Warp's `ToolCall` protobuf
//! (so the Warp client executes them), and extraction of result text from the
//! `ToolCallResult` protos the client sends back.
//!
//! Each built-in tool's knowledge lives in one adapter (see [`builtins`]); this
//! module is the public surface the registry drives. MCP tools are the second
//! adapter at the same seam — dynamic tools advertised from the request's MCP
//! context (see [`crate::mcp`]). Adding a built-in tool means writing one
//! adapter, not editing the functions below.

mod builtins;

use serde_json::{json, Value};
use warp_multi_agent_api as api;

use crate::mcp::{self, McpRegistry};
use builtins::{builtin_tools, ToolResult};

/// OpenAI `tools` array advertised to the model: the built-in tools plus any
/// MCP tools from the request's MCP context.
pub fn tool_schemas(mcp: &McpRegistry) -> Vec<Value> {
    let mut tools: Vec<Value> = builtin_tools().iter().map(|t| t.schema()).collect();
    for tool in &mcp.tools {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": tool.openai_name,
                "description": tool.description,
                "parameters": tool.input_schema,
            }
        }));
    }
    tools
}

/// Converts one OpenAI tool call (function name + JSON arguments string) into a
/// Warp `ToolCall` message the client can execute. Resolves built-ins via the
/// registry, falling back to the MCP registry. Returns `None` for unknown tools.
pub fn openai_tool_call_to_warp(
    tool_call_id: &str,
    name: &str,
    arguments: &str,
    mcp: &McpRegistry,
) -> Option<api::Message> {
    use api::message::tool_call::Tool;
    let args: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));

    let tool = if let Some(builtin) = builtin_tools().iter().find(|t| t.name() == name) {
        builtin.to_warp(&args)
    } else {
        // MCP tool? Map to a CallMcpTool the client executes against the real
        // MCP server.
        let def = mcp.lookup(name)?;
        Tool::CallMcpTool(api::message::tool_call::CallMcpTool {
            name: def.real_name.clone(),
            args: Some(mcp::json_to_struct(&args)),
            server_id: def.server_id.clone(),
        })
    };

    Some(api::Message {
        id: uuid::Uuid::new_v4().to_string(),
        message: Some(api::message::Message::ToolCall(api::message::ToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool: Some(tool),
        })),
        ..Default::default()
    })
}

/// Reverse mapping: a historical Warp `ToolCall` back into an OpenAI tool_call
/// object so prior turns replay correctly to the model.
pub fn warp_tool_call_to_openai(tc: &api::message::ToolCall) -> Option<Value> {
    use api::message::tool_call::Tool;
    let tool = tc.tool.as_ref()?;
    let (name, args): (String, Value) = if let Some((builtin, args)) = builtin_tools()
        .iter()
        .find_map(|t| t.to_openai(tool).map(|args| (t, args)))
    {
        (builtin.name().to_string(), args)
    } else if let Tool::CallMcpTool(c) = tool {
        (
            mcp::sanitize_tool_name(&c.name),
            c.args
                .as_ref()
                .map(mcp::struct_to_json)
                .unwrap_or_else(|| json!({})),
        )
    } else {
        return None;
    };
    Some(json!({
        "id": tc.tool_call_id,
        "type": "function",
        "function": {"name": name, "arguments": args.to_string()},
    }))
}

/// Extracts human/model-readable text from a `ToolCallResult` carried in task
/// history.
pub fn history_tool_result_to_text(tr: &api::message::ToolCallResult) -> String {
    use api::message::tool_call_result::Result as R;
    let normalized = match tr.result.as_ref() {
        Some(R::RunShellCommand(r)) => ToolResult::Shell(r),
        Some(R::ReadFiles(r)) => ToolResult::Read(r),
        Some(R::ApplyFileDiffs(r)) => ToolResult::Apply(r),
        Some(R::CallMcpTool(r)) => ToolResult::Mcp(r),
        Some(R::Cancel(_)) => ToolResult::Cancelled,
        _ => ToolResult::None,
    };
    render_result(&normalized)
}

/// Extracts result text from a `ToolCallResult` carried in the request `input`
/// (the freshly-executed tool result).
pub fn input_tool_result_to_text(tr: &api::request::input::ToolCallResult) -> String {
    use api::request::input::tool_call_result::Result as R;
    let normalized = match tr.result.as_ref() {
        Some(R::RunShellCommand(r)) => ToolResult::Shell(r),
        Some(R::ReadFiles(r)) => ToolResult::Read(r),
        Some(R::ApplyFileDiffs(r)) => ToolResult::Apply(r),
        Some(R::CallMcpTool(r)) => ToolResult::Mcp(r),
        _ => ToolResult::None,
    };
    render_result(&normalized)
}

/// Renders a normalized tool result to model-readable text. Built-in tools each
/// render their own variant; MCP and cancellation are handled here. This is the
/// single home for result rendering across both `ToolCallResult` proto types.
fn render_result(result: &ToolResult) -> String {
    match result {
        ToolResult::Mcp(r) => call_mcp_text(r),
        ToolResult::Cancelled => "(cancelled by user)".to_string(),
        ToolResult::None => String::new(),
        builtin => builtin_tools()
            .iter()
            .find_map(|t| t.result_text(builtin))
            .unwrap_or_default(),
    }
}

fn call_mcp_text(r: &api::CallMcpToolResult) -> String {
    use api::call_mcp_tool_result::success::result::Result as Item;
    use api::call_mcp_tool_result::Result as R;
    match r.result.as_ref() {
        Some(R::Success(s)) => s
            .results
            .iter()
            .filter_map(|res| match res.result.as_ref() {
                Some(Item::Text(t)) => Some(t.text.clone()),
                Some(Item::Image(_)) => Some("[image]".to_string()),
                Some(Item::Resource(_)) => Some("[resource]".to_string()),
                None => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(R::Error(e)) => format!("MCP tool error: {}", e.message),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
