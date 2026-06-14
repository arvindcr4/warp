//! Per-tool adapters for the built-in coding tools. Each tool's knowledge —
//! its OpenAI schema, the forward map to a Warp `ToolCall`, the reverse map for
//! history replay, and how its result renders to text — lives in one place,
//! behind the [`BuiltinTool`] trait. The registry ([`builtin_tools`]) drives
//! both advertising and dispatch, so adding a tool means writing one adapter,
//! not editing five functions.

use serde_json::{json, Value};
use warp_multi_agent_api as api;

use api::message::tool_call::Tool;

/// A tool result normalized away from the two parallel protobuf result types
/// (`message::tool_call_result::Result` and
/// `request::input::tool_call_result::Result`), so result rendering has one home
/// regardless of which type carried it.
pub enum ToolResult<'a> {
    Shell(&'a api::RunShellCommandResult),
    Read(&'a api::ReadFilesResult),
    Apply(&'a api::ApplyFileDiffsResult),
    Mcp(&'a api::CallMcpToolResult),
    Cancelled,
    None,
}

/// One built-in coding tool. Everything a caller must know about it — schema,
/// both direction maps, and result rendering — is reachable through this
/// interface, so a tool has a single home.
pub trait BuiltinTool: Sync {
    /// Name advertised to the model and used to dispatch its calls.
    fn name(&self) -> &'static str;
    /// OpenAI `function` schema advertised to the model.
    fn schema(&self) -> Value;
    /// Builds the Warp `ToolCall` payload from the model's parsed arguments.
    fn to_warp(&self, args: &Value) -> Tool;
    /// If `tool` is this tool's variant, returns its OpenAI arguments object for
    /// history replay; otherwise `None`.
    fn to_openai(&self, tool: &Tool) -> Option<Value>;
    /// If `result` is this tool's variant, renders it to model-readable text;
    /// otherwise `None`.
    fn result_text(&self, result: &ToolResult) -> Option<String>;
}

/// The built-in coding tools, in advertised order — the single source of truth
/// for which tools exist.
pub fn builtin_tools() -> &'static [&'static dyn BuiltinTool] {
    &[&RunShellCommand, &ReadFiles, &ApplyFileDiffs]
}

struct RunShellCommand;
impl BuiltinTool for RunShellCommand {
    fn name(&self) -> &'static str {
        "run_shell_command"
    }
    fn schema(&self) -> Value {
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
        })
    }
    fn to_warp(&self, args: &Value) -> Tool {
        Tool::RunShellCommand(api::message::tool_call::RunShellCommand {
            command: args["command"].as_str().unwrap_or_default().to_string(),
            is_read_only: args["is_read_only"].as_bool().unwrap_or(false),
            ..Default::default()
        })
    }
    fn to_openai(&self, tool: &Tool) -> Option<Value> {
        let Tool::RunShellCommand(c) = tool else {
            return None;
        };
        Some(json!({"command": c.command, "is_read_only": c.is_read_only}))
    }
    fn result_text(&self, result: &ToolResult) -> Option<String> {
        let ToolResult::Shell(r) = result else {
            return None;
        };
        use api::run_shell_command_result::Result as R;
        Some(match r.result.as_ref() {
            Some(R::CommandFinished(f)) => format!("exit code: {}\n{}", f.exit_code, f.output),
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
        })
    }
}

struct ReadFiles;
impl BuiltinTool for ReadFiles {
    fn name(&self) -> &'static str {
        "read_files"
    }
    fn schema(&self) -> Value {
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
        })
    }
    fn to_warp(&self, args: &Value) -> Tool {
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
    fn to_openai(&self, tool: &Tool) -> Option<Value> {
        let Tool::ReadFiles(r) = tool else {
            return None;
        };
        Some(json!({"paths": r.files.iter().map(|f| f.name.clone()).collect::<Vec<_>>()}))
    }
    fn result_text(&self, result: &ToolResult) -> Option<String> {
        let ToolResult::Read(r) = result else {
            return None;
        };
        use api::read_files_result::Result as R;
        Some(match r.result.as_ref() {
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
        })
    }
}

struct ApplyFileDiffs;
impl BuiltinTool for ApplyFileDiffs {
    fn name(&self) -> &'static str {
        "apply_file_diffs"
    }
    fn schema(&self) -> Value {
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
        })
    }
    fn to_warp(&self, args: &Value) -> Tool {
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
    fn to_openai(&self, tool: &Tool) -> Option<Value> {
        let Tool::ApplyFileDiffs(a) = tool else {
            return None;
        };
        Some(json!({
            "summary": a.summary,
            "diffs": a.diffs.iter().map(|d| json!({"file_path": d.file_path, "search": d.search, "replace": d.replace})).collect::<Vec<_>>(),
            "new_files": a.new_files.iter().map(|n| json!({"file_path": n.file_path, "content": n.content})).collect::<Vec<_>>(),
            "deleted_files": a.deleted_files.iter().map(|d| d.file_path.clone()).collect::<Vec<_>>(),
        }))
    }
    fn result_text(&self, result: &ToolResult) -> Option<String> {
        let ToolResult::Apply(r) = result else {
            return None;
        };
        use api::apply_file_diffs_result::Result as R;
        Some(match r.result.as_ref() {
            Some(R::Success(s)) => {
                let updated = s.updated_files_v2.len();
                let deleted = s.deleted_files.len();
                format!("applied: {updated} file(s) updated, {deleted} file(s) deleted")
            }
            Some(R::Error(e)) => format!("error applying diffs: {}", e.message),
            None => "applied".to_string(),
        })
    }
}
