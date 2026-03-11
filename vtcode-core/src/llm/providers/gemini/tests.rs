use super::wire::{
    Candidate, Content, FunctionCall as GeminiFunctionCall, GenerateContentResponse, Part,
};
use super::*;
use crate::config::constants::models;
use crate::llm::provider::{
    MessageContent, MessageRole, SpecificFunctionChoice, SpecificToolChoice, ToolDefinition,
};
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::traits::Tool;
use serde_json::json;

#[test]
fn convert_to_gemini_request_maps_history_and_system_prompt() {
    let provider = GeminiProvider::new("test-key".to_string());
    let mut assistant_message = Message::assistant("Sure thing".to_string());
    assistant_message.tool_calls = Some(vec![ToolCall::function(
        "call_1".to_string(),
        "list_files".to_string(),
        json!({ "path": "." }).to_string(),
    )]);

    let tool_response =
        Message::tool_response("call_1".to_string(), json!({ "result": "ok" }).to_string());

    let tool_def = ToolDefinition::function(
        "list_files".to_string(),
        "List files".to_string(),
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        }),
    );

    let request = LLMRequest {
        messages: vec![
            Message::user("hello".to_string()),
            assistant_message,
            tool_response,
        ],
        system_prompt: Some(Arc::new("System prompt".to_string())),
        tools: Some(Arc::new(vec![tool_def])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        max_tokens: Some(256),
        temperature: Some(0.4),
        tool_choice: Some(ToolChoice::Specific(SpecificToolChoice {
            tool_type: "function".to_string(),
            function: SpecificFunctionChoice {
                name: "list_files".to_string(),
            },
        })),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    let system_instruction = gemini_request
        .system_instruction
        .expect("system instruction should be present");
    assert!(matches!(
        system_instruction.parts.as_slice(),
        [Part::Text {
            text,
            thought_signature: _
        }] if text == "System prompt"
    ));

    assert_eq!(gemini_request.contents.len(), 3);
    assert_eq!(gemini_request.contents[0].role, "user");
    assert!(
        gemini_request.contents[1]
            .parts
            .iter()
            .any(|part| matches!(part, Part::FunctionCall { .. }))
    );
    let tool_part = gemini_request.contents[2]
        .parts
        .iter()
        .find_map(|part| match part {
            Part::FunctionResponse {
                function_response, ..
            } => Some(function_response),
            _ => None,
        })
        .expect("tool response part should exist");
    assert_eq!(tool_part.name, "list_files");
}

#[test]
fn convert_from_gemini_response_extracts_tool_calls() {
    let response = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: "model".to_string(),
                parts: vec![
                    Part::Text {
                        text: "Here you go".to_string(),
                        thought_signature: None,
                    },
                    Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "list_files".to_string(),
                            args: json!({ "path": "." }),
                            id: Some("call_1".to_string()),
                        },
                        thought_signature: None,
                    },
                ],
            },
            finish_reason: Some("FUNCTION_CALL".to_string()),
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };

    let llm_response = GeminiProvider::convert_from_gemini_response(
        response,
        models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
    )
    .expect("conversion should succeed");

    assert_eq!(llm_response.content.as_deref(), Some("Here you go"));
    let calls = llm_response
        .tool_calls
        .expect("tool call should be present");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].function.as_ref().unwrap().name, "list_files");
    assert!(
        calls[0]
            .function
            .as_ref()
            .unwrap()
            .arguments
            .contains("path")
    );
    assert_eq!(llm_response.finish_reason, FinishReason::ToolCalls);
}

