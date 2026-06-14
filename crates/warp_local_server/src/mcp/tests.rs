use super::*;
use warp_multi_agent_api as api;

#[test]
fn sanitize_keeps_valid_and_replaces_invalid() {
    assert_eq!(sanitize_tool_name("get_issue"), "get_issue");
    assert_eq!(
        sanitize_tool_name("github.create-issue"),
        "github_create-issue"
    );
    assert_eq!(sanitize_tool_name("a b/c:d"), "a_b_c_d");
    assert_eq!(sanitize_tool_name(&"x".repeat(80)).len(), 64);
}

#[test]
fn json_struct_round_trip() {
    let v = json!({
        "query": "hello",
        "limit": 5,
        "nested": {"flag": true, "list": [1, "two", null]},
    });
    let s = json_to_struct(&v);
    let back = struct_to_json(&s);
    assert_eq!(back["query"], "hello");
    assert_eq!(back["limit"], 5.0);
    assert_eq!(back["nested"]["flag"], true);
    assert_eq!(back["nested"]["list"][1], "two");
}

#[test]
fn registry_built_from_mcp_context() {
    let schema = json_to_struct(&json!({
        "type": "object",
        "properties": {"q": {"type": "string"}},
    }));
    let request = api::Request {
        mcp_context: Some(api::request::McpContext {
            servers: vec![api::request::mcp_context::McpServer {
                id: "srv-1".to_string(),
                name: "Linear".to_string(),
                description: "Linear MCP".to_string(),
                tools: vec![api::request::mcp_context::McpTool {
                    name: "search_issues".to_string(),
                    description: "Search issues".to_string(),
                    input_schema: Some(schema),
                }],
                resources: vec![],
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    let registry = McpRegistry::from_request(&request);
    assert_eq!(registry.tools.len(), 1);
    let def = registry.lookup("search_issues").expect("tool present");
    assert_eq!(def.real_name, "search_issues");
    assert_eq!(def.server_id, "srv-1");
    assert_eq!(def.input_schema["properties"]["q"]["type"], "string");
}
