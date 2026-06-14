//! Translation between the OpenAI Chat Completions wire format and the
//! Anthropic Messages wire format.
//!
//! The bridge accepts OpenAI-format requests (which is what Warp's backend
//! speaks to custom endpoints) and forwards Anthropic-format requests to the
//! target endpoint, translating responses (including SSE streams) back.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

const DEFAULT_MAX_TOKENS: u64 = 8192;

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Extracts text from an OpenAI message `content` value, which is either a
/// plain string or an array of `{type: "text", text}` parts.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"] == "text")
            .map(|part| part["text"].as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Converts OpenAI message content into Anthropic content blocks.
fn content_blocks(content: &Value) -> Vec<Value> {
    match content {
        Value::String(s) => {
            if s.is_empty() {
                vec![]
            } else {
                vec![json!({"type": "text", "text": s})]
            }
        }
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part["type"].as_str() {
                Some("text") => {
                    let text = part["text"].as_str().unwrap_or_default();
                    (!text.is_empty()).then(|| json!({"type": "text", "text": text}))
                }
                Some("image_url") => {
                    let url = part["image_url"]["url"].as_str().unwrap_or_default();
                    if let Some(data_uri) = url.strip_prefix("data:") {
                        // data:<media_type>;base64,<data>
                        let (meta, data) = data_uri.split_once(',')?;
                        let media_type = meta.strip_suffix(";base64").unwrap_or(meta);
                        Some(json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": media_type,
                                "data": data,
                            }
                        }))
                    } else if url.is_empty() {
                        None
                    } else {
                        Some(json!({
                            "type": "image",
                            "source": {"type": "url", "url": url}
                        }))
                    }
                }
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

/// Translates an OpenAI Chat Completions request body into an Anthropic
/// Messages request body.
pub fn openai_to_anthropic(body: &Value) -> Result<Value, String> {
    let model = body["model"]
        .as_str()
        .ok_or_else(|| "missing required field: model".to_string())?;

    let mut out = Map::new();
    out.insert("model".into(), json!(model));

    let max_tokens = body["max_tokens"]
        .as_u64()
        .or_else(|| body["max_completion_tokens"].as_u64())
        .unwrap_or(DEFAULT_MAX_TOKENS);
    out.insert("max_tokens".into(), json!(max_tokens));

    for key in ["temperature", "top_p"] {
        if let Some(value) = body.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), value.clone());
        }
    }

    match &body["stop"] {
        Value::String(s) => {
            out.insert("stop_sequences".into(), json!([s]));
        }
        Value::Array(stops) => {
            out.insert("stop_sequences".into(), json!(stops));
        }
        _ => {}
    }

    if body["stream"].as_bool() == Some(true) {
        out.insert("stream".into(), json!(true));
    }

    let empty = vec![];
    let messages = body["messages"].as_array().unwrap_or(&empty);

    // System and developer messages become the Anthropic `system` prompt.
    let system: Vec<String> = messages
        .iter()
        .filter(|m| matches!(m["role"].as_str(), Some("system") | Some("developer")))
        .map(|m| content_text(&m["content"]))
        .filter(|s| !s.is_empty())
        .collect();
    if !system.is_empty() {
        out.insert("system".into(), json!(system.join("\n\n")));
    }

    let mut anthropic_messages: Vec<Value> = Vec::new();
    for message in messages {
        match message["role"].as_str() {
            Some("system") | Some("developer") => {}
            Some("user") => {
                let blocks = content_blocks(&message["content"]);
                if !blocks.is_empty() {
                    anthropic_messages.push(json!({"role": "user", "content": blocks}));
                }
            }
            Some("assistant") => {
                let mut blocks = content_blocks(&message["content"]);
                if let Some(tool_calls) = message["tool_calls"].as_array() {
                    for call in tool_calls {
                        let arguments = call["function"]["arguments"].as_str().unwrap_or("{}");
                        let input: Value = serde_json::from_str(arguments).unwrap_or_else(|e| {
                            log::debug!(
                                "Failed to parse tool_call arguments as JSON: {e}; \
                                 arguments were: {arguments}"
                            );
                            json!({})
                        });
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": call["id"].as_str().unwrap_or_default(),
                            "name": call["function"]["name"].as_str().unwrap_or_default(),
                            "input": input,
                        }));
                    }
                }
                if !blocks.is_empty() {
                    anthropic_messages.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            Some("tool") => {
                let result_block = json!({
                    "type": "tool_result",
                    "tool_use_id": message["tool_call_id"].as_str().unwrap_or_default(),
                    "content": content_text(&message["content"]),
                });
                // Anthropic requires alternating roles; consecutive tool
                // results are folded into the preceding user message when it
                // only holds tool results.
                let appended = anthropic_messages.last_mut().is_some_and(|last| {
                    let is_tool_result_user = last["role"] == "user"
                        && last["content"].as_array().is_some_and(|blocks| {
                            blocks.iter().all(|b| b["type"] == "tool_result")
                        });
                    if is_tool_result_user {
                        if let Some(blocks) = last["content"].as_array_mut() {
                            blocks.push(result_block.clone());
                            return true;
                        }
                    }
                    false
                });
                if !appended {
                    anthropic_messages.push(json!({"role": "user", "content": [result_block]}));
                }
            }
            _ => {}
        }
    }
    out.insert("messages".into(), json!(anthropic_messages));

    if let Some(tools) = body["tools"].as_array() {
        let tools: Vec<Value> = tools
            .iter()
            .filter(|tool| tool["type"] == "function")
            .map(|tool| {
                json!({
                    "name": tool["function"]["name"],
                    "description": tool["function"]["description"],
                    "input_schema": tool["function"]["parameters"],
                })
            })
            .collect();
        if !tools.is_empty() {
            out.insert("tools".into(), json!(tools));
        }
    }

    match &body["tool_choice"] {
        Value::String(choice) => {
            let mapped = match choice.as_str() {
                "auto" => Some(json!({"type": "auto"})),
                "none" => Some(json!({"type": "none"})),
                "required" => Some(json!({"type": "any"})),
                _ => None,
            };
            if let Some(mapped) = mapped {
                out.insert("tool_choice".into(), mapped);
            }
        }
        choice @ Value::Object(_) => {
            if let Some(name) = choice["function"]["name"].as_str() {
                out.insert("tool_choice".into(), json!({"type": "tool", "name": name}));
            }
        }
        _ => {}
    }

    Ok(Value::Object(out))
}