#[test]
fn convert_to_gemini_request_keeps_apply_patch_as_function_tool() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![Message::user("patch this file".to_string())],
        tools: Some(Arc::new(vec![ToolDefinition::apply_patch(
            "Apply VT Code patches".to_string(),
        )])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");
    let tools = gemini_request.tools.expect("tools should be present");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_declarations.len(), 1);
    assert_eq!(tools[0].function_declarations[0].name, "apply_patch");
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
    let root = sanitized
        .as_object()
        .expect("root parameters should remain an object");
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
    assert!(
        !props.contains_key("exclusiveMaximum"),
        "exclusiveMaximum should be removed"
    );
    assert!(
        !props.contains_key("exclusiveMinimum"),
        "exclusiveMinimum should be removed"
    );
    assert!(!props.contains_key("minimum"), "minimum should be removed");
    assert!(!props.contains_key("maximum"), "maximum should be removed");

    // These supported fields should be preserved
    assert_eq!(props.get("type").and_then(|v| v.as_str()), Some("integer"));
    assert_eq!(
        props.get("description").and_then(|v| v.as_str()),
        Some("Maximum number of characters")
    );
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
fn sanitize_function_parameters_renames_nested_description_property_names() {
    let parameters = RequestUserInputTool
        .parameter_schema()
        .expect("request_user_input schema");

    let sanitized = sanitize_function_parameters(parameters);
    let option_items =
        &sanitized["properties"]["questions"]["items"]["properties"]["options"]["items"];
    let option_properties = option_items["properties"]
        .as_object()
        .expect("option properties should remain an object");

    assert!(!option_properties.contains_key("description"));
    assert_eq!(
        option_properties["details"]["description"],
        json!("One short sentence explaining impact/tradeoff if selected.")
    );
    assert_eq!(option_items["required"], json!(["label", "details"]));
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
    let properties = sanitized["properties"]
        .as_object()
        .expect("properties should remain an object");

    assert!(properties.contains_key("description"));
    assert!(properties.contains_key("details"));
    assert_eq!(sanitized["required"], json!(["description", "details"]));
}

#[test]
fn apply_stream_delta_handles_replayed_chunks() {
    let mut acc = String::new();
    assert_eq!(
        GeminiProvider::apply_stream_delta(&mut acc, "Hello"),
        Some("Hello".to_string())
    );
    assert_eq!(
        GeminiProvider::apply_stream_delta(&mut acc, "Hello world"),
        Some(" world".to_string())
    );
    assert_eq!(
        GeminiProvider::apply_stream_delta(&mut acc, "Hello world"),
        None
    );
    assert_eq!(acc, "Hello world");
}

#[test]
fn apply_stream_delta_handles_incremental_chunks() {
    let mut acc = String::new();
    assert_eq!(
        GeminiProvider::apply_stream_delta(&mut acc, "Hello"),
        Some("Hello".to_string())
    );
    assert_eq!(
        GeminiProvider::apply_stream_delta(&mut acc, " there"),
        Some(" there".to_string())
    );
    assert_eq!(acc, "Hello there");
}

#[test]
fn apply_stream_delta_handles_rewrites() {
    let mut acc = String::new();
    assert_eq!(
        GeminiProvider::apply_stream_delta(&mut acc, "Hello world"),
        Some("Hello world".to_string())
    );
    assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello"), None);
    assert_eq!(acc, "Hello");
}

#[test]
fn convert_to_gemini_request_includes_reasoning_config() {
    use crate::config::constants::models;
    use crate::config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test High effort level for Gemini 3 Pro
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::High),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    // Check that thinkingConfig is present in generationConfig and has the correct value for High effort
    let generation_config = gemini_request
        .generation_config
        .expect("generation_config should be present");
    let thinking_config = generation_config
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(thinking_config.thinking_level.as_deref().unwrap(), "high");

    // Test Low effort level for Gemini 3 Pro
    let request_low = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Low),
        ..Default::default()
    };

    let gemini_request_low = provider
        .convert_to_gemini_request(&request_low)
        .expect("conversion should succeed");

    // Check that thinkingConfig is present in generationConfig and has "low" value for Low effort
    let generation_config_low = gemini_request_low
        .generation_config
        .expect("generation_config should be present for low effort");
    let thinking_config_low = generation_config_low
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(
        thinking_config_low.thinking_level.as_deref().unwrap(),
        "low"
    );

    // Test that None effort results in low reasoning_config for Gemini (none is treated as low)
    let request_none = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::None),
        ..Default::default()
    };

    let gemini_request_none = provider
        .convert_to_gemini_request(&request_none)
        .expect("conversion should succeed");

    // Check that thinkingConfig is present with low level when effort is None (for Gemini)
    let generation_config_none = gemini_request_none
        .generation_config
        .expect("generation_config should be present for None effort");
    let thinking_config_none = generation_config_none
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(
        thinking_config_none.thinking_level.as_deref().unwrap(),
        "low"
    );
}

