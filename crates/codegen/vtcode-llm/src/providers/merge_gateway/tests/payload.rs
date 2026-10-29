use super::*;

#[test]
fn native_payload_serializes_message_tool_use_and_tool_result_items() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let request = LLMRequest {
        system_prompt: Some(Arc::from("You are helpful")),
        messages: vec![
            Message::user("hello".to_string()),
            Message::assistant_with_tools(
                "calling tool".to_string(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "get_weather".to_string(),
                    r#"{"location":"Paris"}"#.to_string(),
                )],
            ),
            Message::tool_response("call_1".to_string(), "sunny".to_string()),
        ]
        .into(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        )])),
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        max_tokens: Some(128),
        temperature: Some(0.2),
        top_p: Some(0.9),
        stop_sequences: Some(vec!["END".to_string()]),
        tool_choice: Some(ToolChoice::Auto),
        output_format: Some(json!({
            "type": "json_schema",
            "json_schema": {
                "name": "weather",
                "schema": {"type": "object"}
            }
        })),
        service_tier: Some("flex".to_string()),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");
    let input = payload["input"].as_array().expect("input array");

    assert_eq!(payload["model"], models::merge_gateway::DEFAULT_ROUTING);
    assert_eq!(payload["max_tokens"], 128);
    assert!((payload["temperature"].as_f64().expect("temperature should be numeric") - 0.2).abs() < 1e-6);
    assert!((payload["top_p"].as_f64().expect("top_p should be numeric") - 0.9).abs() < 1e-6);
    assert_eq!(payload["stop"], json!(["END"]));
    assert_eq!(payload["tool_choice"], json!("auto"));
    assert_eq!(payload["response_format"]["type"], "json_schema");
    assert_eq!(payload["service_tier"], "flex");
    assert_eq!(payload["tools"][0]["type"], "function");
    assert_eq!(payload["tools"][0]["name"], "get_weather");
    assert_eq!(payload["tools"][0]["parameters"]["required"], json!(["location"]));
    assert_eq!(input.len(), 4);
    assert_eq!(input[0]["type"], "message");
    assert_eq!(input[0]["role"], "system");
    assert_eq!(input[0]["content"], "You are helpful");
    assert_eq!(input[1]["type"], "message");
    assert_eq!(input[1]["role"], "user");
    assert_eq!(input[1]["content"], "hello");
    assert_eq!(input[2]["type"], "message");
    assert_eq!(input[2]["role"], "assistant");
    assert!(input[2]["content"].is_array());
    assert_eq!(input[2]["content"][0]["type"], "text");
    assert_eq!(input[2]["content"][1]["type"], "tool_use");
    assert_eq!(input[2]["content"][1]["id"], "call_1");
    assert_eq!(input[2]["content"][1]["name"], "get_weather");
    assert_eq!(input[2]["content"][1]["input"], json!({"location": "Paris"}));
    assert_eq!(input[3]["type"], "tool_result");
    assert_eq!(input[3]["tool_use_id"], "call_1");
    assert_eq!(input[3]["content"], "sunny");
}

#[test]
fn native_payload_keeps_wire_prefix_byte_stable_across_grown_history() {
    // Companion to the OpenAI `grown_history_keeps_wire_prefix_byte_stable`
    // guard: Merge Gateway is the default provider, so its native payload
    // must also render grown history as a pure extension. The system
    // message stays at input[0] and earlier items must be byte-identical.
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let history = vec![
        Message::user("list files".to_string()),
        Message::assistant_with_tools(
            "checking".to_string(),
            vec![ToolCall::function(
                "call_1".to_string(),
                "get_weather".to_string(),
                r#"{"location":"Paris"}"#.to_string(),
            )],
        ),
        Message::tool_response("call_1".to_string(), "sunny".to_string()),
    ];
    let tools = Some(Arc::new(vec![ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather".to_string(),
        json!({
            "type": "object",
            "properties": {"location": {"type": "string"}},
            "required": ["location"]
        }),
    )]));
    let first = LLMRequest {
        system_prompt: Some(Arc::from("You are helpful")),
        messages: history.clone().into(),
        tools: tools.clone(),
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        ..Default::default()
    };
    let mut grown = history;
    grown.push(Message::assistant_with_tools(
        "reading".to_string(),
        vec![ToolCall::function(
            "call_2".to_string(),
            "get_weather".to_string(),
            r#"{"location":"Nice"}"#.to_string(),
        )],
    ));
    grown.push(Message::tool_response("call_2".to_string(), "rainy".to_string()));
    let second = LLMRequest {
        system_prompt: Some(Arc::from("You are helpful")),
        messages: grown.into(),
        tools,
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        ..Default::default()
    };

    let a = provider.build_native_payload(&first, false).expect("payload");
    let b = provider.build_native_payload(&second, false).expect("payload");
    assert_eq!(a["tools"], b["tools"], "tools must not be rewritten by appended history");
    let input_a = a["input"].as_array().expect("input array");
    let input_b = b["input"].as_array().expect("input array");
    assert!(input_b.len() > input_a.len(), "grown history must extend input");
    assert_eq!(&input_b[..input_a.len()], &input_a[..], "input prefix must be append-stable across turns");
}

