use super::*;

#[test]
fn convert_to_gemini_request_maps_history_and_system_prompt() {
    let provider = GeminiProvider::new("test-key".to_string());
    let mut assistant_message = Message::assistant("Sure thing".to_string());
    assistant_message.tool_calls = Some(vec![ToolCall::function(
        "call_1".to_string(),
        tools::LIST_FILES.to_string(),
        json!({ "path": "." }).to_string(),
    )]);

    let tool_response = Message::tool_response("call_1".to_string(), json!({ "result": "ok" }).to_string());

    let tool_def = ToolDefinition::function(
        tools::LIST_FILES.to_string(),
        "List files".to_string(),
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        }),
    );

    let request = LLMRequest {
        messages: vec![Message::user("hello".to_string()), assistant_message, tool_response].into(),
        system_prompt: Some(Arc::from("System prompt")),
        tools: Some(Arc::new(vec![tool_def])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        max_tokens: Some(256),
        temperature: Some(0.4),
        tool_choice: Some(ToolChoice::Specific(SpecificToolChoice {
            tool_type: "function".to_string(),
            function: SpecificFunctionChoice { name: tools::LIST_FILES.to_string() },
        })),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let system_instruction = gemini_request.system_instruction.expect("system instruction should be present");
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
            Part::FunctionResponse { function_response, .. } => Some(function_response),
            _ => None,
        })
        .expect("tool response part should exist");
    assert_eq!(tool_part.name, tools::LIST_FILES);
}

#[test]
fn convert_to_gemini_request_hoists_history_system_directives_into_system_instruction() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![
            Message::system("Reuse the latest tool outputs before reading again.".to_string()),
            Message::user("explore architecture".to_string()),
        ]
        .into(),
        system_prompt: Some(Arc::from("Stable system instructions")),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let system_instruction = gemini_request.system_instruction.expect("system instruction should be present");
    let text = match system_instruction.parts.as_slice() {
        [Part::Text { text, thought_signature: _ }] => text,
        parts => panic!("expected single text system instruction part, got {parts:?}"),
    };

    assert!(text.contains("Stable system instructions"));
    assert!(text.contains("[History Directives]"));
    assert!(text.contains("- Reuse the latest tool outputs before reading again."));
    assert_eq!(gemini_request.contents.len(), 1);
    assert_eq!(gemini_request.contents[0].role, "user");
}

#[test]
fn convert_to_gemini_request_promotes_history_system_directives_without_base_system_prompt() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![
            Message::system("Summarize the latest tool outputs instead of rereading.".to_string()),
            Message::user("explore architecture".to_string()),
        ]
        .into(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    let system_instruction = gemini_request
        .system_instruction
        .expect("history directives should promote a system instruction");
    let text = match system_instruction.parts.as_slice() {
        [Part::Text { text, thought_signature: _ }] => text,
        parts => panic!("expected single text system instruction part, got {parts:?}"),
    };

    assert!(text.contains("[History Directives]"));
    assert!(text.contains("- Summarize the latest tool outputs instead of rereading."));
    assert_eq!(gemini_request.contents.len(), 1);
    assert_eq!(gemini_request.contents[0].role, "user");
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
                            name: tools::LIST_FILES.to_string(),
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

    let llm_response =
        GeminiProvider::convert_from_gemini_response(response, models::google::GEMINI_3_FLASH_PREVIEW.to_string())
            .expect("conversion should succeed");

    assert_eq!(llm_response.content.as_deref(), Some("Here you go"));
    let calls = llm_response.tool_calls.expect("tool call should be present");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].function.as_ref().unwrap().name, tools::LIST_FILES);
    assert!(calls[0].function.as_ref().unwrap().arguments.contains("path"));
    assert_eq!(llm_response.finish_reason, FinishReason::ToolCalls);
}

#[test]
fn convert_to_gemini_request_keeps_apply_patch_as_function_tool() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![Message::user("patch this file".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition::apply_patch("Apply VT Code patches".to_string())])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");
    let tools = gemini_request.tools.expect("tools should be present");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_declarations.as_ref().expect("function declarations").len(), 1);
    assert_eq!(tools[0].function_declarations.as_ref().expect("function declarations")[0].name, "apply_patch");
}

