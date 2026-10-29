use crate::provider::FinishReason;
use crate::providers::anthropic::response_parser::*;
use serde_json::json;

#[test]
fn test_parse_finish_reason() {
    assert!(matches!(parse_finish_reason("end_turn"), FinishReason::Stop));
    assert!(matches!(parse_finish_reason("max_tokens"), FinishReason::Length));
    assert!(matches!(parse_finish_reason("tool_use"), FinishReason::ToolCalls));
    assert!(matches!(parse_finish_reason("compaction"), FinishReason::Pause));
    assert!(matches!(parse_finish_reason("pause_turn"), FinishReason::Pause));
    assert!(matches!(parse_finish_reason("refusal"), FinishReason::Refusal));
    assert!(matches!(parse_finish_reason("model_context_window_exceeded"), FinishReason::Length));
}

#[test]
fn test_parse_response_basic() {
    let response_json = json!({
        "content": [
            {"type": "text", "text": "Hello, world!"}
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5
        }
    });

    let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
    assert_eq!(response.content.as_deref(), Some("Hello, world!"));
    assert!(matches!(response.finish_reason, FinishReason::Stop));
}

#[test]
fn test_parse_response_carries_refusal_stop_details() {
    let response_json = json!({
        "content": [],
        "stop_reason": "refusal",
        "stop_details": {
            "type": "refusal",
            "category": "cyber",
            "explanation": "declined",
            "fallback_credit_token": "credit-1",
            "fallback_has_prefill_claim": false
        }
    });

    let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
    assert!(matches!(response.finish_reason, FinishReason::Refusal));
    let details = response.reasoning_details.expect("stop_details detail");
    assert_eq!(details.len(), 1);
    let detail: serde_json::Value = serde_json::from_str(&details[0]).expect("serialized stop_details");
    assert_eq!(detail["type"], "stop_details");
    assert_eq!(detail["category"], "cyber");
    assert_eq!(detail["explanation"], "declined");
    assert_eq!(detail["fallback_credit_token"], "credit-1");
    assert_eq!(detail["fallback_has_prefill_claim"], false);
}

#[test]
fn test_parse_response_ignores_null_stop_details() {
    let response_json = json!({
        "content": [{"type": "text", "text": "done"}],
        "stop_reason": "end_turn",
        "stop_details": null
    });

    let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
    assert!(response.reasoning_details.is_none(), "details: {:?}", response.reasoning_details);
}

#[test]
fn test_parse_response_with_compaction() {
    let response_json = json!({
        "content": [
            {
                "type": "compaction",
                "content": "opaque summary",
                "signature": "signed-summary",
                "encrypted_content": "opaque-extension"
            }
        ],
        "stop_reason": "compaction"
    });

    let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
    assert!(matches!(response.finish_reason, FinishReason::Pause));
    assert_eq!(response.compaction.as_deref(), Some("opaque summary"));
    let details = response.reasoning_details.expect("compaction detail");
    assert_eq!(details.len(), 1);
    let detail: serde_json::Value = serde_json::from_str(&details[0]).expect("serialized compaction detail");
    assert_eq!(detail["signature"], "signed-summary");
    assert_eq!(detail["encrypted_content"], "opaque-extension");
}

#[test]
fn test_parse_response_keeps_compaction_block_when_content_is_null() {
    let response = parse_response(
        json!({
            "content": [{"type": "compaction", "content": null, "signature": "signed-summary"}],
            "stop_reason": "compaction"
        }),
        "claude-sonnet-5".to_string(),
    )
    .expect("parse response");

    assert!(response.compaction.is_none());
    let details = response.reasoning_details.expect("opaque compaction detail");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&details[0]).expect("detail")["content"], json!(null));
}

#[test]
fn test_parse_response_with_thinking() {
    let response_json = json!({
        "content": [
            {"type": "thinking", "thinking": "Let me think...", "signature": "sig123"},
            {"type": "text", "text": "The answer is 42."}
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 20
        }
    });

    let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
    let reasoning = response.reasoning.as_deref().expect("expected reasoning content");
    assert!(reasoning.contains("Let me think"));
    assert_eq!(
        response.reasoning_details,
        Some(vec![
            json!({
                "type": "thinking",
                "thinking": "Let me think...",
                "signature": "sig123"
            })
            .to_string()
        ])
    );
    assert_eq!(response.content.as_deref(), Some("The answer is 42."));
}

#[test]
fn test_parse_response_with_tool_use() {
    let response_json = json!({
        "content": [
            {
                "type": "tool_use",
                "id": "tool_123",
                "name": "get_weather",
                "input": {"location": "NYC"}
            }
        ],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5
        }
    });

    let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
    let tool_calls = response.tool_calls.as_ref().expect("expected tool calls");
    assert_eq!(tool_calls.len(), 1);
    let function = tool_calls[0].function.as_ref().expect("expected function call");
    assert_eq!(function.name, "get_weather");
}