fn map_stop_reason(stop_reason: &str) -> &'static str {
    match stop_reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}

fn map_usage(usage: &Value) -> Value {
    let prompt = usage["input_tokens"].as_u64().unwrap_or(0)
        + usage["cache_creation_input_tokens"].as_u64().unwrap_or(0)
        + usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
    let completion = usage["output_tokens"].as_u64().unwrap_or(0);
    json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion,
    })
}

/// A normalized piece of assistant content, independent of whether it arrived
/// as a complete (non-streaming) block or as the opening of a streamed block.
///
/// This is the single place Anthropic content-block types are classified into
/// their OpenAI targets (text → `content`, thinking → `reasoning_content`,
/// tool_use → `tool_calls`); both the batch and streaming paths fold through
/// it, so a new block type is one edit here.
enum ContentPiece {
    Text(String),
    Reasoning(String),
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

/// Maps one complete Anthropic content block to its OpenAI piece, or `None` for
/// block types the bridge does not surface.
fn content_piece(block: &Value) -> Option<ContentPiece> {
    match block["type"].as_str()? {
        "text" => Some(ContentPiece::Text(
            block["text"].as_str().unwrap_or_default().to_string(),
        )),
        "thinking" => Some(ContentPiece::Reasoning(
            block["thinking"].as_str().unwrap_or_default().to_string(),
        )),
        "tool_use" => Some(ContentPiece::ToolUse {
            id: block["id"].as_str().unwrap_or_default().to_string(),
            name: block["name"].as_str().unwrap_or_default().to_string(),
            input: block["input"].clone(),
        }),
        _ => None,
    }
}

/// Translates a non-streaming Anthropic Messages response into an OpenAI
/// Chat Completions response.
pub fn anthropic_to_openai_response(resp: &Value) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();

    if let Some(blocks) = resp["content"].as_array() {
        for block in blocks {
            match content_piece(block) {
                Some(ContentPiece::Text(t)) => text.push_str(&t),
                Some(ContentPiece::Reasoning(r)) => reasoning.push_str(&r),
                Some(ContentPiece::ToolUse { id, name, input }) => {
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": input.to_string(),
                        }
                    }));
                }
                None => {}
            }
        }
    }

    let mut message = Map::new();
    message.insert("role".into(), json!("assistant"));
    message.insert("content".into(), json!(text));
    if !reasoning.is_empty() {
        message.insert("reasoning_content".into(), json!(reasoning));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), json!(tool_calls));
    }

    let finish_reason = map_stop_reason(resp["stop_reason"].as_str().unwrap_or("end_turn"));

    json!({
        "id": resp["id"],
        "object": "chat.completion",
        "created": unix_now(),
        "model": resp["model"],
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason,
        }],
        "usage": map_usage(&resp["usage"]),
    })
}

/// Translates an Anthropic error response body into an OpenAI-style error.
pub fn anthropic_to_openai_error(resp: &Value, status: u16) -> Value {
    let message = resp["error"]["message"]
        .as_str()
        .unwrap_or("upstream request failed");
    let error_type = resp["error"]["type"].as_str().unwrap_or("api_error");
    json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": status,
        }
    })
}

/// Stateful translator from an Anthropic Messages SSE stream to an OpenAI
/// Chat Completions chunk stream.
///
/// Feed raw bytes (as UTF-8 text) into [`SseTranslator::push`]; it returns
/// fully-framed OpenAI SSE lines (`data: {...}\n\n`, terminated by
/// `data: [DONE]\n\n`) as they become available. Partial events are buffered
/// across pushes.
pub struct SseTranslator {
    buffer: String,
    id: String,
    model: String,
    /// Maps an Anthropic content block index to the OpenAI tool call index.
    tool_indices: Vec<(u64, u64)>,
    next_tool_index: u64,
    finish_reason: Option<&'static str>,
    usage: Option<Value>,
}