#[test]
fn tool_free_requests_disable_hosted_and_function_tools_in_both_protocols() {
    let provider = GeminiProvider::new("test-key".to_owned());
    for hosted in [false, true] {
        for stream in [false, true] {
            let mut tools = vec![ToolDefinition::function(
                "lookup".to_owned(),
                "Look up data".to_owned(),
                json!({"type": "object"}),
            )];
            if hosted {
                tools.push(ToolDefinition::web_search(json!({})));
            }
            let mut request = LLMRequest {
                model: "gemini-3-flash-preview".to_owned(),
                messages: vec![Message::user("Synthesize saved evidence".to_owned())].into(),
                tools: Some(Arc::new(tools)),
                tool_choice: Some(ToolChoice::None),
                stream,
                ..Default::default()
            };
            let generate =
                serde_json::to_value(provider.convert_to_gemini_request(&request).expect("generate payload")).unwrap();
            let interaction =
                serde_json::to_value(provider.convert_to_interaction_request(&request).expect("interaction payload"))
                    .unwrap();
            if hosted {
                assert!(generate.get("tools").is_none());
                assert!(interaction.get("tools").is_none());
            } else {
                assert_eq!(generate["toolConfig"]["functionCallingConfig"]["mode"], "NONE");
                assert_eq!(interaction["tool_choice"]["mode"], "none");
                assert!(generate["tools"].is_array());
                assert!(interaction["tools"].is_array());
            }
            request.tool_choice = Some(ToolChoice::Auto);
            let enabled = provider.convert_to_gemini_request(&request).expect("enabled payload");
            assert!(enabled.tools.is_some());
            let enabled = provider
                .convert_to_interaction_request(&request)
                .expect("enabled interaction payload");
            assert!(enabled.tools.is_some());
        }
    }
}

#[test]
fn convert_to_interaction_request_serializes_built_in_and_function_tools() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![Message::user(
            "What is the northernmost city in the United States?".to_string(),
        )]
        .into(),
        tools: Some(Arc::new(vec![
            ToolDefinition::web_search(json!({})),
            ToolDefinition::function(
                "get_weather".to_string(),
                "Gets the weather".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "location": { "type": "string" }
                    },
                    "required": ["location"]
                }),
            ),
        ])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let interaction_request = provider
        .convert_to_interaction_request(&request)
        .expect("interaction request should build");

    assert!(matches!(
        interaction_request.input,
        InteractionInput::Text(ref text)
            if text == "What is the northernmost city in the United States?"
    ));

    let tools = interaction_request.tools.expect("tools should be present");
    assert!(tools.iter().any(|tool| tool.tool_type == "google_search"));
    assert!(
        tools
            .iter()
            .any(|tool| { tool.tool_type == "function" && tool.name.as_deref() == Some("get_weather") })
    );
    assert_eq!(interaction_request.tool_choice.expect("tool choice").mode, "validated");
}

#[test]
fn convert_to_interaction_request_preserves_built_in_tool_config() {
    let provider = GeminiProvider::new("test-key".to_string());
    let request = LLMRequest {
        messages: vec![Message::user("Search for an image".to_string())].into(),
        tools: Some(Arc::new(vec![
            ToolDefinition::web_search(json!({
                "search_types": ["web_search", "image_search"]
            })),
            ToolDefinition::file_search(json!({
                "file_search_store_names": ["fileSearchStores/my-store-name"]
            })),
        ])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let interaction_request = provider
        .convert_to_interaction_request(&request)
        .expect("interaction request should build");
    let tools = interaction_request.tools.expect("tools should be present");

    let google_search = tools
        .iter()
        .find(|tool| tool.tool_type == "google_search")
        .expect("google_search tool");
    assert_eq!(google_search.extra.get("search_types"), Some(&json!(["web_search", "image_search"])));

    let file_search = tools
        .iter()
        .find(|tool| tool.tool_type == "file_search")
        .expect("file_search tool");
    assert_eq!(file_search.extra.get("file_search_store_names"), Some(&json!(["fileSearchStores/my-store-name"])));
}

#[test]
fn convert_to_interaction_request_uses_function_result_for_chained_turns() {
    let provider = GeminiProvider::new("test-key".to_string());
    let mut assistant_message = Message::assistant(String::new());
    assistant_message.tool_calls = Some(vec![ToolCall::function(
        "call_weather_1".to_string(),
        "get_weather".to_string(),
        json!({ "location": "Utqiagvik, Alaska" }).to_string(),
    )]);

    let request = LLMRequest {
        messages: vec![
            Message::user("Check the weather".to_string()),
            assistant_message,
            Message::tool_response(
                "call_weather_1".to_string(),
                json!({ "response": "Very cold. 22 degrees Fahrenheit." }).to_string(),
            ),
        ]
        .into(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Gets the weather".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                }
            }),
        )])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        previous_response_id: Some("interaction_123".to_string()),
        ..Default::default()
    };

    let interaction_request = provider
        .convert_to_interaction_request(&request)
        .expect("interaction request should build");

    assert_eq!(interaction_request.previous_interaction_id.as_deref(), Some("interaction_123"));

    match interaction_request.input {
        InteractionInput::Content(content) => match content.as_slice() {
            [InteractionContent::FunctionResult { call_id, name, result, .. }] => {
                assert_eq!(call_id, "call_weather_1");
                assert_eq!(name.as_deref(), Some("get_weather"));
                assert_eq!(
                    result,
                    &InteractionResult::Json(json!({ "response": "Very cold. 22 degrees Fahrenheit." }))
                );
            }
            other => panic!("expected single function_result content, got {other:?}"),
        },
        other => panic!("expected content delta input, got {other:?}"),
    }
}

