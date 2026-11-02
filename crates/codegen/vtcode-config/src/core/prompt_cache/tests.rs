use super::*;
use assert_fs::TempDir;
use std::fs;

/// Walk up from `CARGO_MANIFEST_DIR` to the directory whose `Cargo.toml`
/// contains `[workspace]`. Hardcoding `parent()` breaks when crates are
/// nested (e.g. `crates/codegen/<crate>` — three levels deep, not one).
fn find_workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut current = manifest.as_path();
    while let Some(parent) = current.parent() {
        let cargo_toml = parent.join("Cargo.toml");
        if cargo_toml.is_file() && fs::read_to_string(&cargo_toml).is_ok_and(|c| c.contains("[workspace]")) {
            return parent.to_path_buf();
        }
        current = parent;
    }
    panic!("could not find workspace root from CARGO_MANIFEST_DIR");
}

#[test]
fn prompt_caching_defaults_align_with_constants() {
    let cfg = PromptCachingConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.max_entries, prompt_cache::DEFAULT_MAX_ENTRIES);
    assert_eq!(cfg.max_age_days, prompt_cache::DEFAULT_MAX_AGE_DAYS);
    assert!((cfg.min_quality_threshold - prompt_cache::DEFAULT_MIN_QUALITY_THRESHOLD).abs() < f64::EPSILON);
    assert_eq!(cfg.cache_friendly_prompt_shaping, prompt_cache::DEFAULT_CACHE_FRIENDLY_PROMPT_SHAPING);
    assert!(cfg.providers.openai.enabled);
    assert_eq!(cfg.providers.openai.min_prefix_tokens, prompt_cache::OPENAI_MIN_PREFIX_TOKENS);
    assert_eq!(cfg.providers.openai.prompt_cache_key_mode, OpenAIPromptCacheKeyMode::Session);
    assert_eq!(cfg.providers.anthropic.extended_ttl_seconds, Some(prompt_cache::ANTHROPIC_EXTENDED_TTL_SECONDS));
    assert_eq!(cfg.providers.gemini.mode, GeminiPromptCacheMode::Implicit);
    assert!(cfg.providers.moonshot.enabled);
    assert_eq!(cfg.providers.openai.prompt_cache_retention, None);
}

#[test]
fn resolve_cache_dir_expands_home() {
    let cfg = PromptCachingConfig {
        cache_dir: "~/.custom/cache".to_string(),
        ..PromptCachingConfig::default()
    };
    let resolved = cfg.resolve_cache_dir(None);
    if let Some(home) = dirs::home_dir() {
        assert!(resolved.starts_with(home));
    } else {
        assert_eq!(resolved, PathBuf::from(".custom/cache"));
    }
}

#[test]
fn resolve_cache_dir_uses_workspace_when_relative() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path();
    let cfg = PromptCachingConfig {
        cache_dir: "relative/cache".to_string(),
        ..PromptCachingConfig::default()
    };
    let resolved = cfg.resolve_cache_dir(Some(workspace));
    assert_eq!(resolved, workspace.join("relative/cache"));
}

#[test]
fn prompt_cache_retention_deserializes_valid_values() {
    let cfg: OpenAIPromptCacheSettings = toml::from_str(
        r#"
        prompt_cache_retention = "in_memory"
        "#,
    )
    .unwrap();
    assert_eq!(cfg.prompt_cache_retention, Some(PromptCacheRetention::InMemory));

    let cfg2: OpenAIPromptCacheSettings = toml::from_str(
        r#"
        prompt_cache_retention = "24h"
        "#,
    )
    .unwrap();
    assert_eq!(cfg2.prompt_cache_retention, Some(PromptCacheRetention::H24));
}

#[test]
fn prompt_cache_retention_unknown_values_deserialize_as_unknown() {
    let cfg: OpenAIPromptCacheSettings = toml::from_str(
        r#"
        prompt_cache_retention = "5m"
        "#,
    )
    .unwrap();
    assert_eq!(cfg.prompt_cache_retention, Some(PromptCacheRetention::Unknown));
}

/// `PromptCacheRetention` exposes the same value through `as_str()`/
/// `Display` and the serde wire form. Both the config table and the OpenAI
/// API wire format read this enum, so a one-sided change (e.g. a bare
/// `#[serde(other)]` that serializes `Unknown` as `"Unknown"`) would desync
/// them. Pin every variant.
#[test]
fn prompt_cache_retention_as_str_matches_serde() {
    for retention in [
        PromptCacheRetention::InMemory,
        PromptCacheRetention::H24,
        PromptCacheRetention::Unknown,
    ] {
        assert_eq!(
            serde_json::to_value(retention).unwrap(),
            serde_json::json!(retention.as_str()),
            "serde wire form drifted from as_str() for {retention:?}"
        );
    }
}

