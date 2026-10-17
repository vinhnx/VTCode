use super::{
    PROVIDER_ERROR_BODY_MAX_BYTES, assistant_interleaved_history_text, extract_reasoning_text_from_detail_values,
    extract_reasoning_text_from_serialized_details, float_to_json_number, is_interleaved_thinking_model,
    is_minimax_m2_model, normalize_reasoning_detail_object, parse_chat_request_openai_format,
    parse_response_openai_format, parse_usage_openai_format, read_provider_error_body, sampling_param_f64,
    serialize_message_content_openai,
};
use crate::provider::{AssistantPhase, Message};
use serde_json::{Value, json};

#[tokio::test]
async fn provider_error_body_reader_caps_untrusted_response_body() {
    let server = wiremock::MockServer::start().await;
    let mut body = "a".repeat(PROVIDER_ERROR_BODY_MAX_BYTES + 256);
    body.push_str("tail-that-must-not-be-read");

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(500).set_body_string(body))
        .mount(&server)
        .await;

    let response = reqwest::Client::new()
        .get(server.uri())
        .send()
        .await
        .expect("mock provider response should be available");
    let body = read_provider_error_body(response).await;

    assert_eq!(body.len(), PROVIDER_ERROR_BODY_MAX_BYTES);
    assert!(!body.contains("tail-that-must-not-be-read"));
}

#[test]
fn serialize_message_content_openai_text_only_parts_matches_concatenation() {
    use crate::provider::{ContentPart, MessageContent};
    // Text-only `Parts` take the fast path; it must equal the string form
    // and must not emit a JSON array.
    let content = MessageContent::parts(vec![
        ContentPart::text("alpha".to_string()),
        ContentPart::text("beta".to_string()),
    ]);
    assert_eq!(serialize_message_content_openai(&content), json!("alphabeta"));

    // An image makes it non-text; the array form is preserved.
    let with_image = MessageContent::parts(vec![
        ContentPart::text("look".to_string()),
        ContentPart::image("aGk=".to_string(), "image/png".to_string()),
    ]);
    assert!(serialize_message_content_openai(&with_image).is_array());
}

#[test]
fn minimax_m2_model_detection_handles_variants() {
    assert!(is_minimax_m2_model("MiniMax-M2.5"));
    assert!(is_minimax_m2_model("minimax/minimax-m2.5"));
    assert!(is_minimax_m2_model("MiniMaxAI/MiniMax-M2.5:novita"));
    assert!(is_minimax_m2_model("MiniMax-M2.7"));
    assert!(is_minimax_m2_model("MiniMax-M3"));
    assert!(is_minimax_m2_model("minimax/minimax-m2.7"));
    assert!(!is_minimax_m2_model("gpt-5"));
}

#[test]
fn interleaved_thinking_model_detection_handles_glm5() {
    assert!(is_interleaved_thinking_model("glm-5.1"));
    assert!(is_interleaved_thinking_model("zai-org/GLM-5.1:novita"));
    assert!(is_interleaved_thinking_model("glm-5.2"));
    assert!(is_interleaved_thinking_model("MiniMax-M2.7"));
    assert!(!is_interleaved_thinking_model("deepseek-r1"));
}

