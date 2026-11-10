use super::*;

#[test]
fn test_long_context_hoisting_is_skipped_for_preserved_thinking_models() {
    for model in [
        models::anthropic::CLAUDE_OPUS_5_5,
        models::anthropic::CLAUDE_FABLE_5_1,
        models::anthropic::CLAUDE_FABLE_5_2,
    ] {
        let payload = long_context_payload(model);
        let messages = payload["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3, "{model}: {payload}");
        assert_eq!(messages[0]["content"][0]["text"], "short opener", "{model}: history must stay in order");
        assert_eq!(messages[1]["role"], "assistant");
        assert!(
            messages[2]["content"][0]["text"]
                .as_str()
                .is_some_and(|text| text.starts_with("large pasted document")),
            "{model}: {payload}"
        );
    }
}

#[test]
fn test_long_context_hoisting_still_applies_without_preserved_thinking() {
    let payload = long_context_payload(models::anthropic::CLAUDE_OPUS_5);
    let first_text = payload["messages"][0]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(first_text.starts_with("large pasted document"), "largest user message hoisted: {payload}");
}

#[test]
fn test_convert_to_anthropic_format_includes_native_web_search_tool() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("find latest rust release notes".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition {
            tool_type: "web_search_20260209".to_string(),
            function: None,
            allowed_callers: None,
            input_examples: None,
            web_search: Some(json!({
                "allowed_callers": ["direct"]
            })),
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

    assert_eq!(payload["tools"][0]["type"], "web_search_20260209");
    assert_eq!(payload["tools"][0]["name"], "web_search");
    assert_eq!(payload["tools"][0]["allowed_callers"], json!(["direct"]));
    assert!(payload["tools"][0]["input_schema"].is_null());
}

#[test]
fn test_convert_to_anthropic_format_rejects_mixed_web_search_domain_filters() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("search docs".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition {
            tool_type: "web_search_20250305".to_string(),
            function: None,
            allowed_callers: None,
            input_examples: None,
            web_search: Some(json!({
                "allowed_domains": ["docs.rs"],
                "blocked_domains": ["example.com"]
            })),
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

    convert_to_anthropic_format(&request, &ctx).unwrap_err();
}

#[test]
fn test_convert_to_anthropic_format_includes_native_code_execution_tool() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("analyze this csv".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition {
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

    assert_eq!(payload["tools"][0]["type"], "code_execution_20250825");
    assert_eq!(payload["tools"][0]["name"], "code_execution");
}
