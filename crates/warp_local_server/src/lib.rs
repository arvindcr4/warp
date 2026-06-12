//! Core of warp-max-server: turns a decoded Warp `Request` into the
//! `ResponseEvent`s that drive one agent turn against the user's own provider.
//! Exposed as a library so the protocol↔provider pipeline can be tested
//! end-to-end against a mock provider.

pub mod history;
pub mod mcp;
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
pub async fn run_turn(
    client: &reqwest::Client,
    request: api::Request,
    auth_header_api_key: Option<String>,
) -> Vec<api::ResponseEvent> {
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

    let Some(provider) = history::resolve_provider(&request, auth_header_api_key) else {
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
    let custom_provider_count = request
        .settings
        .as_ref()
        .and_then(|s| s.custom_model_providers.as_ref())
        .map(|c| c.providers.len())
        .unwrap_or(0);
    eprintln!(
        "warp-max-server: resolved base_url={} model={} api_key_len={} custom_providers={}",
        provider.base_url,
        provider.model,
        provider.api_key.len(),
        custom_provider_count
    );

    if provider.base_url.trim().is_empty() {
        return vec![
            init,
            sse::finished_error("Configured endpoint has an empty URL.".to_string()),
        ];
    }
    if provider.api_key.trim().is_empty() {
        return vec![
            init,
            sse::finished_error(format!(
                "No API key for the selected model. Make sure the model you picked is your \
                 custom endpoint (e.g. MiniMax Token Plan) configured in Settings → AI → Custom \
                 inference with a non-empty key — or run `warp login --api-key sk-...`. \
                 (Resolved endpoint: {}, model: {}.)",
                provider.base_url, provider.model
            )),
        ];
    }

    let mut messages = vec![serde_json::json!({"role": "system", "content": SYSTEM_PROMPT})];
    messages.extend(history::reconstruct_messages(&request));

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

    // For a new conversation the client sends no tasks and expects the server
    // to create the root task (it then re-keys its pending exchange to our id).
    // For continuations it sends the existing task, which we must reuse.
    let is_new_conversation = request
        .task_context
        .as_ref()
        .map(|tc| tc.tasks.is_empty())
        .unwrap_or(true);
    let task_id = if is_new_conversation {
        uuid::Uuid::new_v4().to_string()
    } else {
        history::target_task_id(&request)
    };

    if std::env::var("WARP_MAX_DEBUG").is_ok() {
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
            messages.len(),
            tasks_dump.join(", "),
            input_kind
        );
    }

    let mcp = mcp::McpRegistry::from_request(&request);
    if !mcp.is_empty() {
        eprintln!(
            "warp-max-server: advertising {} MCP tool(s)",
            mcp.tools.len()
        );
    }

    let turn = match provider::call(client, &provider, messages, tools::tool_schemas(&mcp)).await {
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
        if let Some(message) = tools::openai_tool_call_to_warp(id, name, arguments, &mcp) {
            out_messages.push(message);
        }
    }
    if out_messages.is_empty() {
        out_messages.push(agent_output("(no response)".to_string()));
    }

    let mut actions = Vec::new();
    if is_new_conversation {
        actions.push(sse::create_task(task_id.clone()));
    }
    actions.push(sse::add_messages(task_id, out_messages));

    vec![init, sse::client_actions(actions), sse::finished_done()]
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
