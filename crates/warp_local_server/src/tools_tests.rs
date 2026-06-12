use super::*;

#[test]
fn schemas_advertise_core_tools() {
    let names: Vec<String> = tool_schemas()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"run_shell_command".to_string()));
    assert!(names.contains(&"read_files".to_string()));
    assert!(names.contains(&"apply_file_diffs".to_string()));
}

#[test]
fn run_shell_command_maps_to_warp_tool_call() {
    let msg = openai_tool_call_to_warp(
        "call_1",
        "run_shell_command",
        r#"{"command":"ls -la","is_read_only":true}"#,
    )
    .expect("should map");
    let Some(api::message::Message::ToolCall(tc)) = msg.message else {
        panic!("expected tool call");
    };
    assert_eq!(tc.tool_call_id, "call_1");
    let Some(api::message::tool_call::Tool::RunShellCommand(c)) = tc.tool else {
        panic!("expected run_shell_command");
    };
    assert_eq!(c.command, "ls -la");
    assert!(c.is_read_only);
}

#[test]
fn apply_file_diffs_maps_all_change_kinds() {
    let args = r#"{
        "summary": "tweak",
        "diffs": [{"file_path": "a.rs", "search": "old", "replace": "new"}],
        "new_files": [{"file_path": "b.rs", "content": "fn main(){}"}],
        "deleted_files": ["c.rs"]
    }"#;
    let msg = openai_tool_call_to_warp("call_2", "apply_file_diffs", args).unwrap();
    let Some(api::message::Message::ToolCall(tc)) = msg.message else {
        panic!();
    };
    let Some(api::message::tool_call::Tool::ApplyFileDiffs(a)) = tc.tool else {
        panic!();
    };
    assert_eq!(a.summary, "tweak");
    assert_eq!(a.diffs.len(), 1);
    assert_eq!(a.diffs[0].file_path, "a.rs");
    assert_eq!(a.new_files.len(), 1);
    assert_eq!(a.deleted_files[0].file_path, "c.rs");
}

#[test]
fn unknown_tool_returns_none() {
    assert!(openai_tool_call_to_warp("x", "nonexistent_tool", "{}").is_none());
}

#[test]
fn warp_tool_call_round_trips_to_openai() {
    let msg =
        openai_tool_call_to_warp("call_3", "read_files", r#"{"paths":["x.rs","y.rs"]}"#).unwrap();
    let Some(api::message::Message::ToolCall(tc)) = msg.message else {
        panic!();
    };
    let openai = warp_tool_call_to_openai(&tc).unwrap();
    assert_eq!(openai["function"]["name"], "read_files");
    let args: serde_json::Value =
        serde_json::from_str(openai["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(args["paths"][0], "x.rs");
    assert_eq!(args["paths"][1], "y.rs");
}

#[test]
fn run_shell_result_text_includes_exit_code_and_output() {
    let result = api::message::ToolCallResult {
        tool_call_id: "c".to_string(),
        result: Some(api::message::tool_call_result::Result::RunShellCommand(
            api::RunShellCommandResult {
                result: Some(api::run_shell_command_result::Result::CommandFinished(
                    api::ShellCommandFinished {
                        output: "hello".to_string(),
                        exit_code: 0,
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        )),
        ..Default::default()
    };
    let text = history_tool_result_to_text(&result);
    assert!(text.contains("exit code: 0"));
    assert!(text.contains("hello"));
}