#[test]
fn convert_to_interaction_request_preserves_trailing_function_call_turn() {
    let provider = GeminiProvider::new("test-key".to_string());
    let test_signature = "sig_interaction_789".to_string();
    let mut assistant_message = Message::assistant(String::new());
    assistant_message.tool_calls = Some(vec![ToolCall {
        id: "call_789".to_string(),
        call_type: "function".to_string(),
        function: Some(FunctionCall {
            namespace: None,
            name: "get_weather".to_string(),
            arguments: r#"{"city":"Paris"}"#.to_string(),
        }),
        text: None,
        thought_signature: Some(test_signature.clone()),
    }]);

    let request = LLMRequest {
        messages: vec![Message::user("What's the weather?".to_string()), assistant_message].into(),
        model: models::google::GEMINI_3_8_FLASH.to_string(),
        ..Default::default()
    };

    let interaction_request = provider
        .convert_to_interaction_request(&request)
        .expect("interaction request should build");

    match interaction_request.input {
        InteractionInput::Turns(turns) => {
            assert_eq!(turns.len(), 2, "trailing function-call turn must survive the prefill guard");
            assert_eq!(turns[1].role, "model");
            match &turns[1].content {
                InteractionTurnContent::Content(parts) => assert!(
                    parts.iter().any(
                        |part| matches!(part, InteractionContent::FunctionCall { signature, .. } if signature.as_ref() == Some(&test_signature))
                    ),
                    "thought signature should be preserved in request"
                ),
                other => panic!("expected content turn, got {other:?}"),
            }
        }
        other => panic!("expected turns input, got {other:?}"),
    }
}

#[test]
fn convert_to_interaction_request_supports_multimodal_function_results() {
    let provider = GeminiProvider::new("test-key".to_string());
    let mut assistant_message = Message::assistant(String::new());
    assistant_message.tool_calls = Some(vec![ToolCall::function(
        "call_screenshot_1".to_string(),
        "take_screenshot".to_string(),
        json!({ "url": "https://google.com" }).to_string(),
    )]);

    let request = LLMRequest {
        messages: vec![
            Message::user("Take a screenshot".to_string()),
            assistant_message,
            Message {
                role: MessageRole::Tool,
                content: MessageContent::parts(vec![
                    crate::provider::ContentPart::Text {
                        text: "Screenshot captured successfully.".to_string(),
                    },
                    crate::provider::ContentPart::Image {
                        data: "ZmFrZQ==".to_string(),
                        mime_type: "image/png".to_string(),
                        content_type: "image".to_string(),
                        detail: None,
                        image_url: None,
                    },
                ]),
                tool_call_id: Some("call_screenshot_1".to_string()),
                ..Default::default()
            },
        ]
        .into(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "take_screenshot".to_string(),
            "Takes a screenshot".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
        )])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        previous_response_id: Some("interaction_123".to_string()),
        ..Default::default()
    };

    let interaction_request = provider
        .convert_to_interaction_request(&request)
        .expect("interaction request should build");

    match interaction_request.input {
        InteractionInput::Content(content) => match content.as_slice() {
            [InteractionContent::FunctionResult { result, .. }] => match result {
                InteractionResult::Content(items) => {
                    assert!(matches!(
                        items.as_slice(),
                        [
                            InteractionContent::Text { text },
                            InteractionContent::Image { mime_type, data }
                        ] if text == "Screenshot captured successfully."
                            && mime_type == "image/png"
                            && data == "ZmFrZQ=="
                    ));
                }
                other => panic!("expected multimodal content result, got {other:?}"),
            },
            other => panic!("expected single function_result content, got {other:?}"),
        },
        other => panic!("expected content delta input, got {other:?}"),
    }
}

