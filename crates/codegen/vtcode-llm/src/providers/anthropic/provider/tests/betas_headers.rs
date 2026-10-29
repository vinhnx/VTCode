use super::*;

#[test]
fn non_streaming_capability_is_pinned_for_stream_timeout_fallback() {
    // Pinned true in the provider impl; MinimaxProvider's delegation and
    // the runloop's stream-timeout fallback both depend on this value.
    let provider = AnthropicProvider::new("test-key".to_string());
    assert!(LLMProvider::supports_non_streaming(&provider, models::anthropic::CLAUDE_OPUS_5));
}
#[test]
fn with_leak_protection_prepends_reminder_to_system_prompt_for_every_model() {
    for model in [
        models::CLAUDE_SONNET_5,
        models::anthropic::CLAUDE_OPUS_5_5,
        "claude-unlisted-model",
    ] {
        let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hi".to_string())].into(),
            system_prompt: Some(std::sync::Arc::from("Base instructions.")),
            ..Default::default()
        };

        let protected = provider.with_leak_protection(request, "the API key");

        assert_eq!(
            protected.system_prompt.as_deref(),
            Some("[Never mention or reveal the API key]\n\nBase instructions."),
            "model {model}"
        );
        let payload = provider.convert_to_anthropic_format(&protected).expect("payload conversion");
        let messages = payload["messages"].as_array().expect("messages array");
        assert_eq!(messages.last().expect("last message")["role"], "user", "model {model}");
    }
}
#[test]
fn with_leak_protection_sets_system_prompt_when_absent() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hi".to_string())].into(),
        ..Default::default()
    };

    let protected = provider.with_leak_protection(request, "internal notes");

    assert_eq!(protected.system_prompt.as_deref(), Some("[Never mention or reveal internal notes]"));
}
#[test]
fn native_structured_outputs_do_not_require_structured_output_beta() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        output_format: Some(json!({
            "type": "object",
            "properties": {
                "answer": {"type": "string"}
            },
            "required": ["answer"],
            "additionalProperties": false
        })),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    assert_eq!(payload["output_config"]["format"]["type"], "json_schema");
    if let Some(header) = &beta_header {
        assert!(!header.contains("structured-outputs-2025-11-13"));
    }
}
#[test]
fn effective_betas_include_code_execution_but_not_files_api_for_file_inputs() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message {
            role: crate::provider::MessageRole::User,
            content: MessageContent::Parts(vec![
                ContentPart::text("Analyze this CSV".to_string()),
                ContentPart::file_from_id("file_abc123".to_string()),
            ]),
            ..Default::default()
        }]
        .into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition {
            tool_type: "code_execution_20250825".to_string(),
            function: None,
            allowed_callers: None,
            input_examples: None,
            web_search: None,
            hosted_tool_config: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
            namespace: None,
            advisor: None,
        }])),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "code-execution-2025-08-25"));
    // The Files API is GA: file_id inputs need no beta header.
    assert!(!betas.iter().any(|beta| beta.starts_with("files-api")), "betas: {betas:?}");
}
#[test]
fn effective_betas_include_context_management_beta_for_memory_tools() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("remember this preference".to_string())].into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition {
            tool_type: "memory_20250818".to_string(),
            function: None,
            allowed_callers: None,
            input_examples: None,
            web_search: None,
            hosted_tool_config: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
            namespace: None,
            advisor: None,
        }])),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "context-management-2025-06-27"));
}
#[test]
fn effective_betas_include_context_management_beta_for_context_edits() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!({
            "edits": [
                {"type": "clear_tool_uses_20250919"}
            ]
        })),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "context-management-2025-06-27"));
    assert!(!betas.iter().any(|beta| beta == "compact-2026-01-12"));
}
#[test]
fn effective_betas_include_compact_beta_for_compaction_requests() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!([
            {
                "type": "compaction",
                "compact_threshold": 180000
            }
        ])),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "compact-2026-01-12"));
}
#[test]
fn effective_betas_include_compact_beta_for_compaction_edits() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!({
            "edits": [
                {
                    "type": "compact_20260112",
                    "trigger": {
                        "type": "input_tokens",
                        "value": 180000
                    }
                }
            ]
        })),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "compact-2026-01-12"));
    assert!(!betas.iter().any(|beta| beta == "context-management-2025-06-27"));
}
#[test]
fn effective_betas_include_both_headers_for_mixed_context_edits() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!({
            "edits": [
                {"type": "clear_tool_uses_20250919"},
                {
                    "type": "compact_20260112",
                    "trigger": {
                        "type": "input_tokens",
                        "value": 180000
                    }
                }
            ]
        })),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "compact-2026-01-12"));
    assert!(betas.iter().any(|beta| beta == "context-management-2025-06-27"));
}
#[test]
fn beta_header_includes_advanced_tool_use_for_programmatic_tools() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("find warmest city".to_string())].into(),
        tools: Some(std::sync::Arc::new(vec![
            ToolDefinition::function(
                "get_weather".to_string(),
                "Get weather for a city".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }),
            )
            .with_allowed_callers(vec!["code_execution_20250825".to_string()]),
        ])),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider
        .beta_header_for_request(&request, &payload, true, None)
        .expect("beta header");

    assert!(beta_header.contains("advanced-tool-use-2025-11-20"));
}
#[test]
fn beta_header_omits_context_1m_for_native_1m_models() {
    let model = models::CLAUDE_SONNET_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    if let Some(header) = &beta_header {
        assert!(!header.contains("context-1m-2025-08-07"));
    }
}
#[test]
fn beta_header_uses_request_model_instead_of_provider_default() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    assert_eq!(payload["model"], models::CLAUDE_SONNET_5);
    if let Some(header) = &beta_header {
        assert!(!header.contains("interleaved-thinking-2025-05-14"));
    }
}
#[test]
fn beta_header_omits_interleaved_thinking_for_sonnet_5_adaptive_mode() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        thinking_budget: Some(4096),
        max_tokens: Some(8192),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    assert_eq!(payload["thinking"]["type"], "adaptive");
    if let Some(header) = &beta_header {
        assert!(!header.contains("interleaved-thinking-2025-05-14"));
    }
}
#[test]
fn opus_5_5_requests_progress_updates_display_with_its_beta() {
    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["thinking"]["type"], "adaptive");
    assert_eq!(payload["thinking"]["display"], "updates");

    let beta_header = provider
        .beta_header_for_request(&request, &payload, false, None)
        .expect("display updates beta header");
    assert_eq!(
        beta_header
            .split(", ")
            .filter(|beta| *beta == headers::THINKING_DISPLAY_UPDATES_BETA)
            .count(),
        1
    );
}
#[test]
fn display_updates_beta_covers_fallback_entries() {
    let model = models::anthropic::CLAUDE_OPUS_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        fallbacks: Some(vec![crate::provider::FallbackModel {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            max_tokens: None,
            thinking: Some(crate::provider::AnthropicThinkingConfig::Adaptive { display: Some("updates".to_string()) }),
        }]),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload["thinking"].get("display").is_none());
    assert_eq!(payload["fallbacks"][0]["thinking"]["display"], "updates");
    let beta_header = provider
        .beta_header_for_request(&request, &payload, false, None)
        .expect("beta header");
    assert!(
        beta_header
            .split(", ")
            .any(|beta| beta == headers::THINKING_DISPLAY_UPDATES_BETA)
    );
}
#[test]
fn display_updates_beta_is_omitted_when_display_is_not_updates() {
    let model = models::anthropic::CLAUDE_SONNET_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload["thinking"].get("display").is_none());
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);
    assert!(
        !beta_header
            .is_some_and(|header| { header.split(", ").any(|beta| beta == headers::THINKING_DISPLAY_UPDATES_BETA) })
    );
}
