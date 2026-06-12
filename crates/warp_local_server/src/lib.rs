//! Core of warp-max-server: turns a decoded Warp `Request` into the
//! `ResponseEvent`s that drive one agent turn against the user's own provider.
//! Exposed as a library so the protocol↔provider pipeline can be tested
//! end-to-end against a mock provider.

pub mod history;
pub mod provider;
pub mod sse;
pub mod tools;

use warp_multi_agent_api as api;

pub const SYSTEM_PROMPT: &str = "You are Warp Max, an expert agentic coding assistant running inside the user's terminal. \
You help with software engineering tasks: reading and editing code, running commands, debugging, and answering questions. \
You have these tools: run_shell_command (run a terminal command), read_files (read file contents), and apply_file_diffs (create/edit/delete files via exact search/replace). \
Prefer reading relevant files before editing. Make minimal, correct edits. When you edit a file, the `search` text must match the file's current contents verbatim. \
Run commands to verify your work when useful. When the task is complete, give a short, clear summary instead of calling more tools.";

/// Runs one agent turn for a decoded request and returns the ordered
/// `ResponseEvent`s to stream back (always `Init` … `Finished`).
pub async fn run_turn(client: &reqwest::Client, request: api::Request) -> Vec<api::ResponseEvent> {
    let conversation_id = {
        let existing = history::conversation_id(&request);
        if existing.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            existing
        }
    };
    let run_id = uuid::Uuid::new_v4().to_string();
    let init = sse::init(conversation_id, run_id);

    let Some(provider) = history::resolve_provider(&request) else {
        return vec![
            init,
            sse::finished_error(
                "No model provider configured. Add a custom endpoint (e.g. a MiniMax Token Plan) \
                 in Settings → AI → Custom inference, or set \
                 WARP_MAX_BASE_URL/WARP_MAX_API_KEY/WARP_MAX_MODEL."
                    .to_string(),
            ),
        ];
    };
    if provider.base_url.trim().is_empty() {
        return vec![
            init,
            sse::finished_error("Configured endpoint has an empty URL.".to_string()),
        ];
    }

    let mut messages = vec![serde_json::json!({"role": "system", "content": SYSTEM_PROMPT})];
    messages.extend(history::reconstruct_messages(&request));
    let task_id = history::target_task_id(&request);

    if std::env::var("WARP_MAX_DEBUG").is_ok() {
        eprintln!(
            "warp-max-server: turn task_id={} base_url={} model={} history_msgs={}",
            task_id,
            provider.base_url,
            provider.model,
            messages.len()
        );
    }

    let turn = match provider::call(client, &provider, messages, tools::tool_schemas()).await {
        Ok(turn) => turn,
        Err(e) => {
            eprintln!("warp-max-server: provider call failed: {e:#}");
            return vec![init, sse::finished_error(format!("{e:#}"))];
        }
    };

    let mut out_messages: Vec<api::Message> = Vec::new();
    if !turn.text.trim().is_empty() {
        out_messages.push(agent_output(turn.text));
    }
    for (id, name, arguments) in &turn.tool_calls {
        if let Some(message) = tools::openai_tool_call_to_warp(id, name, arguments) {
            out_messages.push(message);
        }
    }
    if out_messages.is_empty() {
        out_messages.push(agent_output("(no response)".to_string()));
    }

    vec![
        init,
        sse::client_actions(vec![sse::add_messages(task_id, out_messages)]),
        sse::finished_done(),
    ]
}

fn agent_output(text: String) -> api::Message {
    api::Message {
        id: uuid::Uuid::new_v4().to_string(),
        message: Some(api::message::Message::AgentOutput(
            api::message::AgentOutput { text },
        )),
        ..Default::default()
    }
}
