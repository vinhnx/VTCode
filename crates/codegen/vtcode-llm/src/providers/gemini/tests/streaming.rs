use super::*;

#[test]
fn interaction_stream_payload_reconstructs_text_reasoning_and_tool_calls() {
    let mut state = InteractionStreamState::default();

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "interaction.start",
            "interaction": {
                "id": "interaction_stream_1",
                "status": "in_progress"
            }
        }),
    )
    .expect("start event should parse");
    assert!(events.is_empty());

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.start",
            "index": 0,
            "content": {
                "type": "thought"
            }
        }),
    )
    .expect("thought start should parse");
    assert!(events.is_empty());

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.delta",
            "index": 0,
            "delta": {
                "type": "thought",
                "thought": "Private provider reasoning."
            }
        }),
    )
    .expect("private thought delta should parse");
    assert!(events.is_empty(), "raw thought content must not become a public reasoning event");

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.delta",
            "index": 0,
            "delta": {
                "type": "thought_summary",
                "content": {
                    "text": "Looked up the city first."
                }
            }
        }),
    )
    .expect("thought delta should parse");
    assert!(matches!(
        events.as_slice(),
        [LLMStreamEvent::Reasoning { delta }] if delta == "Looked up the city first."
    ));

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.delta",
            "index": 0,
            "delta": {
                "type": "thought_signature",
                "signature": "thought_sig_stream"
            }
        }),
    )
    .expect("thought signature should parse");
    assert!(events.is_empty());

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.start",
            "index": 1,
            "content": {
                "type": "text"
            }
        }),
    )
    .expect("text start should parse");
    assert!(events.is_empty());

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.delta",
            "index": 1,
            "delta": {
                "type": "text",
                "text": "Looking it up."
            }
        }),
    )
    .expect("text delta should parse");
    assert!(matches!(
        events.as_slice(),
        [LLMStreamEvent::Token { delta }] if delta == "Looking it up."
    ));

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.start",
            "index": 2,
            "content": {
                "type": "function_call"
            }
        }),
    )
    .expect("function start should parse");
    assert!(events.is_empty());

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "content.delta",
            "index": 2,
            "delta": {
                "type": "function_call",
                "id": "call_weather_stream",
                "name": "get_weather",
                "arguments": {
                    "location": "Utqiagvik, Alaska"
                },
                "signature": "tool_sig_stream"
            }
        }),
    )
    .expect("function delta should parse");
    assert!(events.is_empty());

    let events = GeminiProvider::apply_interaction_stream_payload(
        &mut state,
        &json!({
            "event_type": "interaction.complete",
            "interaction": {
                "id": "interaction_stream_1",
                "status": "requires_action",
                "usage": {
                    "total_input_tokens": 12,
                    "total_output_tokens": 8,
                    "total_tokens": 20
                }
            }
        }),
    )
    .expect("complete event should parse");
    assert!(events.is_empty());
    assert!(state.completed);

    let llm_response =
        GeminiProvider::finalize_interaction_stream_state(state, models::google::GEMINI_3_FLASH_PREVIEW.to_string())
            .expect("finalized interaction stream should parse");

    assert_eq!(llm_response.request_id.as_deref(), Some("interaction_stream_1"));
    assert_eq!(llm_response.content.as_deref(), Some("Looking it up."));
    assert_eq!(llm_response.reasoning.as_deref(), Some("Looked up the city first."));
    assert!(
        llm_response
            .reasoning_details
            .as_ref()
            .is_some_and(|details| details.iter().any(|detail| detail.contains("Private provider reasoning.")))
    );
    assert_eq!(llm_response.finish_reason, FinishReason::ToolCalls);
    assert_eq!(llm_response.usage.as_ref().map(|u| u.total_tokens), Some(20));
    let tool_calls = llm_response.tool_calls.expect("tool call should exist");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].function.as_ref().expect("function").name, "get_weather");
    assert_eq!(tool_calls[0].thought_signature.as_deref(), Some("tool_sig_stream"));
}

#[test]
fn validate_request_rejects_store_false_with_previous_interaction_id() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![Message::user("hello".to_string())].into(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        previous_response_id: Some("interaction_123".to_string()),
        response_store: Some(false),
        ..Default::default()
    };

    let error = provider.validate_request(&request).expect_err("request should be rejected");
    assert!(error.to_string().contains("cannot set store=false"));
}

