use super::helpers::InteractionStreamState;
use super::wire::{
    Candidate, Content, FunctionCall as GeminiFunctionCall, GenerateContentResponse, Interaction, InteractionContent,
    InteractionInput, InteractionOutput, InteractionResult, InteractionTurnContent, Part, ServerToolCall,
    ServerToolResponse,
};
use super::*;
use crate::provider::{MessageContent, MessageRole, SpecificFunctionChoice, SpecificToolChoice, ToolDefinition};
use serde_json::json;
use vtcode_config::constants::models;
use vtcode_config::constants::tools;
use vtcode_config::models::ProviderModelSupport;

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

#[test]
fn convert_to_gemini_request_replays_preserved_raw_parts() {
    let provider = GeminiProvider::new("test-key".to_string());
    let preserved_parts = vec![
        Part::ToolCall {
            tool_call: ServerToolCall {
                tool_type: "GOOGLE_SEARCH_WEB".to_string(),
                args: Some(json!({ "queries": ["northernmost city in the United States"] })),
                id: Some("search_1".to_string()),
            },
            thought_signature: Some("tool_sig".to_string()),
        },
        Part::ToolResponse {
            tool_response: ServerToolResponse {
                tool_type: "GOOGLE_SEARCH_WEB".to_string(),
                response: json!({ "search_suggestions": ["Utqiaġvik, Alaska"] }),
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
    ];

    let assistant_message = Message::assistant_with_tools(
        String::new(),
        vec![ToolCall::function(
            "call_weather".to_string(),
            "get_weather".to_string(),
            json!({ "city": "Utqiaġvik, Alaska" }).to_string(),
        )],
    )
    .with_reasoning_details(Some(vec![json!(format!(
        "__vtcode_gemini_parts__:{}",
        serde_json::to_string(&preserved_parts).expect("serialize preserved parts")
    ))]));

    let request = LLMRequest {
        messages: vec![
            Message::user("Find the northernmost city and weather".to_string()),
            assistant_message,
            Message::tool_response(
                "call_weather".to_string(),
                json!({ "response": "Very cold. 22 degrees Fahrenheit." }).to_string(),
            ),
        ]
        .into(),
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

    assert_eq!(gemini_request.contents.len(), 3);
    assert_eq!(gemini_request.contents[1].parts.len(), preserved_parts.len());
    assert!(matches!(
        &gemini_request.contents[1].parts[0],
        Part::ToolCall {
            tool_call,
            thought_signature
        } if tool_call.tool_type == "GOOGLE_SEARCH_WEB"
            && tool_call.id.as_deref() == Some("search_1")
            && thought_signature.as_deref() == Some("tool_sig")
    ));
    assert!(matches!(
        &gemini_request.contents[1].parts[2],
        Part::FunctionCall {
            function_call,
            thought_signature
        } if function_call.name == "get_weather"
            && function_call.id.as_deref() == Some("call_weather")
            && thought_signature.as_deref() == Some("function_sig")
    ));
}
#[cfg(test)]
mod caching_tests {
    use super::*;
    use vtcode_config::core::{GeminiPromptCacheMode, PromptCachingConfig};

    #[test]
    fn test_gemini_prompt_cache_settings() {
        // Test 1: Defaults (Implicit mode)
        let _provider = GeminiProvider::new("test-key".to_string());
        // Default is explicit caching disabled, implicit is enabled by default in provider logic if config is default?
        // Let's check from_config
        let config = PromptCachingConfig::default();
        let provider = GeminiProvider::from_config(Some("key".into()), None, None, Some(config), None, None, None);

        // Verification: we can't easily inspect private fields without a helper or reflection.
        // We can check if `convert_to_gemini_request` works.
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())].into(),
            model: "gemini-1.5-pro".to_string(),
            ..Default::default()
        };
        let res = provider.convert_to_gemini_request(&request);
        res.unwrap();
    }

    #[test]
    fn test_gemini_explicit_mode_config() {
        let mut config = PromptCachingConfig { enabled: true, ..Default::default() };
        config.providers.gemini.enabled = true;
        config.providers.gemini.mode = GeminiPromptCacheMode::Explicit;
        config.providers.gemini.explicit_ttl_seconds = Some(1200);

        let provider =
            GeminiProvider::from_config(Some("key".into()), None, None, Some(config.clone()), None, None, None);

        // Trigger request creation. It shouldn't panic or fail, even if explicit logic is placeholder.
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())].into(),
            model: "gemini-1.5-pro".to_string(),
            ..Default::default()
        };
        let res = provider.convert_to_gemini_request(&request);
        assert!(res.is_ok(), "Request conversion should succeed");

        // Verify the request conversion produces correct structure with explicit TTL
        let gemini_req = res.expect("request conversion");

        assert!(!gemini_req.contents.is_empty(), "Contents should not be empty");
        // Fail-safe: explicit mode emits the implicit text-only wire shape.
        // An inline `ttlSeconds` part is not valid generateContent schema
        // (`systemInstruction` accepts text only); true explicit caching needs
        // the separate cachedContents lifecycle, which is unimplemented.
        let system_str = serde_json::to_string(&gemini_req.system_instruction).unwrap_or_default();
        assert!(!system_str.contains("ttlSeconds"), "no inline TTL part on the wire");
        assert!(gemini_req.system_instruction.is_some(), "System instruction should be set");
    }

    /// Explicit-cache provider pointed at a mock server.
    fn explicit_cache_provider(server_uri: &str, model: &str) -> GeminiProvider {
        let mut config = PromptCachingConfig { enabled: true, ..Default::default() };
        config.providers.gemini.enabled = true;
        config.providers.gemini.mode = GeminiPromptCacheMode::Explicit;
        config.providers.gemini.explicit_ttl_seconds = Some(1200);
        GeminiProvider::from_config(
            Some("test-key".to_string()),
            Some(model.to_string()),
            Some(server_uri.to_string()),
            Some(config),
            None,
            None,
            None,
        )
    }

    use crate::providers::test_support::start_mock_server_or_skip;

    /// First attempt carries `cachedContent` and gets a stale-cache 404; the
    /// uncached retry gets a minimal SSE success.
    fn stale_cache_then_stream(request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        if body.get("cachedContent").is_some() {
            return wiremock::ResponseTemplate::new(404).set_body_json(json!({
                "error": {
                    "code": 404,
                    "message": "CachedContent not found: cachedContents/vtcode-test",
                    "status": "NOT_FOUND"
                }
            }));
        }
        wiremock::ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw("data: {\"candidates\":[]}\n\n", "text/event-stream")
    }

    /// Explicit-cache creation succeeds against `{base}/cachedContents`.
    fn cached_contents_mock() -> wiremock::Mock {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        Mock::given(method("POST"))
            .and(path("/cachedContents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "cachedContents/vtcode-test" })))
    }

    fn stream_attempts(requests: &[wiremock::Request]) -> Vec<&wiremock::Request> {
        requests
            .iter()
            .filter(|request| request.url.path().ends_with(":streamGenerateContent"))
            .collect()
    }

    #[tokio::test]
    async fn stream_recovers_from_stale_explicit_cache_with_uncached_retry() {
        use wiremock::Mock;
        use wiremock::matchers::{method, path};

        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let model = models::google::GEMINI_3_FLASH_PREVIEW;

        cached_contents_mock().mount(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/models/{model}:streamGenerateContent")))
            .respond_with(stale_cache_then_stream)
            .mount(&server)
            .await;

        let provider = explicit_cache_provider(&server.uri(), model);
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        if let Err(err) = provider.stream(request).await {
            panic!("a stale cachedContent must not fail the streaming turn: {err}");
        }

        let requests = server.received_requests().await.expect("received requests");
        let attempts = stream_attempts(&requests);
        assert_eq!(attempts.len(), 2, "the stale cache name must be retried exactly once");
        let cached: Value = serde_json::from_slice(&attempts[0].body).expect("cached attempt body");
        let retried: Value = serde_json::from_slice(&attempts[1].body).expect("retry body");
        assert_eq!(cached["cachedContent"], "cachedContents/vtcode-test");
        assert!(cached.get("systemInstruction").is_none(), "the cached attempt supplies the prefix from the cache");
        assert!(cached.get("toolConfig").is_none(), "the API rejects toolConfig alongside cachedContent");
        assert!(retried.get("cachedContent").is_none(), "the retry must not reuse the expired cache name");
        assert!(retried.get("systemInstruction").is_some(), "the retry must resend the full system instruction");
        assert!(
            provider.explicit_cache.current().is_none(),
            "the dead cache slot must be cleared so the next turn rebuilds it"
        );
    }

    #[tokio::test]
    async fn non_stale_stream_failure_is_not_retried_without_cache() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let model = models::google::GEMINI_3_FLASH_PREVIEW;

        cached_contents_mock().mount(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/models/{model}:streamGenerateContent")))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "code": 400, "message": "Invalid request: unsupported generationConfig field" }
            })))
            .mount(&server)
            .await;

        let provider = explicit_cache_provider(&server.uri(), model);
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        assert!(provider.stream(request).await.is_err(), "a non-stale 400 must surface as an error");

        let requests = server.received_requests().await.expect("received requests");
        assert_eq!(stream_attempts(&requests).len(), 1, "only stale-cache failures may trigger the uncached retry");
        assert!(
            provider.explicit_cache.current().is_some(),
            "an unrelated failure must keep the cache slot for the next request"
        );
    }

    /// The API forbids `toolConfig` next to `cachedContent`, so a request that
    /// constrains tool use must keep the implicit shape and send the config
    /// itself; otherwise the constraint is silently lost. The segment entry
    /// must survive that skip so later default-config turns reuse it.
    #[tokio::test]
    async fn constrained_tool_choice_skips_cache_without_losing_the_segment_entry() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let model = models::google::GEMINI_3_FLASH_PREVIEW;

        cached_contents_mock().mount(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/models/{model}:streamGenerateContent")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw("data: {\"candidates\":[]}\n\n", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = explicit_cache_provider(&server.uri(), model);
        let default_request = |tool_choice: Option<ToolChoice>| LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "search_workspace".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }),
            )])),
            tool_choice,
            ..Default::default()
        };

        // 1. A default-config turn installs the segment entry.
        if let Err(err) = provider.stream(default_request(None)).await {
            panic!("the default-config turn must succeed: {err}");
        }
        let installed = provider
            .explicit_cache
            .current()
            .expect("the default-config turn installs a segment entry")
            .0;
        assert_eq!(installed, "cachedContents/vtcode-test");

        // 2. The constrained turn keeps its config on the body and must not
        //    clear or replace the installed entry.
        if let Err(err) = provider.stream(default_request(Some(ToolChoice::None))).await {
            panic!("a constrained tool choice must stay enforceable on the wire: {err}");
        }
        assert_eq!(
            provider.explicit_cache.current().map(|(name, _)| name).as_deref(),
            Some(installed.as_str()),
            "skipping the cache must not lose the segment entry"
        );

        // 3. The next default-config turn reuses that same entry.
        if let Err(err) = provider.stream(default_request(None)).await {
            panic!("the following default-config turn must succeed: {err}");
        }

        let requests = server.received_requests().await.expect("received requests");
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path().ends_with("/cachedContents"))
                .count(),
            1,
            "only the default-config turn may create the cache"
        );
        let attempts = stream_attempts(&requests);
        assert_eq!(attempts.len(), 3, "each turn issues exactly one stream request");
        let body = |index: usize| -> Value { serde_json::from_slice(&attempts[index].body).expect("stream body") };
        assert_eq!(body(0)["cachedContent"], "cachedContents/vtcode-test");
        assert!(body(1).get("cachedContent").is_none(), "the constraint cannot travel alongside cachedContent");
        assert_eq!(
            body(1)["toolConfig"]["functionCallingConfig"]["mode"],
            "NONE",
            "the disabled-tool constraint must reach the provider"
        );
        assert_eq!(
            body(2)["cachedContent"],
            "cachedContents/vtcode-test",
            "the following default-config turn must reuse the surviving entry"
        );
    }
}

