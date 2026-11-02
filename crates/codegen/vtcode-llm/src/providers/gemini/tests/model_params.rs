use super::*;

#[test]
fn convert_to_gemini_request_includes_advanced_parameters() {
    use vtcode_config::constants::models;

    let provider = GeminiProvider::new("test-key".to_string());

    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        top_p: Some(0.9),
        top_k: Some(40),
        presence_penalty: Some(0.6),
        frequency_penalty: Some(0.5),
        stop_sequences: Some(vec!["STOP".to_string()]),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let config = gemini_request.generation_config.expect("generation_config should be present");

    assert_eq!(config.top_p, Some(0.9));
    assert_eq!(config.top_k, Some(40));
    assert_eq!(config.presence_penalty, Some(0.6));
    assert_eq!(config.frequency_penalty, Some(0.5));
    assert_eq!(config.stop_sequences.as_ref().and_then(|s| s.first().cloned()), Some("STOP".to_string()));
}

#[test]
fn convert_to_gemini_request_strips_sampling_params_for_latest_models() {
    let provider = GeminiProvider::new("test-key".to_string());

    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        presence_penalty: Some(0.6),
        frequency_penalty: Some(0.5),
        stop_sequences: Some(vec!["STOP".to_string()]),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");
    let config = gemini_request.generation_config.expect("generation_config should be present");

    assert_eq!(config.temperature, None, "temperature should be stripped for gemini-3.6-flash");
    assert_eq!(config.top_p, None, "top_p should be stripped for gemini-3.6-flash");
    assert_eq!(config.top_k, None, "top_k should be stripped for gemini-3.6-flash");
    assert_eq!(config.presence_penalty, Some(0.6), "presence_penalty should still be sent");
    assert_eq!(config.frequency_penalty, Some(0.5), "frequency_penalty should still be sent");
    assert_eq!(
        config.stop_sequences.as_ref().and_then(|s| s.first().cloned()),
        Some("STOP".to_string()),
        "stop_sequences should still be sent"
    );
}

#[test]
fn convert_to_gemini_request_strips_sampling_params_for_3_5_flash_lite() {
    let provider = GeminiProvider::new("test-key".to_string());

    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
        temperature: Some(0.5),
        top_p: Some(0.8),
        top_k: Some(30),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");
    let config = gemini_request.generation_config.expect("generation_config should be present");

    assert_eq!(config.temperature, None, "temperature should be stripped for gemini-3.5-flash-lite");
    assert_eq!(config.top_p, None, "top_p should be stripped for gemini-3.5-flash-lite");
    assert_eq!(config.top_k, None, "top_k should be stripped for gemini-3.5-flash-lite");
}

#[test]
fn latest_model_strips_prefilled_model_turn() {
    let provider = GeminiProvider::new("test-key".to_string());

    // Last message is an assistant (model) turn — should be stripped for latest models
    let request = LLMRequest {
        messages: vec![
            Message::user("hello".to_string()),
            Message::assistant("prefilled response".to_string()),
        ]
        .into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    assert_eq!(gemini_request.contents.len(), 1, "should strip the prefilled model turn, leaving only user message");
    assert_eq!(gemini_request.contents[0].role, "user");
}

#[test]
fn prefilled_turn_preserved_for_older_models() {
    let provider = GeminiProvider::new("test-key".to_string());

    // Last message is an assistant (model) turn — should be preserved for older models
    let request = LLMRequest {
        messages: vec![
            Message::user("hello".to_string()),
            Message::assistant("prefilled response".to_string()),
        ]
        .into(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    assert_eq!(gemini_request.contents.len(), 2, "should preserve the prefilled model turn for older models");
    assert_eq!(gemini_request.contents[1].role, "model");
}

#[test]
fn convert_to_gemini_request_includes_json_mode() {
    use vtcode_config::constants::models;

    let provider = GeminiProvider::new("test-key".to_string());

    let request = LLMRequest {
        messages: vec![Message::user("test".to_string())].into(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        output_format: Some(json!("json")),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let config = gemini_request.generation_config.expect("generation_config should be present");

    assert_eq!(config.response_mime_type.as_deref(), Some("application/json"));
}

#[test]
fn convert_to_gemini_request_combines_google_search_with_function_tools() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![Message::user("Search and then inspect weather".to_string())].into(),
        tools: Some(Arc::new(vec![
            ToolDefinition::web_search(json!({})),
            ToolDefinition::function(
                "get_weather".to_string(),
                "Get the weather for a city".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }),
            ),
        ])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let tools = gemini_request.tools.expect("tools should be present");
    assert!(
        tools.iter().any(|tool| tool.google_search.is_some()),
        "google_search built-in tool should be preserved"
    );
    assert!(
        tools.iter().any(|tool| {
            tool.function_declarations
                .as_ref()
                .is_some_and(|declarations| declarations.iter().any(|decl| decl.name == "get_weather"))
        }),
        "function declarations should be preserved alongside built-in tools"
    );

    let tool_config = gemini_request.tool_config.expect("tool config should be present");
    assert_eq!(tool_config.include_server_side_tool_invocations, Some(true));
    assert_eq!(tool_config.function_calling_config.as_ref().map(|config| config.mode.as_str()), Some("VALIDATED"));
}

#[test]
fn convert_from_gemini_response_preserves_server_side_tool_parts() {
    let response = GenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: "model".to_string(),
                parts: vec![
                    Part::ToolCall {
                        tool_call: ServerToolCall {
                            tool_type: "GOOGLE_SEARCH_WEB".to_string(),
                            args: Some(json!({
                                "queries": ["northernmost city in the United States"]
                            })),
                            id: Some("search_1".to_string()),
                        },
                        thought_signature: Some("tool_sig".to_string()),
                    },
                    Part::ToolResponse {
                        tool_response: ServerToolResponse {
                            tool_type: "GOOGLE_SEARCH_WEB".to_string(),
                            response: json!({
                                "search_suggestions": ["Utqiaġvik, Alaska"]
                            }),
                            id: Some("search_1".to_string()),
                        },
                        thought_signature: Some("tool_sig".to_string()),
                    },
                    Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "get_weather".to_string(),
                            args: json!({ "city": "Utqiaġvik, Alaska" }),
                            id: Some("call_weather".to_string()),
                        },
                        thought_signature: Some("function_sig".to_string()),
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

    assert_eq!(
        llm_response.tool_calls.as_ref().expect("tool calls")[0]
            .function
            .as_ref()
            .expect("function")
            .name,
        "get_weather"
    );

    let preserved = llm_response.reasoning_details.as_ref().expect("raw parts should be preserved");
    assert_eq!(preserved.len(), 1);
    assert!(
        preserved[0].starts_with("__vtcode_gemini_parts__:"),
        "raw parts should be serialized with the Gemini preservation prefix"
    );
}