#[test]
fn convert_from_interaction_response_extracts_request_id_and_tool_calls() {
    let response = Interaction {
        id: "interaction_456".to_string(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        status: Some("requires_action".to_string()),
        outputs: vec![
            InteractionOutput {
                output_type: "text".to_string(),
                text: Some("Looking it up.".to_string()),
                summary: None,
                id: None,
                name: None,
                arguments: None,
                signature: None,
                function_call: None,
            },
            InteractionOutput {
                output_type: "function_call".to_string(),
                text: None,
                summary: None,
                id: Some("call_weather_2".to_string()),
                name: Some("get_weather".to_string()),
                arguments: Some(json!({ "location": "Utqiagvik, Alaska" })),
                signature: Some("sig_123".to_string()),
                function_call: None,
            },
        ],
        usage: None,
    };

    let llm_response =
        GeminiProvider::convert_from_interaction_response(response, models::google::GEMINI_3_FLASH_PREVIEW.to_string())
            .expect("interaction response should parse");

    assert_eq!(llm_response.request_id.as_deref(), Some("interaction_456"));
    assert_eq!(llm_response.content.as_deref(), Some("Looking it up."));
    assert_eq!(llm_response.finish_reason, FinishReason::ToolCalls);
    let tool_calls = llm_response.tool_calls.expect("tool call should exist");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].function.as_ref().expect("function").name, "get_weather");
    assert_eq!(tool_calls[0].thought_signature.as_deref(), Some("sig_123"));
}

#[test]
fn convert_from_interaction_response_prefers_thought_summaries() {
    let response = Interaction {
        id: "interaction_789".to_string(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        status: Some("completed".to_string()),
        outputs: vec![
            InteractionOutput {
                output_type: "thought".to_string(),
                text: None,
                summary: Some("Checked the available evidence.".to_string()),
                id: None,
                name: None,
                arguments: None,
                signature: Some("thought_sig_1".to_string()),
                function_call: None,
            },
            InteractionOutput {
                output_type: "text".to_string(),
                text: Some("Final answer.".to_string()),
                summary: None,
                id: None,
                name: None,
                arguments: None,
                signature: None,
                function_call: None,
            },
        ],
        usage: None,
    };

    let llm_response =
        GeminiProvider::convert_from_interaction_response(response, models::google::GEMINI_3_FLASH_PREVIEW.to_string())
            .expect("interaction response should parse");

    assert_eq!(llm_response.reasoning.as_deref(), Some("Checked the available evidence."));
    assert_eq!(llm_response.content.as_deref(), Some("Final answer."));
    assert_eq!(llm_response.request_id.as_deref(), Some("interaction_789"));
    assert_eq!(llm_response.reasoning_details.as_ref().map(Vec::len), Some(1));
}

#[test]
fn convert_from_interaction_response_hides_raw_thought_text() {
    let response = Interaction {
        id: "interaction_raw_thought".to_string(),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        status: Some("completed".to_string()),
        outputs: vec![InteractionOutput {
            output_type: "thought".to_string(),
            text: Some("Private chain of thought.".to_string()),
            summary: None,
            id: None,
            name: None,
            arguments: None,
            signature: Some("thought_sig_raw".to_string()),
            function_call: None,
        }],
        usage: None,
    };

    let llm_response =
        GeminiProvider::convert_from_interaction_response(response, models::google::GEMINI_3_FLASH_PREVIEW.to_string())
            .expect("interaction response should parse");

    assert!(llm_response.reasoning.is_none());
    assert!(
        llm_response
            .reasoning_details
            .as_ref()
            .is_some_and(|details| details.iter().any(|detail| detail.contains("Private chain of thought.")))
    );
}
