//! Core of warp-max-server: turns a decoded Warp `Request` into the
//! `ResponseEvent`s that drive one agent turn against the user's own provider.
//! Exposed as a library so the protocol↔provider pipeline can be tested
//! end-to-end against a mock provider.

pub mod history;
pub mod mcp;
pub mod provider;
pub mod tools;
pub mod wire;

mod trace;

use warp_multi_agent_api as api;

pub const SYSTEM_PROMPT: &str = "You are Warp Max, an expert agentic coding assistant running inside the user's terminal. \
You work autonomously in a multi-step loop: you call a tool, receive its result, then decide the next action — repeating until the task is fully done. \
You help with software engineering tasks: reading and editing code, running commands, debugging, and answering questions. \
Your tools are: run_shell_command (run a terminal command), read_files (read file contents), and apply_file_diffs (create/edit/delete files via exact search/replace). \
Operate agentically. Do not stop, hand back, or ask the user to confirm while work remains — keep going until the task is complete and verified. \
Take real actions instead of only describing them: whenever there is more to do, make a tool call rather than replying with plain text. A reply that contains no tool call ENDS your turn and returns control to the user, so never end your turn with a plan you have not yet executed. \
Work in small steps: read the relevant files before editing, make minimal correct edits (the `search` text in apply_file_diffs must match the file's current contents verbatim), and run commands to verify your work as you go. \
Only when the task is fully complete, reply with a short, clear summary and no tool calls — that is your final message and ends the session.";

/// Runs one agent turn for a decoded request and returns the ordered
/// `ResponseEvent`s to stream back (always `Init` … `Finished`). Stream framing
/// is owned by [`wire::turn`]; this only mints the conversation/run ids and
/// delegates the turn body to [`agent_turn`].
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
    wire::turn(
        conversation_id,
        run_id,
        agent_turn(client, request, auth_header_api_key).await,
    )
}

/// Runs the turn body: resolve the provider, reconstruct the transcript, call
/// the provider once, and map the result into `ClientAction`s. Returns the
/// actions to emit or an error message to surface — never `Init`/`Finished`,
/// which is [`wire::turn`]'s job.
async fn agent_turn(
    client: &reqwest::Client,
    request: api::Request,
    auth_header_api_key: Option<String>,
) -> Result<Vec<api::ClientAction>, String> {
    let provider = provider::resolve(&request, auth_header_api_key).map_err(|e| e.message())?;
    let custom_provider_count = request
        .settings
        .as_ref()
        .and_then(|s| s.custom_model_providers.as_ref())
        .map(|c| c.providers.len())
        .unwrap_or(0);
    trace::resolved_provider(&provider, custom_provider_count);

    let mut messages = vec![serde_json::json!({"role": "system", "content": SYSTEM_PROMPT})];
    messages.extend(history::reconstruct_messages(&request));
    trace::message_shape(&messages);

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
    trace::turn_dump(&request, &task_id, &provider, messages.len());

    let mcp = mcp::McpRegistry::from_request(&request);
    trace::mcp_advertised(&mcp);

    let turn = provider::call(client, &provider, messages, tools::tool_schemas(&mcp))
        .await
        .map_err(|e| {
            trace::provider_call_failed(&e);
            format!("{e:#}")
        })?;

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
        actions.push(wire::create_task(task_id.clone()));
    }
    actions.push(wire::add_messages(task_id, out_messages));
    Ok(actions)
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
