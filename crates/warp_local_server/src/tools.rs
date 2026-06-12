//! The agent tool set: OpenAI function schemas advertised to the model, plus
//! translation between the model's tool calls and Warp's `ToolCall` protobuf
//! (so the Warp client executes them), and extraction of result text from the
//! `ToolCallResult` protos the client sends back.
//!
//! v1 supports the core coding-agent trio: run_shell_command, read_files, and
//! apply_file_diffs. Adding a tool means: (1) add a schema in `tool_schemas`,
//! (2) map it in `openai_tool_call_to_warp`, (3) map its result text in the
//! `*_tool_result_to_text` helpers, and (4) reverse-map in
//! `warp_tool_call_to_openai` for history replay.

use serde_json::{json, Value};
use warp_multi_agent_api as api;

/// OpenAI `tools` array advertised to the model.
pub fn tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "run_shell_command",
                "description": "Run a shell command in the user's terminal and return its output. Use for inspecting the project, running builds/tests, git, etc.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "The exact shell command to run."},
                        "is_read_only": {"type": "boolean", "description": "True if the command only reads state and has no side effects."}
                    },
                    "required": ["command"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_files",
                "description": "Read the contents of one or more files by path.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "paths": {"type": "array", "items": {"type": "string"}, "description": "Absolute or workspace-relative file paths to read."}
                    },
                    "required": ["paths"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "apply_file_diffs",
                "description": "Edit, create, or delete files. Use search/replace edits for existing files; provide exact search text that occurs verbatim in the file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "summary": {"type": "string", "description": "Short summary of the change."},
                        "diffs": {
                            "type": "array",
                            "description": "Search/replace edits to existing files.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "file_path": {"type": "string"},
                                    "search": {"type": "string", "description": "Exact text to replace (must appear verbatim)."},
                                    "replace": {"type": "string", "description": "Replacement text."}
                                },
                                "required": ["file_path", "search", "replace"]
                            }
                        },
                        "new_files": {
                            "type": "array",
                            "description": "Files to create.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "file_path": {"type": "string"},
                                    "content": {"type": "string"}
                                },
                                "required": ["file_path", "content"]
                            }
                        },
                        "deleted_files": {
                            "type": "array",
                            "description": "Paths of files to delete.",
                            "items": {"type": "string"}
                        }
                    },
                    "required": ["summary"]
                }
            }
        }),
    ]
}