#[test]
fn gemini31_pro_reasoning_mapping() {
    use crate::config::constants::models;
    use crate::config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test High effort level for Gemini 3.1 Pro
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::High),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    let generation_config = gemini_request
        .generation_config
        .expect("generation_config should be present");
    let thinking_config = generation_config
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(thinking_config.thinking_level.as_deref().unwrap(), "high");
}

#[test]
fn thought_signature_preserved_in_function_call_response() {
    let test_signature = "encrypted_signature_xyz123".to_string();

    let response = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: "model".to_string(),
                parts: vec![Part::FunctionCall {
                    function_call: GeminiFunctionCall {
                        name: "get_weather".to_string(),
                        args: json!({"city": "London"}),
                        id: Some("call_123".to_string()),
                    },
                    thought_signature: Some(test_signature.clone()),
                }],
            },
            finish_reason: Some("FUNCTION_CALL".to_string()),
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };

    let llm_response = GeminiProvider::convert_from_gemini_response(
        response,
        models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
    )
    .expect("conversion should succeed");

    let tool_calls = llm_response.tool_calls.expect("should have tool calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(
        tool_calls[0].thought_signature,
        Some(test_signature),
        "thought signature should be preserved"
    );
}

#[test]
fn thought_signature_roundtrip_in_request() {
    let provider = GeminiProvider::new("test-key".to_string());
    let test_signature = "sig_abc_def_123".to_string();

    let request = LLMRequest {
        messages: vec![
            Message::user("What's the weather?".to_string()),
            Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text(String::new()),
                reasoning: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_456".to_string(),
                    call_type: "function".to_string(),
                    function: Some(FunctionCall {
                        name: "get_weather".to_string(),
                        arguments: r#"{"city":"Paris"}"#.to_string(),
                    }),
                    text: None,
                    thought_signature: Some(test_signature.clone()),
                }]),
                tool_call_id: None,
                phase: None,
                origin_tool: None,
            },
        ],
        model: models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    // Find the FunctionCall part with thought signature
    let assistant_content = &gemini_request.contents[1];
    let has_signature = assistant_content.parts.iter().any(|part| match part {
        Part::FunctionCall {
            thought_signature, ..
        } => thought_signature.as_ref() == Some(&test_signature),
        _ => false,
    });

    assert!(
        has_signature,
        "thought signature should be preserved in request"
    );
}

#[test]
fn parallel_function_calls_single_signature() {
    let test_signature = "parallel_sig_123".to_string();

    let response = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: "model".to_string(),
                parts: vec![
                    Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "get_weather".to_string(),
                            args: json!({"city": "Paris"}),
                            id: Some("call_1".to_string()),
                        },
                        thought_signature: Some(test_signature.clone()),
                    },
                    Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "get_weather".to_string(),
                            args: json!({"city": "London"}),
                            id: Some("call_2".to_string()),
                        },
                        thought_signature: None, // Only first has signature
                    },
                ],
            },
            finish_reason: Some("FUNCTION_CALL".to_string()),
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };

    let llm_response = GeminiProvider::convert_from_gemini_response(
        response,
        models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
    )
    .expect("conversion should succeed");

    let tool_calls = llm_response.tool_calls.expect("should have tool calls");
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(
        tool_calls[0].thought_signature,
        Some(test_signature),
        "first call should have signature"
    );
    assert_eq!(
        tool_calls[1].thought_signature, None,
        "second call should not have signature"
    );
}

