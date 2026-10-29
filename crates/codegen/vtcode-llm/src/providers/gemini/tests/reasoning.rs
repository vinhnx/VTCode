use super::*;

#[test]
fn convert_to_gemini_request_includes_reasoning_config() {
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test High effort level for Gemini 3 Pro
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::High),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    // Check that thinkingConfig is present in generationConfig and has the correct value for High effort
    let generation_config = gemini_request.generation_config.expect("generation_config should be present");
    let thinking_config = generation_config
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(thinking_config.thinking_level.as_deref().unwrap(), "high");

    // Test Low effort level for Gemini 3 Pro
    let request_low = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
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
    assert_eq!(thinking_config_low.thinking_level.as_deref().unwrap(), "low");

    // Test that None effort results in low reasoning_config for Gemini (none is treated as low)
    let request_none = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
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
    assert_eq!(thinking_config_none.thinking_level.as_deref().unwrap(), "low");
}

#[test]
fn gemini31_pro_reasoning_mapping() {
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test High effort level for Gemini 3.1 Pro
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::High),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let generation_config = gemini_request.generation_config.expect("generation_config should be present");
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

    let llm_response =
        GeminiProvider::convert_from_gemini_response(response, models::google::GEMINI_3_8_FLASH.to_string())
            .expect("conversion should succeed");

    let tool_calls = llm_response.tool_calls.expect("should have tool calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].thought_signature, Some(test_signature), "thought signature should be preserved");
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
                        namespace: None,
                        name: "get_weather".to_string(),
                        arguments: r#"{"city":"Paris"}"#.to_string(),
                    }),
                    text: None,
                    thought_signature: Some(test_signature.clone()),
                }]),
                tool_call_id: None,
                phase: None,
                origin_tool: None,
                metadata: None,
                clear_at: None,
            },
        ]
        .into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    // Find the FunctionCall part with thought signature
    let assistant_content = &gemini_request.contents[1];
    let has_signature = assistant_content.parts.iter().any(|part| match part {
        Part::FunctionCall { thought_signature, .. } => thought_signature.as_ref() == Some(&test_signature),
        _ => false,
    });

    assert!(has_signature, "thought signature should be preserved in request");
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

    let llm_response =
        GeminiProvider::convert_from_gemini_response(response, models::google::GEMINI_3_8_FLASH.to_string())
            .expect("conversion should succeed");

    let tool_calls = llm_response.tool_calls.expect("should have tool calls");
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0].thought_signature, Some(test_signature), "first call should have signature");
    assert_eq!(tool_calls[1].thought_signature, None, "second call should not have signature");
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

    let llm_response =
        GeminiProvider::convert_from_gemini_response(response, models::google::GEMINI_3_FLASH_PREVIEW.to_string())
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
    use vtcode_config::constants::models;
    use vtcode_config::models::ModelId;
    use vtcode_config::models::Provider;

    // Test that the provider correctly identifies Gemini 3 Pro as supporting reasoning effort
    assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_8_FLASH));
    assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_8_FLASH));
    assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_8_FLASH));
    assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_FLASH_PREVIEW));

    // Test model IDs as well
    assert!(ModelId::Gemini38Flash.supports_reasoning_effort());
    assert!(ModelId::Gemini38Flash.supports_reasoning_effort());
    assert!(ModelId::Gemini38Flash.supports_reasoning_effort());
}

