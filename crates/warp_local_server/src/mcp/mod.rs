//! MCP (Model Context Protocol) support for the local agent backend.
//!
//! The Warp client ships its configured MCP servers/tools in
//! `request.mcp_context`. We advertise those tools to the model (alongside the
//! built-in tools), and map the model's calls to Warp `CallMcpTool` actions the
//! client executes against the real MCP server.

use serde_json::{json, Value};
use warp_multi_agent_api as api;

/// One MCP tool advertised to the model.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    /// Name advertised to the model (sanitized to the OpenAI function-name charset).
    pub openai_name: String,
    /// The real MCP tool name the client must invoke.
    pub real_name: String,
    /// The MCP server id that provides this tool (may be empty for legacy ctx).
    pub server_id: String,
    pub description: String,
    /// JSON Schema for the tool's arguments.
    pub input_schema: Value,
}

/// All MCP tools available for a request, keyed by their advertised name.
#[derive(Debug, Default)]
pub struct McpRegistry {
    pub tools: Vec<McpToolDef>,
}

impl McpRegistry {
    /// Builds the registry from the request's MCP context (server-grouped tools
    /// plus the deprecated flat tool list). Duplicate advertised names keep the
    /// first occurrence.
    pub fn from_request(request: &api::Request) -> Self {
        let mut tools: Vec<McpToolDef> = Vec::new();
        let Some(ctx) = request.mcp_context.as_ref() else {
            return Self { tools };
        };

        let mut push = |name: &str, description: &str, schema: Option<Value>, server_id: &str| {
            let openai_name = sanitize_tool_name(name);
            if openai_name.is_empty() || tools.iter().any(|t| t.openai_name == openai_name) {
                return;
            }
            tools.push(McpToolDef {
                openai_name,
                real_name: name.to_string(),
                server_id: server_id.to_string(),
                description: description.to_string(),
                input_schema: schema.unwrap_or_else(|| json!({"type": "object"})),
            });
        };

        for server in &ctx.servers {
            for tool in &server.tools {
                push(
                    &tool.name,
                    &tool.description,
                    tool.input_schema.as_ref().map(struct_to_json),
                    &server.id,
                );
            }
        }
        #[allow(deprecated)]
        for tool in &ctx.tools {
            push(
                &tool.name,
                &tool.description,
                tool.input_schema.as_ref().map(struct_to_json),
                "",
            );
        }

        Self { tools }
    }

    pub fn lookup(&self, openai_name: &str) -> Option<&McpToolDef> {
        self.tools.iter().find(|t| t.openai_name == openai_name)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// Sanitizes a name to the OpenAI/MiniMax function-name charset
/// (`[a-zA-Z0-9_-]`, max 64 chars). Deterministic, so the same MCP tool name
/// always maps to the same advertised name (used for both advertising and
/// history replay).
pub fn sanitize_tool_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.len() > 64 {
        s.truncate(64);
    }
    s
}

/// Converts a protobuf `Struct` into a JSON object value.
pub fn struct_to_json(s: &prost_types::Struct) -> Value {
    Value::Object(
        s.fields
            .iter()
            .map(|(k, v)| (k.clone(), prost_value_to_json(v)))
            .collect(),
    )
}

fn prost_value_to_json(v: &prost_types::Value) -> Value {
    use prost_types::value::Kind;
    match &v.kind {
        None | Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::BoolValue(b)) => Value::Bool(*b),
        Some(Kind::NumberValue(n)) => json!(n),
        Some(Kind::StringValue(s)) => Value::String(s.clone()),
        Some(Kind::ListValue(l)) => {
            Value::Array(l.values.iter().map(prost_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => struct_to_json(s),
    }
}

/// Converts a JSON value into a protobuf `Struct` (objects only; non-object
/// values are wrapped under a `"value"` key so the result is always a Struct).
pub fn json_to_struct(value: &Value) -> prost_types::Struct {
    match value {
        Value::Object(map) => prost_types::Struct {
            fields: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_prost_value(v)))
                .collect(),
        },
        other => prost_types::Struct {
            fields: [("value".to_string(), json_to_prost_value(other))]
                .into_iter()
                .collect(),
        },
    }
}

fn json_to_prost_value(value: &Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match value {
        Value::Null => Kind::NullValue(0),
        Value::Bool(b) => Kind::BoolValue(*b),
        Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => Kind::StringValue(s.clone()),
        Value::Array(a) => Kind::ListValue(prost_types::ListValue {
            values: a.iter().map(json_to_prost_value).collect(),
        }),
        Value::Object(_) => Kind::StructValue(json_to_struct(value)),
    };
    prost_types::Value { kind: Some(kind) }
}

#[cfg(test)]
mod tests;