/// Converts one OpenAI tool call (function name + JSON arguments string) into a
/// Warp `ToolCall` message the client can execute. Returns `None` for unknown
/// tools.
pub fn openai_tool_call_to_warp(
    tool_call_id: &str,
    name: &str,
    arguments: &str,
) -> Option<api::Message> {
    use api::message::tool_call::Tool;
    let args: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));

    let tool = match name {
        "run_shell_command" => Tool::RunShellCommand(api::message::tool_call::RunShellCommand {
            command: args["command"].as_str().unwrap_or_default().to_string(),
            is_read_only: args["is_read_only"].as_bool().unwrap_or(false),
            ..Default::default()
        }),
        "read_files" => {
            let files = args["paths"]
                .as_array()
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(|p| p.as_str())
                        .map(|name| api::message::tool_call::read_files::File {
                            name: name.to_string(),
                            line_ranges: vec![],
                        })
                        .collect()
                })
                .unwrap_or_default();
            Tool::ReadFiles(api::message::tool_call::ReadFiles { files })
        }
        "apply_file_diffs" => {
            let diffs = args["diffs"]
                .as_array()
                .map(|ds| {
                    ds.iter()
                        .map(|d| api::message::tool_call::apply_file_diffs::FileDiff {
                            file_path: d["file_path"].as_str().unwrap_or_default().to_string(),
                            search: d["search"].as_str().unwrap_or_default().to_string(),
                            replace: d["replace"].as_str().unwrap_or_default().to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let new_files = args["new_files"]
                .as_array()
                .map(|ns| {
                    ns.iter()
                        .map(|n| api::message::tool_call::apply_file_diffs::NewFile {
                            file_path: n["file_path"].as_str().unwrap_or_default().to_string(),
                            content: n["content"].as_str().unwrap_or_default().to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let deleted_files = args["deleted_files"]
                .as_array()
                .map(|ds| {
                    ds.iter()
                        .filter_map(|d| d.as_str())
                        .map(
                            |file_path| api::message::tool_call::apply_file_diffs::DeleteFile {
                                file_path: file_path.to_string(),
                            },
                        )
                        .collect()
                })
                .unwrap_or_default();
            Tool::ApplyFileDiffs(api::message::tool_call::ApplyFileDiffs {
                summary: args["summary"].as_str().unwrap_or_default().to_string(),
                diffs,
                new_files,
                deleted_files,
                ..Default::default()
            })
        }
        _ => return None,
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
    let (name, args) = match tc.tool.as_ref()? {
        Tool::RunShellCommand(c) => (
            "run_shell_command",
            json!({"command": c.command, "is_read_only": c.is_read_only}),
        ),
        Tool::ReadFiles(r) => (
            "read_files",
            json!({"paths": r.files.iter().map(|f| f.name.clone()).collect::<Vec<_>>()}),
        ),
        Tool::ApplyFileDiffs(a) => (
            "apply_file_diffs",
            json!({
                "summary": a.summary,
                "diffs": a.diffs.iter().map(|d| json!({"file_path": d.file_path, "search": d.search, "replace": d.replace})).collect::<Vec<_>>(),
                "new_files": a.new_files.iter().map(|n| json!({"file_path": n.file_path, "content": n.content})).collect::<Vec<_>>(),
                "deleted_files": a.deleted_files.iter().map(|d| d.file_path.clone()).collect::<Vec<_>>(),
            }),
        ),
        _ => return None,
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
    match tr.result.as_ref() {
        Some(R::RunShellCommand(r)) => run_shell_text(r),
        Some(R::ReadFiles(r)) => read_files_text(r),
        Some(R::ApplyFileDiffs(r)) => apply_diffs_text(r),
        Some(R::Cancel(_)) => "(cancelled by user)".to_string(),
        _ => String::new(),
    }
}

/// Extracts result text from a `ToolCallResult` carried in the request `input`
/// (the freshly-executed tool result).
pub fn input_tool_result_to_text(tr: &api::request::input::ToolCallResult) -> String {
    use api::request::input::tool_call_result::Result as R;
    match tr.result.as_ref() {
        Some(R::RunShellCommand(r)) => run_shell_text(r),
        Some(R::ReadFiles(r)) => read_files_text(r),
        Some(R::ApplyFileDiffs(r)) => apply_diffs_text(r),
        _ => String::new(),
    }
}

fn run_shell_text(r: &api::RunShellCommandResult) -> String {
    use api::run_shell_command_result::Result as R;
    match r.result.as_ref() {
        Some(R::CommandFinished(f)) => {
            format!("exit code: {}\n{}", f.exit_code, f.output)
        }
        Some(R::PermissionDenied(_)) => "(command denied by user)".to_string(),
        Some(R::LongRunningCommandSnapshot(_)) => {
            "(command is still running in the background)".to_string()
        }
        None => {
            // Fall back to the deprecated flat fields for older payloads.
            #[allow(deprecated)]
            {
                format!("exit code: {}\n{}", r.exit_code, r.output)
            }
        }
    }
}

fn read_files_text(r: &api::ReadFilesResult) -> String {
    use api::read_files_result::Result as R;
    match r.result.as_ref() {
        Some(R::TextFilesSuccess(s)) => s
            .files
            .iter()
            .map(|f| format!("===== {} =====\n{}", f.file_path, f.content))
            .collect::<Vec<_>>()
            .join("\n\n"),
        Some(R::AnyFilesSuccess(s)) => s
            .files
            .iter()
            .filter_map(|f| match f.content.as_ref() {
                Some(api::any_file_content::Content::TextContent(t)) => {
                    Some(format!("===== {} =====\n{}", t.file_path, t.content))
                }
                Some(api::any_file_content::Content::BinaryContent(b)) => Some(format!(
                    "===== {} (binary, {} bytes) =====",
                    b.file_path,
                    b.data.len()
                )),
                None => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        Some(R::Error(e)) => format!("error reading files: {}", e.message),
        None => String::new(),
    }
}

fn apply_diffs_text(r: &api::ApplyFileDiffsResult) -> String {
    use api::apply_file_diffs_result::Result as R;
    match r.result.as_ref() {
        Some(R::Success(s)) => {
            let updated = s.updated_files_v2.len();
            let deleted = s.deleted_files.len();
            format!("applied: {updated} file(s) updated, {deleted} file(s) deleted")
        }
        Some(R::Error(e)) => format!("error applying diffs: {}", e.message),
        None => "applied".to_string(),
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