#[test]
fn native_payload_omits_callable_tools_for_tool_choice_none() {
    for (model, stream) in models::merge_gateway::SUPPORTED_MODELS
        .iter()
        .flat_map(|model| [(*model, false), (*model, true)])
    {
        let provider = MergeGatewayProvider::with_model("test-key".to_string(), model.to_string());
        let mut request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("Summarize the conversation.".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "read_file".to_string(),
                "Read a file".to_string(),
                json!({"type": "object"}),
            )])),
            tool_choice: Some(ToolChoice::None),
            ..Default::default()
        };

        let payload = provider.build_native_payload(&request, stream).expect("payload");
        assert!(payload.get("tools").is_none(), "tool-free request must omit tools for route {model}");
        assert!(payload.get("tool_choice").is_none(), "tool_choice=none must not be sent for route {model}");

        // The no-tool normalization must not suppress an explicit
        // tool-enabled request on any route.
        request.tool_choice = Some(ToolChoice::Auto);
        let enabled_payload = provider.build_native_payload(&request, stream).expect("enabled payload");
        assert!(enabled_payload.get("tools").is_some(), "tools must be preserved for route {model}");
        assert_eq!(enabled_payload["tool_choice"], json!("auto"));
    }
}

#[test]
fn legacy_payload_omits_callable_tools_for_tool_choice_none() {
    let provider = MergeGatewayProvider::from_config(
        Some("test-key".to_string()),
        Some(models::merge_gateway::DEFAULT_ROUTING.to_string()),
        Some("https://example.test/v1/openai".to_string()),
        None,
        None,
        None,
        None,
    );
    let core = provider.legacy_core.as_ref().expect("legacy core");

    for (model, stream) in models::merge_gateway::SUPPORTED_MODELS
        .iter()
        .flat_map(|model| [(*model, false), (*model, true)])
    {
        let mut request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("Summarize the conversation.".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "read_file".to_string(),
                "Read a file".to_string(),
                json!({"type": "object"}),
            )])),
            tool_choice: Some(ToolChoice::None),
            stream,
            ..Default::default()
        };

        let payload = core.convert_request(&request).expect("legacy payload");
        assert!(payload.get("tools").is_none(), "tool-free request must omit tools for route {model}");
        assert!(payload.get("tool_choice").is_none(), "tool_choice=none must not be sent for route {model}");

        request.tool_choice = Some(ToolChoice::Auto);
        let enabled_payload = core.convert_request(&request).expect("enabled legacy payload");
        assert!(enabled_payload.get("tools").is_some(), "tools must be preserved for route {model}");
        assert_eq!(enabled_payload["tool_choice"], json!("auto"));
    }
}

#[test]
fn native_payload_omits_callable_tools_regardless_of_tool_vendor() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        messages: vec![Message::user("Summarize.".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read a file".to_string(),
            json!({"type": "object"}),
        )])),
        tool_choice: Some(ToolChoice::None),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");
    assert!(payload.get("tools").is_none());

    // A cached missing-vendor verdict cannot reopen tool use during synthesis.
    provider.mark_tool_vendor_missing(&request.model);
    let recovered = provider.build_native_payload(&request, false).expect("recovery payload");
    assert!(recovered.get("tools").is_none(), "tools omitted when no tool vendor can serve them");
    assert!(recovered.get("tool_choice").is_none());

    // Tool-enabled requests still send tools (and fail fast upstream).
    request.tool_choice = Some(ToolChoice::Auto);
    let enabled = provider.build_native_payload(&request, false).expect("enabled payload");
    assert!(enabled.get("tools").is_some());
}

#[test]
fn native_payload_enables_response_streaming() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::XAI_GROK_4_6.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::XAI_GROK_4_6.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, true).expect("payload");

    assert_eq!(payload["stream"], true);
}

#[test]
fn native_payload_forwards_reasoning_effort_on_native_routes() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::OPENAI_GPT_5_5.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::OPENAI_GPT_5_5.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(payload["reasoning_effort"], "high");
    assert!(payload.get("thinking").is_none());
}

#[test]
fn native_payload_forwards_thinking_budget_for_budget_routes() {
    let provider = MergeGatewayProvider::with_model(
        "test-key".to_string(),
        models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5.to_string(),
    );
    let request = LLMRequest {
        model: models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        max_tokens: Some(2000),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("reasoning_effort").is_none());
    assert_eq!(payload["thinking"]["type"], "enabled");
    assert_eq!(payload["thinking"]["budget_tokens"], 1900);
}

#[test]
fn native_payload_omits_thinking_when_budget_exceeds_max_tokens() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEEPSEEK_FLASH.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::DEEPSEEK_FLASH.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::Medium),
        max_tokens: Some(1000),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("reasoning_effort").is_none());
    assert!(payload.get("thinking").is_none());
}

