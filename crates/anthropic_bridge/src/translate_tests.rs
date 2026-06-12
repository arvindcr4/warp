use serde_json::{json, Value};

use super::*;

#[test]
fn request_extracts_system_and_defaults_max_tokens() {
    let body = json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": "Be terse."},
            {"role": "user", "content": "hi"},
        ],
    });
    let out = openai_to_anthropic(&body).unwrap();
    assert_eq!(out["model"], "MiniMax-M3");
    assert_eq!(out["max_tokens"], 8192);
    assert_eq!(out["system"], "Be terse.");
    assert_eq!(out["messages"].as_array().unwrap().len(), 1);
    assert_eq!(out["messages"][0]["role"], "user");
    assert_eq!(out["messages"][0]["content"][0]["text"], "hi");
    assert!(out.get("stream").is_none());
}

#[test]
fn request_missing_model_is_an_error() {
    assert!(openai_to_anthropic(&json!({"messages": []})).is_err());
}

#[test]
fn request_maps_tools_and_tool_choice() {
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "x"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {}},
            }
        }],
        "tool_choice": "required",
    });
    let out = openai_to_anthropic(&body).unwrap();
    assert_eq!(out["tools"][0]["name"], "get_weather");
    assert_eq!(out["tools"][0]["input_schema"]["type"], "object");
    assert_eq!(out["tool_choice"]["type"], "any");
}

#[test]
fn request_maps_named_tool_choice() {
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "x"}],
        "tool_choice": {"type": "function", "function": {"name": "get_weather"}},
    });
    let out = openai_to_anthropic(&body).unwrap();
    assert_eq!(
        out["tool_choice"],
        json!({"type": "tool", "name": "get_weather"})
    );
}