#[test]
fn validate_prompt_cache_rejects_unknown_retention() {
    let mut cfg = PromptCachingConfig::default();
    cfg.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::Unknown);
    assert!(cfg.validate().is_err());
}

#[test]
fn prompt_cache_key_mode_parses_from_toml() {
    let parsed: PromptCachingConfig = toml::from_str(
        r#"
[providers.openai]
prompt_cache_key_mode = "off"
"#,
    )
    .expect("prompt cache config should parse");

    assert_eq!(parsed.providers.openai.prompt_cache_key_mode, OpenAIPromptCacheKeyMode::Off);
}

#[test]
fn build_openai_prompt_cache_key_uses_trimmed_lineage_id() {
    let key = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(" lineage-abc "));

    assert_eq!(key.as_deref(), Some("vtcode:openai:lineage-abc"));
}

#[test]
fn map_prompt_cache_key_namespaces_merge_gateway() {
    assert_eq!(
        map_prompt_cache_key_for_provider("merge-gateway", "vtcode:openai:abc".to_string()),
        "vtcode:merge:abc"
    );
    assert_eq!(map_prompt_cache_key_for_provider("openai", "vtcode:openai:abc".to_string()), "vtcode:openai:abc");
    assert_eq!(
        map_prompt_cache_key_for_provider("openrouter", "vtcode:openai:abc".to_string()),
        "vtcode:openrouter:abc"
    );
    assert_eq!(map_prompt_cache_key_for_provider("xai", "vtcode:openai:abc".to_string()), "vtcode:xai:abc");
}

#[test]
fn session_affinity_key_gate_covers_openrouter_and_xai() {
    assert!(session_affinity_key_enabled("openrouter", true, false));
    assert!(session_affinity_key_enabled("xai", true, false));
    assert!(session_affinity_key_enabled("openai", true, true));
    assert!(!session_affinity_key_enabled("openai", true, false));
    assert!(!session_affinity_key_enabled("openrouter", false, true));
    assert!(!session_affinity_key_enabled("anthropic", true, true));
    assert!(session_affinity_provider("openrouter"));
    assert!(session_affinity_provider("xai"));
    assert!(!session_affinity_provider("anthropic"));
}

#[test]
fn build_session_affinity_prompt_cache_key_namespaces_providers() {
    let mode = OpenAIPromptCacheKeyMode::Session;
    assert_eq!(
        build_session_affinity_prompt_cache_key("openai", true, &mode, Some("lineage-1")).as_deref(),
        Some("vtcode:openai:lineage-1")
    );
    assert_eq!(
        build_session_affinity_prompt_cache_key("merge-gateway", true, &mode, Some("lineage-1")).as_deref(),
        Some("vtcode:merge:lineage-1")
    );
    assert_eq!(
        build_session_affinity_prompt_cache_key("openrouter", true, &mode, Some("lineage-1")).as_deref(),
        Some("vtcode:openrouter:lineage-1")
    );
    assert_eq!(
        build_session_affinity_prompt_cache_key("xai", true, &mode, Some("lineage-1")).as_deref(),
        Some("vtcode:xai:lineage-1")
    );
    // OpenAI/Merge honor Off mode; OpenRouter/xAI ignore it for sticky routing.
    assert_eq!(
        build_session_affinity_prompt_cache_key("openai", true, &OpenAIPromptCacheKeyMode::Off, Some("lineage-1")),
        None
    );
    assert_eq!(
        build_session_affinity_prompt_cache_key("openrouter", true, &OpenAIPromptCacheKeyMode::Off, Some("lineage-1"))
            .as_deref(),
        Some("vtcode:openrouter:lineage-1")
    );
    // Disabled gate or blank lineage yields None.
    assert_eq!(build_session_affinity_prompt_cache_key("openrouter", false, &mode, Some("lineage-1")), None);
    assert_eq!(build_session_affinity_prompt_cache_key("openrouter", true, &mode, Some("  ")), None);
    assert_eq!(build_session_affinity_prompt_cache_key("anthropic", true, &mode, Some("lineage-1")), None);
}

