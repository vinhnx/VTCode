use super::*;

#[test]
fn test_convert_to_anthropic_format_splits_runtime_context_without_caching_tail() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("stable system instructions\n[Runtime Context]\n- turns: 7\n- tool_calls: 3")),
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

    assert!(payload["system"].is_array());
    assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
    assert!(payload["system"][0]["text"].as_str().unwrap_or("").contains("stable system"));
    assert!(
        payload["system"][1]["text"]
            .as_str()
            .unwrap_or("")
            .contains("[Runtime Context]")
    );
    assert!(payload["system"][1].get("cache_control").is_none());
    assert!(payload.get("cache_control").is_none());
}

#[test]
fn test_convert_to_anthropic_format_splits_editor_only_runtime_context_without_caching_tail() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from(
            "stable system instructions\n[Runtime Context]\n## Active Editor Context\n- Active file: src/main.rs",
        )),
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

    assert!(payload["system"].is_array());
    assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
    assert!(
        payload["system"][1]["text"]
            .as_str()
            .unwrap_or("")
            .contains("## Active Editor Context")
    );
    assert!(payload["system"][1].get("cache_control").is_none());
    assert!(payload.get("cache_control").is_none());
}

#[test]
fn test_convert_to_anthropic_format_keeps_planning_notices_out_of_cached_prefix() {
    // Prompt-caching discipline: planning/full-auto transitions must live
    // in the uncached suffix so toggling them never invalidates the stable
    // prefix cache.
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from(
            "stable system instructions\n# PLANNING WORKFLOW (READ-ONLY)\nread-only\n[Harness Limits]\n- max_tool_wall_clock_secs: 60",
        )),
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

    assert!(payload["system"].is_array());
    assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
    assert!(payload["system"][0]["text"].as_str().unwrap_or("").contains("stable system"));
    assert!(!payload["system"][0]["text"].as_str().unwrap_or("").contains("PLANNING"));
    let tail = payload["system"][1]["text"].as_str().unwrap_or("");
    assert!(tail.contains("# PLANNING WORKFLOW (READ-ONLY)"));
    assert!(tail.contains("[Harness Limits]"));
    assert!(payload["system"][1].get("cache_control").is_none());
}

#[test]
fn test_convert_to_anthropic_format_uses_extended_message_ttl_for_budget_continuations() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("stable system instructions")),
        messages: vec![Message::user("resume ".repeat(60))].into(),
        prompt_cache_profile: Some(PromptCacheProfile::BudgetContinuation),
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

    assert_eq!(payload["cache_control"]["ttl"], "1h");
    assert_eq!(payload["messages"][0]["content"][0]["cache_control"]["ttl"], "1h");
}

#[test]
fn test_convert_to_anthropic_format_hoists_history_system_directives_into_system_prompt() {
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        system_prompt: Some(Arc::from("stable system instructions")),
        messages: vec![
            Message::user("explore architecture".to_string()),
            Message::system(
                "Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration.".to_string(),
            ),
        ].into(),
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

    assert!(payload["system"].is_array());
    assert!(
        payload["system"][1]["text"]
            .as_str()
            .unwrap_or("")
            .contains("[History Directives]")
    );
    assert!(
        payload["system"][1]["text"]
            .as_str()
            .unwrap_or("")
            .contains("Previous turn already completed tool execution")
    );
    assert_eq!(payload["messages"].as_array().map_or(0, |msgs| msgs.len()), 1);
    assert_eq!(payload["messages"][0]["role"], "user");
    assert!(payload["system"][1].get("cache_control").is_none());
}

#[test]
fn test_mid_conversation_system_message_is_not_duplicated_into_system_prompt() {
    let directive = "Reuse the latest tool outputs instead of rerunning the same exploration.";
    let payload = mid_conversation_payload(vec![
        Message::user("explore architecture".to_string()),
        Message::system(directive.to_string()),
    ]);

    assert_eq!(payload["messages"][1]["role"], "system");
    assert_eq!(payload["messages"][1]["content"][0]["text"], directive);
    let system_text = payload["system"].to_string();
    assert!(!system_text.contains(directive), "directive must render only in messages[]: {system_text}");
    assert!(!system_text.contains("[History Directives]"));
}

#[test]
fn test_mid_conversation_system_messages_keep_system_prompt_stable_across_turns() {
    let first_turn = mid_conversation_payload(vec![Message::user("explore architecture".to_string())]);
    let second_turn = mid_conversation_payload(vec![
        Message::user("explore architecture".to_string()),
        Message::system("Previous turn already completed tool execution.".to_string()),
        Message::assistant("Done exploring.".to_string()),
        Message::user("now summarize".to_string()),
        Message::system("Keep the summary under ten lines.".to_string()),
    ]);

    assert_eq!(first_turn["system"], second_turn["system"]);
    assert_eq!(first_turn["messages"][0]["content"][0]["text"], second_turn["messages"][0]["content"][0]["text"]);
    let roles: Vec<&str> = second_turn["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter_map(|message| message["role"].as_str())
        .collect();
    assert_eq!(roles, ["user", "system", "assistant", "user", "system"]);
}

#[test]
fn test_leading_history_system_message_is_folded_once_on_mid_conversation_route() {
    let summary = "Previous conversation summary: the parser was refactored.";
    let payload = mid_conversation_payload(vec![
        Message::system(summary.to_string()),
        Message::user("continue".to_string()),
    ]);

    // A system message cannot be messages[0], so the leading run is folded
    // into the system prompt and dropped from messages[].
    let messages = payload["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    let system_text = payload["system"].to_string();
    assert_eq!(system_text.matches(summary).count(), 1, "summary folded exactly once: {system_text}");
}