#[test]
fn thought_signature_propagation_from_text_to_function_call() {
    let test_signature = "text_reasoning_signature_789".to_string();

    // Scenario: Gemini 3 returns reasoning text with a signature, followed by a function call without one.
    // The signature from the text should be attached to the function call.
    let response = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: "model".to_string(),
                parts: vec![
                    Part::Text {
                        text: "I think I should check the weather.".to_string(),
                        thought_signature: Some(test_signature.clone()),
                    },
                    Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "get_weather".to_string(),
                            args: json!({"city": "Tokyo"}),
                            id: Some("call_tokyo".to_string()),
                        },
                        thought_signature: None, // Missing on function call itself
                    },
                ],
            },
            finish_reason: Some("FUNCTION_CALL".to_string()),
        }],
        prompt_feedback: None,
        usage_metadata: None,
    };

    let llm_response = GeminiProvider::convert_from_gemini_response(
        response,
        models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
    )
    .expect("conversion should succeed");

    let tool_calls = llm_response.tool_calls.expect("should have tool calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(
        tool_calls[0].thought_signature,
        Some(test_signature),
        "thought signature should be propagated from text part to function call"
    );
}

#[test]
fn gemini_provider_supports_reasoning_effort_for_gemini3() {
    use crate::config::constants::models;
    use crate::config::models::ModelId;
    use crate::config::models::Provider;

    // Test that the provider correctly identifies Gemini 3 Pro as supporting reasoning effort
    assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_1_PRO_PREVIEW));
    assert!(
        Provider::Gemini
            .supports_reasoning_effort(models::google::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS)
    );
    assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_1_PRO_PREVIEW));
    assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_FLASH_PREVIEW));

    // Test model IDs as well
    assert!(ModelId::Gemini31ProPreview.supports_reasoning_effort());
    assert!(ModelId::Gemini31ProPreviewCustomTools.supports_reasoning_effort());
    assert!(ModelId::Gemini3FlashPreview.supports_reasoning_effort());
}

#[test]
fn gemini3_flash_extended_thinking_levels() {
    use crate::config::constants::models;

    // Test that Gemini 3 Flash supports extended thinking levels
    assert!(GeminiProvider::supports_extended_thinking(
        models::google::GEMINI_3_FLASH_PREVIEW
    ));

    // But Gemini 3 Pro does not
    assert!(!GeminiProvider::supports_extended_thinking(
        models::google::GEMINI_3_1_PRO_PREVIEW
    ));
    assert!(!GeminiProvider::supports_extended_thinking(
        models::google::GEMINI_3_1_PRO_PREVIEW
    ));

    // Get supported levels for each model
    let flash_levels =
        GeminiProvider::supported_thinking_levels(models::google::GEMINI_3_FLASH_PREVIEW);
    assert_eq!(flash_levels, vec!["minimal", "low", "medium", "high"]);

    let pro31_levels =
        GeminiProvider::supported_thinking_levels(models::google::GEMINI_3_1_PRO_PREVIEW);
    assert_eq!(pro31_levels, vec!["low", "high"]);

    let pro_levels =
        GeminiProvider::supported_thinking_levels(models::google::GEMINI_3_1_PRO_PREVIEW);
    assert_eq!(pro_levels, vec!["low", "high"]);
}

#[test]
fn gemini_3_pro_temperature_warning_predicate_excludes_flash_models() {
    assert!(GeminiProvider::is_gemini_3_pro_model(
        models::google::GEMINI_3_1_PRO_PREVIEW
    ));
    assert!(GeminiProvider::is_gemini_3_pro_model(
        models::google::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS
    ));
    assert!(!GeminiProvider::is_gemini_3_pro_model(
        models::google::GEMINI_3_FLASH_PREVIEW
    ));
    assert!(!GeminiProvider::is_gemini_3_pro_model(
        models::google::GEMINI_3_1_FLASH_LITE_PREVIEW
    ));
}

