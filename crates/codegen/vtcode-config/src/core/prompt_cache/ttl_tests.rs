use super::*;

#[test]
fn prompt_cache_ttl_parsing_and_default() {
    assert_eq!(OpenAIPromptCacheSettings::default().prompt_cache_ttl, None);
    for (wire, expected) in [("30m", PromptCacheTtl::M30), ("12h", PromptCacheTtl::H12)] {
        let parsed: OpenAIPromptCacheSettings = toml::from_str(&format!("prompt_cache_ttl = \"{wire}\"")).unwrap();
        assert_eq!(parsed.prompt_cache_ttl, Some(expected));
        assert_eq!(expected.as_str(), wire);
    }
    assert!(toml::from_str::<OpenAIPromptCacheSettings>("prompt_cache_ttl = \"24h\"").is_err());
}

#[test]
fn prompt_cache_gap_route_fallback_preserves_model_baselines() {
    let mut cfg = PromptCachingConfig::default();
    cfg.providers.openai.prompt_cache_ttl = Some(PromptCacheTtl::H12);
    for (provider, model, expected) in [
        ("openai", "gpt-6-luna", Some(1_800)),
        ("OPENAI", "gpt-6-luna", Some(1_800)),
        ("openai", "gpt-5.1-mini", Some(600)),
        ("merge-gateway", "gpt-6-luna", Some(300)),
        ("custom", "gpt-6-luna", None),
    ] {
        assert_eq!(cfg.gap_threshold_secs_for_route(provider, model, None), expected, "{provider}/{model}");
    }
    assert_eq!(cfg.gap_threshold_secs_for_route("openai", "gpt-6-luna", Some(PromptCacheTtl::H12)), Some(43_200));
    cfg.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::H24);
    assert_eq!(cfg.gap_threshold_secs_for_route("openai", "gpt-5.1-mini", None), Some(1_800));
    cfg.gap_warning_threshold_secs = Some(77);
    assert_eq!(cfg.gap_threshold_secs_for_route("openai", "gpt-6-luna", None), Some(77));
    cfg.providers.openai.enabled = false;
    assert_eq!(cfg.gap_threshold_secs_for_route("openai", "gpt-6-luna", None), None);
}

#[test]
fn prompt_cache_ttl_gap_thresholds_follow_effective_route() {
    let mut cfg = PromptCachingConfig::default();
    for (ttl, seconds) in [(PromptCacheTtl::M30, 1_800), (PromptCacheTtl::H12, 43_200)] {
        cfg.providers.openai.prompt_cache_ttl = Some(ttl);
        assert_eq!(cfg.gap_threshold_secs_for_model("openai", "gpt-6-luna"), Some(seconds));
        assert_eq!(cfg.gap_threshold_secs_for_route("custom", "opaque-model", Some(ttl)), Some(seconds));
    }
    assert_eq!(cfg.gap_threshold_secs_for_route("custom", "opaque-model", None), None);
    cfg.gap_warning_threshold_secs = Some(77);
    assert_eq!(cfg.gap_threshold_secs_for_route("custom", "opaque-model", Some(PromptCacheTtl::H12)), Some(77));
    cfg.providers.openai.enabled = false;
    assert_eq!(cfg.gap_threshold_secs_for_route("custom", "opaque-model", Some(PromptCacheTtl::H12)), None);
}
