use super::*;

#[test]
fn test_convert_to_anthropic_format_reuses_last_explicit_ttl_for_automatic_cache() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("system prompt")),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings {
        cache_tool_definitions: false,
        cache_user_messages: false,
        tools_ttl_seconds: 3600,
        messages_ttl_seconds: 300,
        ..AnthropicPromptCacheSettings::default()
    };
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["cache_control"]["ttl"], "1h");
    assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
}

#[test]
fn test_convert_to_anthropic_format_skips_automatic_cache_when_slots_exhausted() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("stable system")),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "do_work".to_string(),
            "Do work".to_string(),
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )])),
        messages: vec![
            Message::user("aaaaaaaa".to_string()),
            Message::user("bbbbbbbb".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings {
        max_breakpoints: 4,
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert!(payload.get("cache_control").is_none());
    assert!(payload["tools"][0]["cache_control"].is_object());
    assert!(payload["system"][0]["cache_control"].is_object());
    assert!(payload["messages"][0]["content"][0]["cache_control"].is_object());
    assert!(payload["messages"][1]["content"][0]["cache_control"].is_object());
}

#[test]
fn test_rolling_anchors_only_last_two_qualifying_messages() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("stable system")),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "do_work".to_string(),
            "Do work".to_string(),
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )])),
        messages: vec![
            Message::user("aaaaaaaa".to_string()),
            Message::user("bbbbbbbb".to_string()),
            Message::user("cccccccc".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings {
        max_breakpoints: 4,
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    // Tools and system consume two breakpoints; the remaining two land on
    // the last two qualifying user messages (rolling anchors). The oldest
    // qualifying message must NOT be anchored.
    assert!(payload["tools"][0]["cache_control"].is_object());
    assert!(payload["system"][0]["cache_control"].is_object());
    assert!(payload["messages"][0]["content"][0].get("cache_control").is_none());
    assert!(payload["messages"][1]["content"][0]["cache_control"].is_object());
    assert!(payload["messages"][2]["content"][0]["cache_control"].is_object());
}

#[test]
fn test_message_anchoring_skipped_when_tools_and_system_exhaust_breakpoints() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("stable system")),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "do_work".to_string(),
            "Do work".to_string(),
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )])),
        messages: vec![
            Message::user("aaaaaaaa".to_string()),
            Message::user("bbbbbbbb".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings {
        max_breakpoints: 2,
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    // Tools and system exhaust the budget; no message gets an anchor.
    assert!(payload["tools"][0]["cache_control"].is_object());
    assert!(payload["system"][0]["cache_control"].is_object());
    assert!(payload["messages"][0]["content"][0].get("cache_control").is_none());
    assert!(payload["messages"][1]["content"][0].get("cache_control").is_none());
}

#[test]
fn test_message_anchoring_uses_remaining_breakpoint_on_newest_message() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("stable system")),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "do_work".to_string(),
            "Do work".to_string(),
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )])),
        messages: vec![
            Message::user("aaaaaaaa".to_string()),
            Message::user("bbbbbbbb".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings {
        max_breakpoints: 3,
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    // Only one breakpoint is left after tools+system; it must go to the
    // newest qualifying message (the primary rolling anchor).
    assert!(payload["messages"][0]["content"][0].get("cache_control").is_none());
    assert!(payload["messages"][1]["content"][0]["cache_control"].is_object());
}
