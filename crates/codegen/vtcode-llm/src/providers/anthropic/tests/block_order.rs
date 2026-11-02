use crate::provider::{LLMResponse, Message};
use crate::providers::anthropic::request_builder::{RequestBuilderContext, convert_to_anthropic_format};
use crate::providers::anthropic::response_parser::parse_response;
use serde_json::{Value, json};
use vtcode_config::constants::models;
use vtcode_config::core::{AnthropicConfig, AnthropicPromptCacheSettings};

/// Mirror of the runloop: an assistant turn stored from a response.
fn assistant_message(response: LLMResponse) -> Message {
    let details = response
        .reasoning_details
        .map(|details| details.into_iter().map(Value::String).collect());
    let content = response.content.unwrap_or_default();
    match response.tool_calls {
        Some(tool_calls) => Message::assistant_with_tools_and_reasoning(content, tool_calls, details),
        None => Message::assistant(content).with_reasoning_details(details),
    }
}

fn replayed_assistant_content(history: Vec<Message>) -> Vec<Value> {
    let request = crate::provider::LLMRequest {
        model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        messages: history.into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: false,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };
    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");
    payload["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == "assistant")
        .expect("assistant message")["content"]
        .as_array()
        .expect("assistant content")
        .clone()
}

fn interleaved_content() -> Value {
    json!([
        { "type": "thinking", "thinking": "", "signature": "sig-1" },
        { "type": "text", "text": "Reading the parser first." },
        { "type": "thinking", "thinking": "Found the entry point.", "signature": "sig-2" },
        { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": { "path": "src/parser.rs" } }
    ])
}

fn history_with(assistant: Message) -> Vec<Message> {
    vec![
        Message::user("fix the parser".to_string()),
        assistant,
        Message::tool_response("toolu_1".to_string(), "fn parse() {}".to_string()),
    ]
}

#[test]
fn interleaved_response_replays_blocks_in_received_order() {
    let response = parse_response(
        json!({ "content": interleaved_content(), "stop_reason": "tool_use" }),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
    )
    .expect("parse");

    let content = replayed_assistant_content(history_with(assistant_message(response)));

    assert_eq!(content, interleaved_content().as_array().expect("array").clone());
}

#[test]
fn fixed_order_response_stores_no_block_order_record() {
    let response = parse_response(
        json!({
            "content": [
                { "type": "thinking", "thinking": "", "signature": "sig-1" },
                { "type": "text", "text": "Reading." },
                { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {} }
            ],
            "stop_reason": "tool_use"
        }),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
    )
    .expect("parse");

    assert!(
        !response
            .reasoning_details
            .as_ref()
            .expect("details")
            .iter()
            .any(|detail| detail.contains("anthropic_block_order"))
    );
}

#[test]
fn mid_output_fallback_drops_declined_thinking_and_tool_use_before_boundary() {
    let response = parse_response(
        json!({
            "content": [
                { "type": "thinking", "thinking": "Refused model reasoning.", "signature": "sig-1" },
                { "type": "text", "text": "Checking. " },
                { "type": "tool_use", "id": "toolu_declined", "name": "read_file", "input": {} },
                { "type": "fallback", "from": { "model": "claude-fable-5-1" }, "to": { "model": "claude-opus-4-8" } },
                { "type": "text", "text": "Here is the answer." }
            ],
            "stop_reason": "end_turn"
        }),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
    )
    .expect("parse");

    assert!(response.tool_calls.is_none());
    assert!(response.reasoning.is_none());
    assert_eq!(response.content.as_deref(), Some("Checking. Here is the answer."));
    let details: Vec<Value> = response
        .reasoning_details
        .clone()
        .expect("details")
        .iter()
        .map(|detail| serde_json::from_str(detail).expect("detail json"))
        .collect();
    assert!(details.iter().all(|detail| detail["type"] != "thinking"));
    assert!(details.iter().any(|detail| {
        detail["type"] == "fallback"
            && detail["from"]["model"] == "claude-fable-5-1"
            && detail["to"]["model"] == "claude-opus-4-8"
    }));

    let content = replayed_assistant_content(vec![
        Message::user("fix the parser".to_string()),
        assistant_message(response),
        Message::user("thanks".to_string()),
    ]);
    assert_eq!(
        content,
        vec![
            json!({ "type": "text", "text": "Checking. " }),
            json!({ "type": "text", "text": "Here is the answer." }),
        ]
    );
}

#[test]
fn edited_assistant_text_falls_back_to_default_order() {
    let response = parse_response(
        json!({ "content": interleaved_content(), "stop_reason": "tool_use" }),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
    )
    .expect("parse");
    let mut assistant = assistant_message(response);
    assistant.content = crate::provider::MessageContent::Text("rewritten by the runtime".to_string());

    let content = replayed_assistant_content(history_with(assistant));
    let types: Vec<&str> = content.iter().filter_map(|block| block["type"].as_str()).collect();

    assert_eq!(types, ["thinking", "thinking", "text", "tool_use"]);
    assert_eq!(content[2]["text"], "rewritten by the runtime");
}