#[test]
fn build_openai_prompt_cache_key_honors_disabled_or_off_mode() {
    assert_eq!(build_openai_prompt_cache_key(false, &OpenAIPromptCacheKeyMode::Session, Some("id")), None);
    assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Off, Some("id")), None);
    assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some("  ")), None);
}

#[test]
fn provider_enablement_respects_global_and_provider_flags() {
    let mut cfg = PromptCachingConfig { enabled: true, ..PromptCachingConfig::default() };
    cfg.providers.openai.enabled = true;
    assert!(cfg.is_provider_enabled("openai"));
    assert!(cfg.is_provider_enabled("merge-gateway"));

    cfg.enabled = false;
    assert!(!cfg.is_provider_enabled("openai"));
    assert!(!cfg.is_provider_enabled("merge-gateway"));
}

#[test]
fn provider_enablement_handles_aliases_and_modes() {
    let mut cfg = PromptCachingConfig { enabled: true, ..PromptCachingConfig::default() };

    cfg.providers.anthropic.enabled = true;
    assert!(cfg.is_provider_enabled("minimax"));

    cfg.providers.gemini.enabled = true;
    cfg.providers.gemini.mode = GeminiPromptCacheMode::Off;
    assert!(!cfg.is_provider_enabled("gemini"));
}

#[test]
fn gap_threshold_uses_provider_defaults() {
    let cfg = PromptCachingConfig::default();
    assert_eq!(cfg.gap_threshold_secs("anthropic"), Some(prompt_cache::ANTHROPIC_CACHE_GAP_WARNING_SECONDS));
    assert_eq!(cfg.gap_threshold_secs("minimax"), Some(prompt_cache::ANTHROPIC_CACHE_GAP_WARNING_SECONDS));
    assert_eq!(cfg.gap_threshold_secs("openai"), Some(prompt_cache::OPENAI_CACHE_GAP_WARNING_SECONDS));
    assert_eq!(cfg.gap_threshold_secs("gemini"), Some(prompt_cache::DEFAULT_CACHE_GAP_WARNING_SECONDS));
}

#[test]
fn gap_threshold_uses_model_ttl_and_ignores_legacy_retention_for_modern_models() {
    let mut cfg = PromptCachingConfig::default();
    for retention in [
        None,
        Some(PromptCacheRetention::H24),
        Some(PromptCacheRetention::InMemory),
    ] {
        cfg.providers.openai.prompt_cache_retention = retention;
        for model in ["gpt-5.6-sol", "gpt-6-luna", "gpt-6.1-sol"] {
            assert_eq!(cfg.gap_threshold_secs_for_model("openai", model), Some(1800), "{model}");
        }
    }
    assert_eq!(cfg.gap_threshold_secs_for_model("openai", "gpt-5.1-mini"), Some(600));
    assert_eq!(cfg.gap_threshold_secs_for_model("merge-gateway", "gpt-6-luna"), Some(300));
    cfg.gap_warning_threshold_secs = Some(77);
    assert_eq!(cfg.gap_threshold_secs_for_model("openai", "gpt-6-luna"), Some(77));
    cfg.enabled = false;
    assert_eq!(cfg.gap_threshold_secs_for_model("openai", "gpt-6-luna"), None);
}

#[test]
fn explicit_cache_models_reject_migration_aliases_and_auxiliary_mode_is_opt_in() {
    for model in [
        "gpt-5.4",
        "gpt-5-codex",
        "gpt",
        "gpt-5.1-mini",
        "openai/gpt-6-luna",
        "gpt-6-future",
    ] {
        assert!(!supports_openai_explicit_prompt_cache(model), "{model}");
    }
    for model in ["gpt-5.6", "gpt-5.6-terra", "gpt-6-astra", "gpt-6-sol"] {
        assert!(supports_openai_explicit_prompt_cache(model), "{model}");
    }
    let defaults: OpenAIPromptCacheSettings = toml::from_str("").unwrap();
    assert!(!defaults.auxiliary_explicit_cache);
    let configured: OpenAIPromptCacheSettings = toml::from_str("auxiliary_explicit_cache = true").unwrap();
    assert!(configured.auxiliary_explicit_cache);
}

#[test]
fn gap_threshold_respects_disable_switches() {
    let cfg = PromptCachingConfig { gap_warning_enabled: false, ..Default::default() };
    assert_eq!(cfg.gap_threshold_secs("anthropic"), None);

    let mut cfg = PromptCachingConfig::default();
    cfg.providers.anthropic.enabled = false;
    assert_eq!(cfg.gap_threshold_secs("anthropic"), None);
    assert_eq!(cfg.gap_threshold_secs("minimax"), None);

    let cfg = PromptCachingConfig { enabled: false, ..Default::default() };
    assert_eq!(cfg.gap_threshold_secs("openai"), None);
}

