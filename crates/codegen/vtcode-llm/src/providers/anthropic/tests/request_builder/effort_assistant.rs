use super::*;

#[test]
fn test_convert_to_anthropic_format_uses_model_default_effort_when_unset() {
    // Opus 5.5 is tuned for `medium`; the others default to `high`.
    for (model, expected) in [
        (models::anthropic::CLAUDE_OPUS_5_5, "medium"),
        (models::anthropic::CLAUDE_OPUS_5, "high"),
        (models::CLAUDE_SONNET_5, "high"),
        (models::anthropic::CLAUDE_FABLE_5_1, "high"),
    ] {
        let payload = adaptive_effort_payload(model, None, None);
        assert_eq!(payload["output_config"]["effort"], expected, "model {model}");
    }
}

#[test]
fn test_convert_to_anthropic_format_honors_explicit_reasoning_effort_over_model_default() {
    use vtcode_config::types::ReasoningEffortLevel;

    let payload = adaptive_effort_payload(models::anthropic::CLAUDE_OPUS_5_5, Some(ReasoningEffortLevel::High), None);
    assert_eq!(payload["output_config"]["effort"], "high");

    // `agent.reasoning_effort` / `/effort` wins over `provider.anthropic.effort`.
    let payload = adaptive_effort_payload(
        models::anthropic::CLAUDE_OPUS_5_5,
        Some(ReasoningEffortLevel::High),
        Some(ReasoningEffortLevel::Low),
    );
    assert_eq!(payload["output_config"]["effort"], "high");
}

#[test]
fn test_convert_to_anthropic_format_minimax_m31_flash_uses_adaptive_effort() {
    use vtcode_config::types::ReasoningEffortLevel;

    let payload = adaptive_effort_payload(models::minimax::MINIMAX_M3_1_FLASH, None, None);
    assert_eq!(payload["output_config"]["effort"], "max");
    assert!(payload.get("thinking").is_some());

    let payload = adaptive_effort_payload(models::minimax::MINIMAX_M3_1_FLASH, Some(ReasoningEffortLevel::High), None);
    assert_eq!(payload["output_config"]["effort"], "high");
}

#[test]
fn test_convert_to_anthropic_format_honors_explicit_configured_effort() {
    use vtcode_config::types::ReasoningEffortLevel;

    let payload = adaptive_effort_payload(models::anthropic::CLAUDE_OPUS_5_5, None, Some(ReasoningEffortLevel::XHigh));
    assert_eq!(payload["output_config"]["effort"], "xhigh");

    let payload = adaptive_effort_payload(models::CLAUDE_SONNET_5, None, Some(ReasoningEffortLevel::XHigh));
    assert_eq!(payload["output_config"]["effort"], "xhigh");
}

#[test]
fn test_convert_to_anthropic_format_never_ends_on_assistant_for_sonnet_5() {
    assert_trailing_assistant_is_followed_by_user_sentinel(models::CLAUDE_SONNET_5);
}

#[test]
fn test_convert_to_anthropic_format_never_ends_on_assistant_for_opus_5_5() {
    assert_trailing_assistant_is_followed_by_user_sentinel(models::anthropic::CLAUDE_OPUS_5_5);
}

#[test]
fn test_convert_to_anthropic_format_never_ends_on_assistant_for_unknown_model() {
    assert_trailing_assistant_is_followed_by_user_sentinel("claude-unlisted-model");
}

#[test]
fn test_convert_to_anthropic_format_never_ends_on_assistant_for_claude_4_6_and_later() {
    for model in ["claude-opus-4-8", "claude-opus-4-6", "claude-sonnet-4-6"] {
        assert_trailing_assistant_is_followed_by_user_sentinel(model);
    }
}

#[test]
fn test_convert_to_anthropic_format_keeps_trailing_assistant_for_prefill_backends() {
    for model in [
        models::minimax::MINIMAX_M3,
        "claude-haiku-4-5",
        "claude-sonnet-4-5-20250929",
    ] {
        let messages = convert_ending_on_assistant(model);

        assert_eq!(messages.len(), 2, "model {model}: {messages:?}");
        assert_eq!(messages[1]["role"], "assistant", "model {model} continues from the trailing turn");
        assert_eq!(messages[1]["content"][0]["text"], "partial answer");
    }
}

#[test]
fn test_convert_to_anthropic_format_includes_native_memory_tool() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("remember my preferred test runner".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition {
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
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: false,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["tools"][0]["type"], "memory_20250818");
    assert_eq!(payload["tools"][0]["name"], "memory");
}

#[test]
fn test_convert_to_anthropic_format_preserves_function_allowed_callers() {
    let mut tool = ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather for a city".to_string(),
        json!({
            "type": "object",
            "properties": {
                "city": {"type": "string"}
            },
            "required": ["city"]
        }),
    );
    tool.allowed_callers = Some(vec!["code_execution_20250825".to_string()]);

    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("find warmest city".to_string())].into(),
        tools: Some(Arc::new(vec![tool])),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: false,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["tools"][0]["allowed_callers"], json!(["code_execution_20250825"]));
}

#[test]
fn test_convert_to_anthropic_format_preserves_tool_examples_and_strict() {
    let tool = ToolDefinition::function(
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
    .with_strict(true)
    .with_input_examples(vec![json!({
        "input": "Weather in Paris",
        "tool_use": {
            "city": "Paris"
        }
    })]);

    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("find warmest city".to_string())].into(),
        tools: Some(Arc::new(vec![tool])),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: false,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["tools"][0]["strict"], json!(true));
    assert_eq!(
        payload["tools"][0]["input_examples"],
        json!([{
            "input": "Weather in Paris",
            "tool_use": {
                "city": "Paris"
            }
        }])
    );
}
