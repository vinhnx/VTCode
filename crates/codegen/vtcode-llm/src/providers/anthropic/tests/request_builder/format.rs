use super::*;

#[test]
fn test_tool_result_blocks_empty() {
    let blocks = tool_result_blocks("");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "text");
}

#[test]
fn test_tool_result_blocks_plain_text() {
    let blocks = tool_result_blocks("Hello world");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "text");
    assert_eq!(blocks[0]["text"], "Hello world");
}

#[test]
fn test_tool_result_blocks_json_string() {
    let blocks = tool_result_blocks("\"Hello\"");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "text");
    assert_eq!(blocks[0]["text"], "Hello");
}

#[test]
fn test_tool_result_blocks_json_object() {
    let blocks = tool_result_blocks("{\"key\": \"value\"}");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["type"], "text");
    assert_eq!(blocks[0]["text"], "{\"key\":\"value\"}");
}

#[test]
fn test_convert_to_anthropic_format_adds_top_level_cache_control() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["cache_control"]["type"], "ephemeral");
    assert_eq!(payload["cache_control"]["ttl"], "5m");
}

#[test]
fn test_convert_to_anthropic_format_preserves_opus_5_mid_conversation_system_message() {
    let request = LLMRequest {
        model: models::CLAUDE_OPUS_5.to_string(),
        messages: vec![
            Message::user("Review this code.".to_string()),
            Message::system("From now on, every suggestion must include explicit type annotations.".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["messages"][0]["role"], "user");
    assert_eq!(payload["messages"][1]["role"], "system");
    assert_eq!(
        payload["messages"][1]["content"][0]["text"],
        "From now on, every suggestion must include explicit type annotations."
    );
}

#[test]
fn test_convert_to_anthropic_format_preserves_opus_48_mid_conversation_system_message() {
    let request = LLMRequest {
        model: models::CLAUDE_OPUS_5.to_string(),
        messages: vec![
            Message::user("Review this code.".to_string()),
            Message::system("From now on, every suggestion must include explicit type annotations.".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["messages"][0]["role"], "user");
    assert_eq!(payload["messages"][1]["role"], "system");
    assert_eq!(
        payload["messages"][1]["content"][0]["text"],
        "From now on, every suggestion must include explicit type annotations."
    );
}

#[test]
fn test_convert_to_anthropic_format_drops_mid_conversation_system_for_pre_opus_48() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![
            Message::user("Review this code.".to_string()),
            Message::system("Use strict typing.".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let cache_settings = AnthropicPromptCacheSettings::default();
    let anthropic_config = AnthropicConfig::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: true,
        prompt_cache_settings: &cache_settings,
        anthropic_config: &anthropic_config,
        model: models::anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: false,
    };

    let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

    assert_eq!(payload["messages"].as_array().unwrap().len(), 1);
    assert_eq!(payload["messages"][0]["role"], "user");
}

#[test]
fn test_convert_to_anthropic_format_uses_native_structured_outputs() {
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

    assert_eq!(payload["output_config"]["format"]["type"], "json_schema");
    assert_eq!(payload["output_config"]["format"]["schema"]["required"], json!(["answer"]));
    assert!(payload.get("tools").is_none());
}