#[test]
fn request_translates_assistant_tool_calls_and_merges_tool_results() {
    let body = json!({
        "model": "m",
        "messages": [
            {"role": "user", "content": "weather in SF and NYC?"},
            {"role": "assistant", "content": "", "tool_calls": [
                {"id": "call_1", "type": "function",
                 "function": {"name": "get_weather", "arguments": "{\"city\":\"SF\"}"}},
                {"id": "call_2", "type": "function",
                 "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}},
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
            {"role": "tool", "tool_call_id": "call_2", "content": "rainy"},
        ],
    });
    let out = openai_to_anthropic(&body).unwrap();
    let messages = out["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);

    let assistant = &messages[1];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["content"][0]["type"], "tool_use");
    assert_eq!(assistant["content"][0]["input"]["city"], "SF");

    // Both tool results fold into a single user message.
    let results = &messages[2];
    assert_eq!(results["role"], "user");
    let blocks = results["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["tool_use_id"], "call_1");
    assert_eq!(blocks[1]["content"], "rainy");
}

#[test]
fn request_passes_through_sampling_params_stop_and_stream() {
    let body = json!({
        "model": "m",
        "messages": [{"role": "user", "content": "x"}],
        "max_tokens": 512,
        "temperature": 0.3,
        "top_p": 0.9,
        "stop": ["END"],
        "stream": true,
    });
    let out = openai_to_anthropic(&body).unwrap();
    assert_eq!(out["max_tokens"], 512);
    assert_eq!(out["temperature"], 0.3);
    assert_eq!(out["top_p"], 0.9);
    assert_eq!(out["stop_sequences"], json!(["END"]));
    assert_eq!(out["stream"], true);
}

#[test]
fn response_translates_text_thinking_and_tool_use() {
    let resp = json!({
        "id": "msg_123",
        "model": "MiniMax-M3",
        "stop_reason": "tool_use",
        "content": [
            {"type": "thinking", "thinking": "pondering..."},
            {"type": "text", "text": "Checking the weather."},
            {"type": "tool_use", "id": "tu_1", "name": "get_weather", "input": {"city": "SF"}},
        ],
        "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 90},
    });
    let out = anthropic_to_openai_response(&resp);
    assert_eq!(out["id"], "msg_123");
    assert_eq!(out["object"], "chat.completion");
    let message = &out["choices"][0]["message"];
    assert_eq!(message["content"], "Checking the weather.");
    assert_eq!(message["reasoning_content"], "pondering...");
    assert_eq!(message["tool_calls"][0]["function"]["name"], "get_weather");
    let arguments: Value = serde_json::from_str(
        message["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(arguments["city"], "SF");
    assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(out["usage"]["prompt_tokens"], 100);
    assert_eq!(out["usage"]["completion_tokens"], 5);
    assert_eq!(out["usage"]["total_tokens"], 105);
}

#[test]
fn error_translation_keeps_message_and_type() {
    let resp =
        json!({"type": "error", "error": {"type": "authentication_error", "message": "bad key"}});
    let out = anthropic_to_openai_error(&resp, 401);
    assert_eq!(out["error"]["message"], "bad key");
    assert_eq!(out["error"]["type"], "authentication_error");
    assert_eq!(out["error"]["code"], 401);
}

fn data_frame(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn parse_frame(frame: &str) -> Value {
    serde_json::from_str(frame.strip_prefix("data: ").unwrap().trim()).unwrap()
}

#[test]
fn sse_translates_text_stream_end_to_end() {
    let mut translator = SseTranslator::new();
    let mut frames = Vec::new();

    frames.extend(translator.push(&data_frame(json!({
        "type": "message_start",
        "message": {"id": "msg_1", "model": "MiniMax-M3"},
    }))));
    frames.extend(translator.push(&data_frame(json!({
        "type": "content_block_start", "index": 0,
        "content_block": {"type": "text", "text": ""},
    }))));
    frames.extend(translator.push(&data_frame(json!({
        "type": "content_block_delta", "index": 0,
        "delta": {"type": "text_delta", "text": "Hello"},
    }))));
    frames.extend(translator.push(&data_frame(json!({
        "type": "message_delta",
        "delta": {"stop_reason": "end_turn"},
        "usage": {"input_tokens": 3, "output_tokens": 2},
    }))));
    frames.extend(translator.push(&data_frame(json!({"type": "message_stop"}))));

    assert_eq!(frames.len(), 4);
    let role = parse_frame(&frames[0]);
    assert_eq!(role["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(role["id"], "msg_1");
    assert_eq!(role["model"], "MiniMax-M3");

    let content = parse_frame(&frames[1]);
    assert_eq!(content["choices"][0]["delta"]["content"], "Hello");

    let finish = parse_frame(&frames[2]);
    assert_eq!(finish["choices"][0]["finish_reason"], "stop");
    assert_eq!(finish["usage"]["total_tokens"], 5);

    assert_eq!(frames[3], "data: [DONE]\n\n");
}

#[test]
fn sse_translates_tool_use_stream() {
    let mut translator = SseTranslator::new();
    translator.push(&data_frame(json!({
        "type": "message_start",
        "message": {"id": "msg_2", "model": "m"},
    })));

    let start = translator.push(&data_frame(json!({
        "type": "content_block_start", "index": 1,
        "content_block": {"type": "tool_use", "id": "tu_9", "name": "get_weather"},
    })));
    let start = parse_frame(&start[0]);
    let call = &start["choices"][0]["delta"]["tool_calls"][0];
    assert_eq!(call["index"], 0);
    assert_eq!(call["id"], "tu_9");
    assert_eq!(call["function"]["name"], "get_weather");

    let delta = translator.push(&data_frame(json!({
        "type": "content_block_delta", "index": 1,
        "delta": {"type": "input_json_delta", "partial_json": "{\"city\""},
    })));
    let delta = parse_frame(&delta[0]);
    assert_eq!(
        delta["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
        "{\"city\""
    );
}

#[test]
fn sse_buffers_events_split_across_pushes() {
    let mut translator = SseTranslator::new();
    let frame = data_frame(json!({
        "type": "message_start",
        "message": {"id": "msg_3", "model": "m"},
    }));
    let (first, second) = frame.split_at(frame.len() / 2);
    assert!(translator.push(first).is_empty());
    let frames = translator.push(second);
    assert_eq!(frames.len(), 1);
    assert_eq!(parse_frame(&frames[0])["id"], "msg_3");
}

#[test]
fn sse_ignores_pings_and_event_lines() {
    let mut translator = SseTranslator::new();
    let out = translator.push("event: ping\ndata: {\"type\": \"ping\"}\n\n");
    assert!(out.is_empty());
}