#[test]
fn gemini3_flash_minimal_thinking_mapping() {
    use crate::config::constants::models;
    use crate::config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test Minimal thinking level for Gemini 3 Flash
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Minimal),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    let generation_config = gemini_request
        .generation_config
        .expect("generation_config should be present");
    let thinking_config = generation_config
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(
        thinking_config.thinking_level.as_deref().unwrap(),
        "minimal",
        "Gemini 3 Flash should support minimal thinking level"
    );
}

#[test]
fn gemini3_flash_medium_thinking_mapping() {
    use crate::config::constants::models;
    use crate::config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test Medium thinking level for Gemini 3 Flash
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Medium),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    let generation_config = gemini_request
        .generation_config
        .expect("generation_config should be present");
    let thinking_config = generation_config
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(
        thinking_config.thinking_level.as_deref().unwrap(),
        "medium",
        "Gemini 3 Flash should support medium thinking level"
    );
}

#[test]
fn gemini3_pro_medium_thinking_fallback() {
    use crate::config::constants::models;
    use crate::config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test Medium thinking level for Gemini 3 Pro (should fallback to high)
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Medium),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    let generation_config = gemini_request
        .generation_config
        .expect("generation_config should be present");
    let thinking_config = generation_config
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(
        thinking_config.thinking_level.as_deref().unwrap(),
        "high",
        "Gemini 3 Pro should fallback to high for medium reasoning effort"
    );
}

#[test]
fn convert_to_gemini_request_includes_advanced_parameters() {
    use crate::config::constants::models;

    let provider = GeminiProvider::new("test-key".to_string());

    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        top_p: Some(0.9),
        top_k: Some(40),
        presence_penalty: Some(0.6),
        frequency_penalty: Some(0.5),
        stop_sequences: Some(vec!["STOP".to_string()]),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    let config = gemini_request
        .generation_config
        .expect("generation_config should be present");

    assert_eq!(config.top_p, Some(0.9));
    assert_eq!(config.top_k, Some(40));
    assert_eq!(config.presence_penalty, Some(0.6));
    assert_eq!(config.frequency_penalty, Some(0.5));
    assert_eq!(
        config
            .stop_sequences
            .as_ref()
            .and_then(|s| s.first().cloned()),
        Some("STOP".to_string())
    );
}

#[test]
fn convert_to_gemini_request_includes_json_mode() {
    use crate::config::constants::models;

    let provider = GeminiProvider::new("test-key".to_string());

    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        output_format: Some(json!("json")),
        ..Default::default()
    };

    let gemini_request = provider
        .convert_to_gemini_request(&request)
        .expect("conversion should succeed");

    let config = gemini_request
        .generation_config
        .expect("generation_config should be present");

    assert_eq!(
        config.response_mime_type.as_deref(),
        Some("application/json")
    );
}
#[cfg(test)]
mod caching_tests {
    use super::*;
    use crate::config::core::{GeminiPromptCacheMode, PromptCachingConfig};