#[test]
fn sanitize_function_parameters_removes_additional_properties() {
    let parameters = json!({
        "type": "object",
        "properties": {
            "input": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    });

    let sanitized = sanitize_function_parameters(parameters);
    let root = sanitized.as_object().expect("root parameters should remain an object");
    assert!(!root.contains_key("additionalProperties"));

    let nested = root
        .get("properties")
        .and_then(|value| value.as_object())
        .and_then(|props| props.get("input"))
        .and_then(|value| value.as_object())
        .expect("nested object should be preserved");
    assert!(!nested.contains_key("additionalProperties"));
}

#[test]
fn sanitize_function_parameters_removes_examples() {
    let parameters = json!({
        "type": "object",
        "examples": [{ "path": "src/lib.rs" }],
        "properties": {
            "path": {
                "type": "string",
                "examples": ["src/lib.rs"]
            }
        }
    });

    let sanitized = sanitize_function_parameters(parameters);
    assert!(sanitized.get("examples").is_none());
    assert!(sanitized["properties"]["path"].get("examples").is_none());
}

#[test]
fn sanitize_function_parameters_removes_exclusive_min_max() {
    // Test case for the bug: exclusiveMaximum and exclusiveMinimum in nested properties
    let parameters = json!({
        "type": "object",
        "properties": {
            "max_length": {
                "type": "integer",
                "exclusiveMaximum": 1000000,
                "exclusiveMinimum": 0,
                "minimum": 1,
                "maximum": 999999,
                "description": "Maximum number of characters"
            }
        }
    });

    let sanitized = sanitize_function_parameters(parameters);
    let props = sanitized
        .get("properties")
        .and_then(|v| v.as_object())
        .and_then(|p| p.get("max_length"))
        .and_then(|v| v.as_object())
        .expect("max_length property should exist");

    // These unsupported fields should be removed
    assert!(!props.contains_key("exclusiveMaximum"), "exclusiveMaximum should be removed");
    assert!(!props.contains_key("exclusiveMinimum"), "exclusiveMinimum should be removed");
    assert!(!props.contains_key("minimum"), "minimum should be removed");
    assert!(!props.contains_key("maximum"), "maximum should be removed");

    // These supported fields should be preserved
    assert_eq!(props.get("type").and_then(|v| v.as_str()), Some("integer"));
    assert_eq!(props.get("description").and_then(|v| v.as_str()), Some("Maximum number of characters"));
}

#[test]
fn sanitize_function_parameters_drops_invalid_required_entries() {
    let parameters = json!({
        "type": "object",
        "properties": {
            "label": { "type": "string" }
        },
        "required": ["label", "description"]
    });

    let sanitized = sanitize_function_parameters(parameters);
    assert_eq!(sanitized["required"], json!(["label"]));
}

#[test]
fn sanitize_function_parameters_preserves_real_details_property() {
    let parameters = json!({
        "type": "object",
        "properties": {
            "description": { "type": "string" },
            "details": { "type": "string" }
        },
        "required": ["description", "details"]
    });

    let sanitized = sanitize_function_parameters(parameters);
    let properties = sanitized["properties"].as_object().expect("properties should remain an object");

    assert!(properties.contains_key("description"));
    assert!(properties.contains_key("details"));
    assert_eq!(sanitized["required"], json!(["description", "details"]));
}

#[test]
fn apply_stream_delta_handles_replayed_chunks() {
    let mut acc = String::new();
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello"), Some("Hello".to_string()));
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello world"), Some(" world".to_string()));
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello world"), None);
    assert_eq!(acc, "Hello world");
}

#[test]
fn apply_stream_delta_handles_incremental_chunks() {
    let mut acc = String::new();
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello"), Some("Hello".to_string()));
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, " there"), Some(" there".to_string()));
    assert_eq!(acc, "Hello there");
}

#[test]
fn apply_stream_delta_handles_rewrites() {
    let mut acc = String::new();
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello world"), Some("Hello world".to_string()));
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello"), None);
    assert_eq!(acc, "Hello");
}
