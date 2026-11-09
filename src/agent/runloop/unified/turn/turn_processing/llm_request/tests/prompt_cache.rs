use super::*;

#[test]
fn openai_prompt_cache_enablement_requires_provider_and_flags() {
    assert!(is_openai_prompt_cache_enabled("openai", true, true));
    assert!(is_openai_prompt_cache_enabled("merge-gateway", true, true));
    assert!(!is_openai_prompt_cache_enabled("openai", false, true));
    assert!(!is_openai_prompt_cache_enabled("openai", true, false));
    assert!(!is_openai_prompt_cache_enabled("merge-gateway", false, true));
    assert!(!is_openai_prompt_cache_enabled("anthropic", true, true));
    // Sticky-routing providers use session affinity keys independently of the OpenAI cache flag.
    assert!(!is_openai_prompt_cache_enabled("openrouter", true, true));
    assert!(!is_openai_prompt_cache_enabled("opencode-go", true, true));
    assert!(is_session_affinity_key_enabled("openrouter", true, false));
    assert!(is_session_affinity_key_enabled("xai", true, false));
    assert!(is_session_affinity_key_enabled("opencode-go", true, false));
    assert!(is_session_affinity_key_enabled("openai", true, true));
    assert!(!is_session_affinity_key_enabled("openai", true, false));
    assert!(!is_session_affinity_key_enabled("openrouter", false, true));
    assert!(!is_session_affinity_key_enabled("opencode-go", false, true));
    assert!(!is_session_affinity_key_enabled("anthropic", true, true));
}
#[test]
fn prompt_cache_shaping_mode_requires_global_opt_in_and_provider_cache() {
    let mut cfg = PromptCachingConfig {
        enabled: true,
        cache_friendly_prompt_shaping: true,
        ..PromptCachingConfig::default()
    };
    cfg.providers.openai.enabled = true;

    assert_eq!(resolve_prompt_cache_shaping_mode("openai", &cfg), PromptCacheShapingMode::TrailingRuntimeContext);

    cfg.cache_friendly_prompt_shaping = false;
    assert_eq!(resolve_prompt_cache_shaping_mode("openai", &cfg), PromptCacheShapingMode::Disabled);
}
#[test]
fn prompt_cache_shaping_mode_uses_block_mode_for_anthropic_family() {
    let mut cfg = PromptCachingConfig {
        enabled: true,
        cache_friendly_prompt_shaping: true,
        ..PromptCachingConfig::default()
    };
    cfg.providers.anthropic.enabled = true;

    assert_eq!(
        resolve_prompt_cache_shaping_mode("anthropic", &cfg),
        PromptCacheShapingMode::AnthropicBlockRuntimeContext
    );
    assert_eq!(
        resolve_prompt_cache_shaping_mode("minimax", &cfg),
        PromptCacheShapingMode::AnthropicBlockRuntimeContext
    );
}
#[test]
fn prompt_cache_shaping_mode_respects_gemini_mode_off() {
    let mut cfg = PromptCachingConfig {
        enabled: true,
        cache_friendly_prompt_shaping: true,
        ..PromptCachingConfig::default()
    };
    cfg.providers.gemini.enabled = true;
    cfg.providers.gemini.mode = vtcode_core::config::core::GeminiPromptCacheMode::Off;

    assert_eq!(resolve_prompt_cache_shaping_mode("gemini", &cfg), PromptCacheShapingMode::Disabled);
}
#[test]
fn openai_prompt_cache_key_uses_stable_session_identifier() {
    let lineage_id = "lineage-abc-123";
    let first = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(lineage_id));
    let second = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(lineage_id));

    assert_eq!(first, Some("vtcode:openai:lineage-abc-123".to_string()));
    assert_eq!(first, second);
}
#[test]
fn session_affinity_prompt_cache_key_is_populated_for_sticky_routing_providers() {
    use vtcode_core::config::build_session_affinity_prompt_cache_key;

    // Production gate: sticky-routing providers get lineage when global prompt-cache is on.
    for provider in ["openrouter", "xai", "opencode-go"] {
        assert!(is_session_affinity_key_enabled(provider, true, false), "{provider}");

        let key = build_session_affinity_prompt_cache_key(
            provider,
            is_session_affinity_key_enabled(provider, true, false),
            &OpenAIPromptCacheKeyMode::Off,
            Some("session-lineage-9"),
        );
        let expected = format!("vtcode:{provider}:session-lineage-9");
        assert_eq!(key.as_deref(), Some(expected.as_str()), "{provider}");
    }

    // Global prompt-cache off leaves production lineage unset.
    assert_eq!(
        build_session_affinity_prompt_cache_key(
            "openrouter",
            is_session_affinity_key_enabled("openrouter", false, true),
            &OpenAIPromptCacheKeyMode::Session,
            Some("session-lineage-9"),
        ),
        None
    );
}
#[test]
fn merge_gateway_prompt_cache_key_is_stable_and_unsuffixed() {
    // Interactive request assembly must pass this mapped key through unchanged
    // (no `{stable_prefix_hash}` suffix). Prefix identity is tracked in
    // tool_catalog_hash / system_prompt_prefix_hash, not the wire key —
    // same contract as AgentRunner after c714e4593.
    let key = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some("lineage-abc-123"))
        .map(|key| map_prompt_cache_key_for_provider("merge-gateway", key));

    assert_eq!(key, Some("vtcode:merge:lineage-abc-123".to_string()));
}
#[test]
fn openai_prompt_cache_key_honors_off_mode_or_disabled_cache() {
    assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Off, Some("lineage-1"),), None);
    assert_eq!(build_openai_prompt_cache_key(false, &OpenAIPromptCacheKeyMode::Session, Some("lineage-1"),), None);
    assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, None,), None);
}