    #[test]
    fn test_gemini_prompt_cache_settings() {
        // Test 1: Defaults (Implicit mode)
        let _provider = GeminiProvider::new("test-key".to_string());
        // Default is explicit caching disabled, implicit is enabled by default in provider logic if config is default?
        // Let's check from_config
        let config = PromptCachingConfig::default();
        let provider = GeminiProvider::from_config(
            Some("key".into()),
            None,
            None,
            Some(config),
            None,
            None,
            None,
        );

        // Verification: we can't easily inspect private fields without a helper or reflection.
        // We can check if `convert_to_gemini_request` works.
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())],
            model: "gemini-1.5-pro".to_string(),
            ..Default::default()
        };
        let res = provider.convert_to_gemini_request(&request);
        assert!(res.is_ok());
    }

    #[test]
    fn test_gemini_explicit_mode_config() {
        let mut config = PromptCachingConfig {
            enabled: true,
            ..Default::default()
        };
        config.providers.gemini.enabled = true;
        config.providers.gemini.mode = GeminiPromptCacheMode::Explicit;
        config.providers.gemini.explicit_ttl_seconds = Some(1200);

        let provider = GeminiProvider::from_config(
            Some("key".into()),
            None,
            None,
            Some(config.clone()),
            None,
            None,
            None,
        );

        // Trigger request creation. It shouldn't panic or fail, even if explicit logic is placeholder.
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())],
            model: "gemini-1.5-pro".to_string(),
            ..Default::default()
        };
        let res = provider.convert_to_gemini_request(&request);
        assert!(res.is_ok(), "Request conversion should succeed");

        // Verify the request conversion produces correct structure with explicit TTL
        let gemini_req = res.expect("request conversion");

        assert!(
            !gemini_req.contents.is_empty(),
            "Contents should not be empty"
        );
        // Verify system instruction is set with TTL
        assert!(
            gemini_req.system_instruction.is_some(),
            "System instruction should be set"
        );
        // Verify TTL is included in system instruction when explicitly configured
        if let Some(ttl_seconds) = config.providers.gemini.explicit_ttl_seconds {
            let system_str =
                serde_json::to_string(&gemini_req.system_instruction).unwrap_or_default();
            assert!(
                system_str.contains(&ttl_seconds.to_string()),
                "Cache control or TTL should be configured when explicit_ttl_seconds is set"
            );
        }
    }
}

#[test]
fn part_json_deserialization_function_call_with_thought_signature() {
    // Test 1: FunctionCall with thoughtSignature (camelCase - native Gemini API)
    let json_camel = json!({
        "functionCall": {"name": "test_func", "args": {"key": "value"}},
        "thoughtSignature": "sig_camel_123"
    });
    let part: Part = serde_json::from_value(json_camel)
        .expect("should deserialize function call with camelCase thoughtSignature");
    match &part {
        Part::FunctionCall {
            function_call,
            thought_signature,
        } => {
            assert_eq!(function_call.name, "test_func");
            assert_eq!(
                thought_signature.as_deref(),
                Some("sig_camel_123"),
                "thoughtSignature (camelCase) should be captured"
            );
        }
        other => panic!("Expected FunctionCall, got {:?}", other),
    }

    // Test 2: FunctionCall WITHOUT thought signature
    let json_no_sig = json!({
        "functionCall": {"name": "test_func", "args": {"key": "value"}}
    });
    let part2: Part = serde_json::from_value(json_no_sig)
        .expect("should deserialize function call without signature");
    match &part2 {
        Part::FunctionCall {
            function_call,
            thought_signature,
        } => {
            assert_eq!(function_call.name, "test_func");
            assert_eq!(thought_signature, &None, "missing signature should be None");
        }
        other => panic!("Expected FunctionCall, got {:?}", other),
    }

    // Test 3: Text part
    let json_text = json!({"text": "hello world"});
    let part3: Part = serde_json::from_value(json_text).expect("should deserialize text part");
    match &part3 {
        Part::Text { text, .. } => {
            assert_eq!(text, "hello world");
        }
        other => panic!("Expected Text, got {:?}", other),
    }

    // Test 4: Full candidate with function call + thought signature (simulates API response)
    let candidate_json = json!({
        "content": {
            "role": "model",
            "parts": [{
                "functionCall": {"name": "run_pty_cmd", "args": {"command": "cargo check"}},
                "thoughtSignature": "api_signature_abc"
            }]
        },
        "finishReason": "FUNCTION_CALL"
    });
    let candidate: StreamingCandidate =
        serde_json::from_value(candidate_json).expect("should deserialize streaming candidate");
    assert_eq!(candidate.content.parts.len(), 1);
    match &candidate.content.parts[0] {
        Part::FunctionCall {
            function_call,
            thought_signature,
        } => {
            assert_eq!(function_call.name, "run_pty_cmd");
            assert_eq!(
                thought_signature.as_deref(),
                Some("api_signature_abc"),
                "thought signature should be preserved from API response"
            );
        }
        other => panic!("Expected FunctionCall in candidate, got {:?}", other),
    }
}