#[test]
fn gap_threshold_treats_extended_retention_as_an_advisory_not_a_guarantee() {
    let mut cfg = PromptCachingConfig::default();
    cfg.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::H24);
    assert_eq!(cfg.gap_threshold_secs("openai"), Some(1800));
    cfg.gap_warning_threshold_secs = Some(42);
    assert_eq!(cfg.gap_threshold_secs("openai"), Some(42));
    cfg.gap_warning_threshold_secs = None;

    cfg.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::InMemory);
    assert_eq!(cfg.gap_threshold_secs("openai"), Some(prompt_cache::OPENAI_CACHE_GAP_WARNING_SECONDS));
}

#[test]
fn gap_threshold_honors_explicit_override() {
    let cfg = PromptCachingConfig {
        gap_warning_threshold_secs: Some(42),
        ..Default::default()
    };
    assert_eq!(cfg.gap_threshold_secs("anthropic"), Some(42));
    assert_eq!(cfg.gap_threshold_secs("openai"), Some(42));
}

#[test]
fn gap_warning_fields_parse_from_toml() {
    let parsed: PromptCachingConfig = toml::from_str(
        r#"
gap_warning_enabled = false
gap_warning_threshold_secs = 120
"#,
    )
    .expect("prompt cache config should parse");

    assert!(!parsed.gap_warning_enabled);
    assert_eq!(parsed.gap_warning_threshold_secs, Some(120));

    let defaults: PromptCachingConfig = toml::from_str("").expect("empty config");
    assert!(defaults.gap_warning_enabled);
    assert_eq!(defaults.gap_warning_threshold_secs, None);
}

#[test]
fn bundled_config_templates_match_prompt_cache_defaults() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bootstrap_template =
        fs::read_to_string(manifest_dir.join("data/default_config.toml")).expect("bootstrap config template");
    let config: crate::VTCodeConfig = toml::from_str(&bootstrap_template).expect("parse bootstrap template");
    assert!(config.prompt_cache.enabled);
    assert!(config.prompt_cache.cache_friendly_prompt_shaping);
    assert!(bootstrap_template.contains("# prompt_cache_retention = \"24h\""));

    let workspace_root = find_workspace_root();
    let example_config = fs::read_to_string(workspace_root.join("vtcode.toml.example")).expect("vtcode.toml.example");
    if example_config.contains("[prompt_cache]") {
        assert!(example_config.contains("enabled = true"));
        assert!(example_config.contains("cache_friendly_prompt_shaping = true"));
        assert!(example_config.contains("# prompt_cache_retention = \"24h\""));
    }

    let prompt_cache_guide =
        fs::read_to_string(workspace_root.join("docs/tools/PROMPT_CACHING_GUIDE.md")).expect("prompt caching guide");
    // The guide is markdownlint-wrapped, so sentences may be reflowed across
    // lines; match against whitespace-normalized text instead of raw lines.
    let normalized_guide = vtcode_commons::formatting::collapse_whitespace(&prompt_cache_guide);
    assert!(
        normalized_guide.contains("VT Code enables `prompt_cache.cache_friendly_prompt_shaping = true` by default.")
    );
    // The retention doc block (guide lines 44-48) states `prompt_cache_retention` is
    // opt-in for older supported Responses models; assert that intent survives doc rewrites.
    assert!(normalized_guide.contains("`prompt_cache_retention` is opt-in for older supported Responses models"));

    let field_reference = fs::read_to_string(workspace_root.join("docs/config/CONFIG_FIELD_REFERENCE.md"))
        .expect("config field reference");
    assert!(field_reference.contains("`prompt_cache.cache_friendly_prompt_shaping`"));
    assert!(field_reference.contains("`prompt_cache.providers.openai.prompt_cache_retention`"));
}

#[test]
fn bundled_example_config_parses_and_validates() {
    // Guards the tracked template against stale keys and schema drift:
    // the shipped example must load as VTCodeConfig and pass validation.
    let workspace_root = find_workspace_root();
    let example = fs::read_to_string(workspace_root.join("vtcode.toml.example")).expect("vtcode.toml.example");
    let config: crate::VTCodeConfig = toml::from_str(&example).expect("example config should parse");
    config.validate().expect("example config should validate");
}