#[test]
fn native_payload_omits_reasoning_for_unclassified_routes() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::DEFAULT_ROUTING.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::DEFAULT_ROUTING.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("reasoning_effort").is_none());
    assert!(payload.get("thinking").is_none());
}

#[test]
fn reasoning_capabilities_are_route_aware() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::OPENAI_GPT_5_5.to_string());
    assert!(provider.supports_reasoning(models::merge_gateway::OPENAI_GPT_5_5));
    assert!(provider.supports_reasoning_effort(models::merge_gateway::OPENAI_GPT_5_5));
    assert!(provider.supports_reasoning(models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5));
    assert!(provider.supports_reasoning_effort(models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5));
    assert!(provider.supports_reasoning(models::merge_gateway::ZAI_GLM_5_3_FLASH));
    assert!(provider.supports_reasoning_effort(models::merge_gateway::ZAI_GLM_5_3_FLASH));
    assert!(!provider.supports_reasoning(models::merge_gateway::DEFAULT_ROUTING));
    assert!(!provider.supports_reasoning_effort(models::merge_gateway::DEFAULT_ROUTING));
}

#[test]
fn native_payload_forwards_reasoning_effort_on_zai_route() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::High),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(payload["reasoning_effort"], "high");
    assert!(payload.get("thinking").is_none());
}

#[test]
fn model_behavior_override_wins_for_reasoning_capabilities() {
    let model = models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5.to_string();
    let model_behavior = serde_json::from_value::<ModelConfig>(json!({
        "model_supports_reasoning": true,
        "model_supports_reasoning_effort": true,
    }))
    .expect("model behavior");
    let provider = MergeGatewayProvider::from_config(
        Some("test-key".to_string()),
        Some(model.clone()),
        None,
        None,
        None,
        None,
        Some(model_behavior),
    );

    assert!(provider.supports_reasoning(&model));
    assert!(provider.supports_reasoning_effort(&model));
}

#[test]
fn native_payload_sanitizes_gemini_incompatible_tool_schemas() {
    let provider = MergeGatewayProvider::with_model(
        "test-key".to_string(),
        models::merge_gateway::GOOGLE_GEMINI_3_7_FLASH.to_string(),
    );
    let request = LLMRequest {
        model: models::merge_gateway::GOOGLE_GEMINI_3_7_FLASH.to_string(),
        tools: Some(Arc::new(vec![
            ToolDefinition::function(
                "write_stdin".to_string(),
                "Write to a running command".to_string(),
                write_stdin_parameters(),
            ),
            ToolDefinition::function("apply_patch".to_string(), "Edit files".to_string(), apply_patch_parameters()),
        ])),
        ..Default::default()
    };

    let payload = provider.build_native_payload(&request, false).expect("payload");
    let tools = payload["tools"].as_array().expect("native tools");

    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|tool| tool["parameters"].get("anyOf").is_none()));
    assert_eq!(tools[0]["parameters"]["required"], json!(["session_id"]));
    assert!(tools[1]["parameters"].get("required").is_none());
}

#[test]
fn native_payload_forwards_prompt_cache_key_for_routing_stickiness() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("vtcode:merge:session-123".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:merge:session-123"));
}

#[test]
fn native_payload_omits_blank_prompt_cache_key() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("   ".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("prompt_cache_key").is_none());
}

#[test]
fn native_payload_includes_session_id_from_cache_key_lineage() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        system_prompt: Some(Arc::from("sys")),
        messages: vec![Message::user("hi".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("vtcode:merge:session-lineage-1".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert_eq!(
        payload.get("session_id").and_then(Value::as_str),
        Some("session-lineage-1"),
        "body session_id must be the namespaced lineage without the vtcode prefix"
    );
    assert_eq!(merge_session_identity(&request).as_deref(), Some("session-lineage-1"));
}

#[test]
fn native_payload_omits_blank_session_id() {
    let provider =
        MergeGatewayProvider::with_model("test-key".to_string(), models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string());
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        system_prompt: Some(Arc::from("sys")),
        messages: vec![Message::user("hi".to_string())].into(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("   ".to_string());

    let payload = provider.build_native_payload(&request, false).expect("payload");

    assert!(payload.get("session_id").is_none());
    assert!(merge_session_identity(&request).is_none());
}

#[test]
fn merge_session_identity_strips_legacy_prefix_hash_suffix() {
    let mut request = LLMRequest {
        model: models::merge_gateway::ZAI_GLM_5_3_FLASH.to_string(),
        ..Default::default()
    };
    request.prompt_cache_key = Some("vtcode:merge:lineage-abc-deadbeef01234567".to_string());
    assert_eq!(merge_session_identity(&request).as_deref(), Some("lineage-abc"));

    request.prompt_cache_key = Some("vtcode:openai:lineage-abc-0123456789abcdef".to_string());
    assert_eq!(merge_session_identity(&request).as_deref(), Some("lineage-abc"));

    // Non-hex / wrong-length tails stay intact.
    request.prompt_cache_key = Some("vtcode:merge:lineage-notahash".to_string());
    assert_eq!(merge_session_identity(&request).as_deref(), Some("lineage-notahash"));
}