#[test]
fn grown_history_keeps_contents_prefix_stable() {
    // Implicit caching keys on the request prefix: a grown second turn must
    // extend `contents` without rewriting earlier turns, and the system
    // instruction must repeat verbatim.
    let provider = GeminiProvider::new("test-key".to_string());
    let first_turn = vec![
        Message::user("list files".to_string()),
        Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                "call_1".to_string(),
                "list_files".to_string(),
                json!({ "path": "." }).to_string(),
            )],
        ),
        Message::tool_response("call_1".to_string(), "a.txt".to_string()),
    ];
    let mut second_turn = first_turn.clone();
    second_turn.push(Message::user("read a.txt".to_string()));

    let build = |messages: Vec<Message>| {
        provider
            .convert_to_gemini_request(&LLMRequest {
                messages: messages.into(),
                system_prompt: Some(Arc::from("stable instructions")),
                model: "gemini-2.5-flash".to_string(),
                ..Default::default()
            })
            .expect("conversion should succeed")
    };

    let first = build(first_turn);
    let second = build(second_turn);

    assert_eq!(
        serde_json::to_value(&first.system_instruction).expect("serialize"),
        serde_json::to_value(&second.system_instruction).expect("serialize"),
        "system instruction must repeat verbatim"
    );
    let first_contents = serde_json::to_value(&first.contents).expect("serialize");
    let second_contents = serde_json::to_value(&second.contents).expect("serialize");
    let (Some(first_items), Some(second_items)) = (first_contents.as_array(), second_contents.as_array()) else {
        panic!("contents should serialize as arrays");
    };
    assert!(second_items.len() > first_items.len(), "grown history must extend contents");
    assert_eq!(
        &second_items[..first_items.len()],
        first_items,
        "grown history must not rewrite the contents prefix"
    );
}