#[test]
fn normalize_reasoning_detail_object_decodes_stringified_json_object() {
    let normalized = normalize_reasoning_detail_object(&json!(r#"{"type":"reasoning.text","id":"r1","text":"trace"}"#))
        .expect("normalized object");
    assert!(normalized.is_object());
    assert_eq!(normalized["type"], "reasoning.text");
}

#[test]
fn normalize_reasoning_detail_object_rejects_plain_text() {
    assert!(normalize_reasoning_detail_object(&json!("plain-text")).is_none());
}

#[test]
fn assistant_interleaved_history_prefers_preserved_raw_detail() {
    let message = Message::assistant("answer".to_string())
        .with_reasoning_details(Some(vec![json!("<think>raw trace</think>answer")]));

    assert_eq!(
        assistant_interleaved_history_text(&message, "glm-5.1").as_deref(),
        Some("<think>raw trace</think>answer")
    );
}

#[test]
fn assistant_interleaved_history_wraps_reasoning_when_needed() {
    let message = Message::assistant("answer".to_string()).with_reasoning(Some("trace".to_string()));

    assert_eq!(
        assistant_interleaved_history_text(&message, "MiniMax-M2.7").as_deref(),
        Some("<think>trace</think>answer")
    );
}

#[test]
fn parse_openai_response_preserves_array_reasoning_details() {
    let response_json = json!({
        "choices": [{
            "message": {
                "content": "done",
                "reasoning_details": [{
                    "type": "reasoning.text",
                    "text": "step one"
                }]
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 1,
            "completion_tokens": 1,
            "total_tokens": 2
        }
    });

    let parsed = parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
        response_json,
        "test",
        "test-model".to_string(),
        false,
        None,
    )
    .expect("response should parse");

    assert_eq!(parsed.reasoning.as_deref(), Some("step one"));
    assert!(parsed.reasoning_details.is_some());
    let first_detail = parsed
        .reasoning_details
        .as_ref()
        .and_then(|details| details.first())
        .expect("reasoning detail should exist");
    let parsed_detail: Value = serde_json::from_str(first_detail).expect("reasoning detail should be json");
    assert_eq!(parsed_detail["type"], "reasoning.text");
}

#[test]
fn parse_openai_response_preserves_raw_interleaved_content_in_reasoning_details() {
    let response_json = json!({
        "choices": [{
            "message": {
                "content": "<think>step one</think>done"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 1,
            "completion_tokens": 1,
            "total_tokens": 2
        }
    });

    let parsed = parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
        response_json,
        "test",
        "glm-5.1".to_string(),
        false,
        None,
    )
    .expect("response should parse");

    assert_eq!(parsed.content.as_deref(), Some("done"));
    assert_eq!(parsed.reasoning.as_deref(), Some("step one"));
    assert_eq!(
        parsed
            .reasoning_details
            .as_ref()
            .and_then(|details| details.first())
            .map(String::as_str),
        Some("<think>step one</think>done")
    );
}

#[test]
fn extract_reasoning_text_from_detail_values_handles_stringified_json() {
    let details = vec![json!(r#"{"type":"reasoning.text","text":"trace one"}"#)];
    assert_eq!(extract_reasoning_text_from_detail_values(&details).as_deref(), Some("trace one"));
}

#[test]
fn extract_reasoning_text_from_serialized_details_handles_json_items() {
    let details = vec![
        json!({"type":"reasoning.text","text":"first"}).to_string(),
        json!({"type":"reasoning.text","text":"second"}).to_string(),
    ];
    assert_eq!(extract_reasoning_text_from_serialized_details(&details).as_deref(), Some("first\n\nsecond"));
}

#[test]
fn parse_chat_request_openai_format_preserves_assistant_phase() {
    let request = parse_chat_request_openai_format(
        &json!({
            "messages": [
                {"role": "assistant", "content": "Working", "phase": "commentary"},
                {"role": "assistant", "content": "Done", "phase": "final_answer"},
                {"role": "user", "content": "Continue", "phase": "commentary"}
            ]
        }),
        "default-model",
    )
    .expect("request should parse");

    assert_eq!(request.messages[0].phase, Some(AssistantPhase::Commentary));
    assert_eq!(request.messages[1].phase, Some(AssistantPhase::FinalAnswer));
    assert_eq!(request.messages[2].phase, None);
}

#[test]
fn parse_usage_openai_format_extracts_basic_fields() {
    let response = json!({
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }
    });
    let usage = parse_usage_openai_format(&response, false).expect("usage expected");
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.cached_prompt_tokens, None);
    assert_eq!(usage.cache_creation_tokens, None);
}

#[test]
fn parse_usage_openai_format_includes_cache_metrics_when_enabled() {
    let response = json!({
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150,
            "prompt_cache_hit_tokens": 30,
            "prompt_cache_miss_tokens": 70
        }
    });
    let usage = parse_usage_openai_format(&response, true).expect("usage expected");
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.cached_prompt_tokens, Some(30));
    assert_eq!(usage.cache_read_tokens, Some(30));
    assert_eq!(usage.cache_creation_tokens, Some(70));
}

#[test]
fn parse_usage_openai_format_maps_openai_details_cached_tokens() {
    let response = json!({
        "usage": {
            "prompt_tokens": 1000,
            "completion_tokens": 20,
            "total_tokens": 1020,
            "prompt_tokens_details": {"cached_tokens": 800, "cache_write_tokens": 50}
        }
    });
    let usage = parse_usage_openai_format(&response, true).expect("usage expected");
    assert_eq!(usage.cached_prompt_tokens, Some(800));
    assert_eq!(usage.cache_read_tokens, Some(800));
    assert_eq!(usage.cache_creation_tokens, Some(50));
}

#[test]
fn parse_usage_openai_format_maps_openrouter_top_level_cache_write_tokens() {
    let response = json!({
        "usage": {
            "prompt_tokens": 120,
            "completion_tokens": 80,
            "total_tokens": 200,
            "prompt_tokens_details": {"cached_tokens": 90},
            "prompt_cache_write_tokens": 15
        }
    });
    let usage = parse_usage_openai_format(&response, true).expect("usage expected");
    assert_eq!(usage.cached_prompt_tokens, Some(90));
    assert_eq!(usage.cache_read_tokens, Some(90));
    assert_eq!(usage.cache_creation_tokens, Some(15));
}

#[test]
fn parse_usage_openai_format_excludes_cache_metrics_when_disabled() {
    let response = json!({
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150,
            "prompt_cache_hit_tokens": 30,
            "prompt_cache_miss_tokens": 70
        }
    });
    let usage = parse_usage_openai_format(&response, false).expect("usage expected");
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.cached_prompt_tokens, None);
    assert_eq!(usage.cache_creation_tokens, None);
}

#[test]
fn parse_usage_openai_format_handles_missing_usage() {
    let response = json!({"choices": []});
    assert!(parse_usage_openai_format(&response, true).is_none());
}

#[test]
fn float_to_json_number_keeps_shortest_f32_wire_form() {
    let number = float_to_json_number(0.7).expect("finite value");
    assert_eq!(number.to_string(), "0.7");
    assert_ne!(number.to_string(), "0.699999988079071");

    let payload = json!({ "temperature": number });
    assert_eq!(serde_json::to_string(&payload).unwrap(), r#"{"temperature":0.7}"#);
}

#[test]
#[expect(clippy::float_cmp, reason = "round-trip must be bit-exact")]
fn float_to_json_number_round_trips_f32_values() {
    for raw in [0.5_f32, 0.25, 0.1, 1.0, 2.0, -0.75, 0.123_456_79] {
        let number = float_to_json_number(raw).expect("finite value");
        let wire: f64 = number.to_string().parse().expect("wire form parses");
        assert_eq!(wire as f32, raw, "wire form of {raw} must round-trip");
    }
}

#[test]
fn float_to_json_number_rejects_non_finite() {
    assert!(float_to_json_number(f32::NAN).is_err());
    assert!(float_to_json_number(f32::INFINITY).is_err());
    assert_eq!(sampling_param_f64(0.7_f32).to_bits(), 0.7_f64.to_bits(), "widened value must equal the f64 literal");
}