impl SseTranslator {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            id: String::new(),
            model: String::new(),
            tool_indices: Vec::new(),
            next_tool_index: 0,
            finish_reason: None,
            usage: None,
        }
    }

    fn chunk(&self, delta: Value, finish_reason: Option<&str>, usage: Option<&Value>) -> String {
        let mut frame = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": unix_now(),
            "model": self.model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason,
            }],
        });
        if let Some(usage) = usage {
            frame["usage"] = usage.clone();
        }
        format!("data: {frame}\n\n")
    }

    fn tool_index_for_block(&self, block_index: u64) -> Option<u64> {
        self.tool_indices
            .iter()
            .find(|(block, _)| *block == block_index)
            .map(|(_, tool)| *tool)
    }

    /// Processes one decoded Anthropic SSE `data:` payload, returning any
    /// OpenAI SSE frames to emit.
    fn process_event(&mut self, data: &Value) -> Vec<String> {
        match data["type"].as_str() {
            Some("message_start") => {
                self.id = data["message"]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.model = data["message"]["model"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                vec![self.chunk(json!({"role": "assistant", "content": ""}), None, None)]
            }
            Some("content_block_start") => {
                let block_index = data["index"].as_u64().unwrap_or(0);
                // tool_use blocks open a streamed tool call; text/thinking
                // blocks carry their payload in subsequent deltas, so they
                // produce no frame here. Classification is shared with the
                // non-streaming path via `content_piece`.
                match content_piece(&data["content_block"]) {
                    Some(ContentPiece::ToolUse { id, name, .. }) => {
                        let tool_index = self.next_tool_index;
                        self.next_tool_index += 1;
                        self.tool_indices.push((block_index, tool_index));
                        vec![self.chunk(
                            json!({"tool_calls": [{
                                "index": tool_index,
                                "id": id,
                                "type": "function",
                                "function": {"name": name, "arguments": ""},
                            }]}),
                            None,
                            None,
                        )]
                    }
                    _ => vec![],
                }
            }
            Some("content_block_delta") => {
                let block_index = data["index"].as_u64().unwrap_or(0);
                let delta = &data["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => {
                        vec![self.chunk(json!({"content": delta["text"]}), None, None)]
                    }
                    Some("thinking_delta") => vec![self.chunk(
                        json!({"reasoning_content": delta["thinking"]}),
                        None,
                        None,
                    )],
                    Some("input_json_delta") => match self.tool_index_for_block(block_index) {
                        Some(tool_index) => vec![self.chunk(
                            json!({"tool_calls": [{
                                "index": tool_index,
                                "function": {"arguments": delta["partial_json"]},
                            }]}),
                            None,
                            None,
                        )],
                        None => vec![],
                    },
                    _ => vec![],
                }
            }
            Some("message_delta") => {
                if let Some(stop_reason) = data["delta"]["stop_reason"].as_str() {
                    self.finish_reason = Some(map_stop_reason(stop_reason));
                }
                if !data["usage"].is_null() {
                    self.usage = Some(map_usage(&data["usage"]));
                }
                vec![]
            }
            Some("message_stop") => {
                let finish = self.finish_reason.unwrap_or("stop");
                vec![
                    self.chunk(json!({}), Some(finish), self.usage.as_ref()),
                    "data: [DONE]\n\n".to_string(),
                ]
            }
            Some("error") => {
                let frame = json!({"error": data["error"]});
                vec![format!("data: {frame}\n\n"), "data: [DONE]\n\n".to_string()]
            }
            // ping, content_block_stop, and unknown events produce no output.
            _ => vec![],
        }
    }

    /// Feeds raw SSE text into the translator, returning translated OpenAI
    /// SSE frames for every complete upstream event.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        // Normalize CRLF to LF to prevent unbounded buffer growth when upstream
        // sends \r\n\r\n event separators that don't match the \n\n pattern.
        let text = if text.contains('\r') {
            text.replace("\r\n", "\n")
        } else {
            text.to_string()
        };
        self.buffer.push_str(&text);
        let mut out = Vec::new();
        // SSE events are separated by a blank line. Process every complete
        // event currently in the buffer, keeping any trailing partial event.
        while let Some(pos) = self.buffer.find("\n\n") {
            let event: String = self.buffer.drain(..pos + 2).collect();

            // Per SSE spec, multiple data: lines within one event are
            // joined with \n to form the complete event payload.
            let mut data_lines: Vec<&str> = Vec::new();
            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    data_lines.push(data.trim());
                }
            }
            // Ignore events with no data: lines (e.g. bare event: ping).
            if data_lines.is_empty() {
                continue;
            }
            let joined = data_lines.join("\n");
            match serde_json::from_str::<Value>(&joined) {
                Ok(value) => out.extend(self.process_event(&value)),
                Err(e) => log::debug!(
                    "Failed to parse SSE event payload as JSON: {e}; payload was: {joined}"
                ),
            }
        }
        out
    }
}

impl Default for SseTranslator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "translate_tests.rs"]
mod tests;