#[test]
fn part_json_deserialization_function_call_with_thought_signature() {
    // Test 1: FunctionCall with thoughtSignature (camelCase - native Gemini API)
    let json_camel = json!({
        "functionCall": {"name": "test_func", "args": {"key": "value"}},
        "thoughtSignature": "sig_camel_123"
    });
    let part: Part =
        serde_json::from_value(json_camel).expect("should deserialize function call with camelCase thoughtSignature");
    match &part {
        Part::FunctionCall { function_call, thought_signature } => {
            assert_eq!(function_call.name, "test_func");
            assert_eq!(
                thought_signature.as_deref(),
                Some("sig_camel_123"),
                "thoughtSignature (camelCase) should be captured"
            );
        }
        other => panic!("Expected FunctionCall, got {other:?}"),
    }

    // Test 2: FunctionCall WITHOUT thought signature
    let json_no_sig = json!({
        "functionCall": {"name": "test_func", "args": {"key": "value"}}
    });
    let part2: Part = serde_json::from_value(json_no_sig).expect("should deserialize function call without signature");
    match &part2 {
        Part::FunctionCall { function_call, thought_signature } => {
            assert_eq!(function_call.name, "test_func");
            assert_eq!(thought_signature, &None, "missing signature should be None");
        }
        other => panic!("Expected FunctionCall, got {other:?}"),
    }

    // Test 3: Text part
    let json_text = json!({"text": "hello world"});
    let part3: Part = serde_json::from_value(json_text).expect("should deserialize text part");
    match &part3 {
        Part::Text { text, .. } => {
            assert_eq!(text, "hello world");
        }
        other => panic!("Expected Text, got {other:?}"),
    }

    // Test 4: Full candidate with function call + thought signature (simulates API response)
    let candidate_json = json!({
        "content": {
            "role": "model",
            "parts": [{
                "functionCall": {"name": "exec_command", "args": {"command": "cargo check"}},
                "thoughtSignature": "api_signature_abc"
            }]
        },
        "finishReason": "FUNCTION_CALL"
    });
    let candidate: StreamingCandidate =
        serde_json::from_value(candidate_json).expect("should deserialize streaming candidate");
    assert_eq!(candidate.content.parts.len(), 1);
    match &candidate.content.parts[0] {
        Part::FunctionCall { function_call, thought_signature } => {
            assert_eq!(function_call.name, "exec_command");
            assert_eq!(
                thought_signature.as_deref(),
                Some("api_signature_abc"),
                "thought signature should be preserved from API response"
            );
        }
        other => panic!("Expected FunctionCall in candidate, got {other:?}"),
    }
}