#[test]
fn gemini3_flash_extended_thinking_levels() {
    use vtcode_config::constants::models;

    // Test that Gemini 3 Flash supports extended thinking levels
    assert!(GeminiProvider::supports_extended_thinking(models::google::GEMINI_3_FLASH_PREVIEW));

    // But Gemini 3 Pro does not
    assert!(!GeminiProvider::supports_extended_thinking(models::google::GEMINI_3_8_FLASH));
    assert!(!GeminiProvider::supports_extended_thinking("gemini-3-pro"));

    // Get supported levels for each model
    let flash_levels = GeminiProvider::supported_thinking_levels(models::google::GEMINI_3_FLASH_PREVIEW);
    assert_eq!(flash_levels, vec!["minimal", "low", "medium", "high"]);

    // Pro predicates are string-based by design (they cover future Pro
    // releases with no catalog entry yet), so literals carry the intent here.
    let pro31_levels = GeminiProvider::supported_thinking_levels("gemini-3.1-pro");
    assert_eq!(pro31_levels, vec!["low", "high"]);

    let pro_levels = GeminiProvider::supported_thinking_levels("gemini-3-pro");
    assert_eq!(pro_levels, vec!["low", "high"]);

    // Gemini 4 Argon supports low/medium/high (medium default), no minimal
    let argon_levels = GeminiProvider::supported_thinking_levels(models::google::GEMINI_4_ARGON);
    assert_eq!(argon_levels, vec!["low", "medium", "high"]);
}

#[test]
fn gemini4_argon_medium_thinking_mapping() {
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Argon natively supports medium: it must not fall back to high
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_4_ARGON.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Medium),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let generation_config = gemini_request.generation_config.expect("generation_config should be present");
    let thinking_config = generation_config
        .thinking_config
        .as_ref()
        .expect("thinking_config should be present");
    assert_eq!(
        thinking_config.thinking_level.as_deref().unwrap(),
        "medium",
        "Gemini 4 Argon should map medium reasoning effort to medium thinking level"
    );
}

#[test]
fn gemini4_argon_output_limit_validation() {
    use vtcode_config::constants::models;

    let provider = GeminiProvider::new("test-key".to_string());

    // 64K sync output limit applies to Gemini 4 models, not the 8K legacy default
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_4_ARGON.to_string(),
        max_tokens: Some(65_536),
        ..Default::default()
    };
    assert!(provider.validate_request(&request).is_ok(), "Gemini 4 Argon should accept max_tokens up to 65536");

    let over_limit = LLMRequest { max_tokens: Some(65_537), ..request };
    assert!(
        provider.validate_request(&over_limit).is_err(),
        "Gemini 4 Argon should reject max_tokens above the 65536 sync limit"
    );
}

#[test]
fn gemini_3_pro_temperature_warning_predicate_excludes_flash_models() {
    // Pro predicate is string-based by design (covers future Pro releases
    // with no catalog entry yet), so literals carry the intent here.
    assert!(GeminiProvider::is_gemini_3_pro_model("gemini-3.1-pro"));
    assert!(GeminiProvider::is_gemini_3_pro_model("gemini-3-pro"));
    assert!(!GeminiProvider::is_gemini_3_pro_model(models::google::GEMINI_3_FLASH_PREVIEW));
    assert!(!GeminiProvider::is_gemini_3_pro_model(models::google::GEMINI_3_8_FLASH));
}

#[test]
fn gemini3_flash_minimal_thinking_mapping() {
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test Minimal thinking level for Gemini 3 Flash
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Minimal),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let generation_config = gemini_request.generation_config.expect("generation_config should be present");
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
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    let provider = GeminiProvider::new("test-key".to_string());

    // Test Medium thinking level for Gemini 3 Flash
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Medium),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let generation_config = gemini_request.generation_config.expect("generation_config should be present");
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
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    let mut model_behavior = vtcode_config::core::ModelConfig::default();
    model_behavior.model_supports_reasoning_effort = Some(true);
    let provider = GeminiProvider::from_config(
        Some("test-key".to_string()),
        Some("gemini-3-pro".to_string()),
        None,
        None,
        None,
        None,
        Some(model_behavior),
    );

    // Test Medium thinking level for Gemini 3 Pro (should fallback to high)
    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: "gemini-3-pro".to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Medium),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let generation_config = gemini_request.generation_config.expect("generation_config should be present");
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
