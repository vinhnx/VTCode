use super::*;

#[test]
fn anthropic_speed_exact_models_and_dated_snapshots() {
    for (model, supported, beta) in [
        ("claude-opus-5-5", true, false),
        ("claude-opus-5-5-20261016", true, false),
        ("claude-opus-5", true, true),
        ("claude-opus-5-20261001", true, true),
        ("claude-opus-4-8", true, true),
        ("claude-opus-4-8-20260416", true, true),
        ("claude-opus-4-6", false, false),
        ("claude-sonnet-5-5", false, false),
        ("claude-opus-5-50", false, false),
        ("claude-opus-5-latest", false, false),
        ("claude-opus-5-2026101", false, false),
        ("anthropic/claude-opus-5", false, false),
        ("prefix-claude-opus-5", false, false),
    ] {
        assert_eq!(AnthropicSpeed::supports_model(model), supported, "{model}");
        assert_eq!(AnthropicSpeed::requires_beta(model), beta, "{model}");
    }
}

#[test]
fn anthropic_speed_native_endpoint_rejects_nonstandard_routes() {
    for (url, native) in [
        ("https://api.anthropic.com/v1", true),
        ("https://api.anthropic.com:443/v1/", true),
        ("http://api.anthropic.com/v1", false),
        ("https://api.anthropic.com:8443/v1", false),
        ("https://api.anthropic.com/v1/messages", false),
        ("https://api.anthropic.com/v1?gateway=true", false),
        ("https://api.anthropic.com/v1#fragment", false),
        ("https://user:secret@api.anthropic.com/v1", false),
        ("https://api.anthropic.com.example/v1", false),
        ("https://gateway.example/v1", false),
        ("", false),
    ] {
        assert_eq!(AnthropicSpeed::is_native_endpoint(url), native, "{url}");
    }
}

#[test]
fn anthropic_speed_config_override_wins_over_environment() {
    let env = vtcode_commons::env_lock::lock();
    let previous = std::env::var_os(env_vars::ANTHROPIC_BASE_URL);
    env.set_var(env_vars::ANTHROPIC_BASE_URL, " https://gateway.example/v1 ");
    assert_eq!(AnthropicSpeed::resolve_base_url(None), "https://gateway.example/v1");
    let mut cfg = VTCodeConfig::default();
    assert!(!cfg.anthropic_speed_supported("anthropic", "claude-opus-5"));
    cfg.provider_overrides.entry("anthropic".to_owned()).or_default().base_url =
        Some(" https://api.anthropic.com/v1 ".to_owned());
    assert!(cfg.anthropic_speed_supported("anthropic", "claude-opus-5"));
    assert!(!cfg.anthropic_speed_supported("openrouter", "claude-opus-5"));
    assert!(!cfg.anthropic_speed_supported("custom", "claude-opus-5"));
    cfg.provider_overrides.get_mut("anthropic").unwrap().base_url = Some(" ".to_owned());
    assert!(!cfg.anthropic_speed_supported("anthropic", "claude-opus-5"));
    env.remove_var(env_vars::ANTHROPIC_BASE_URL);
    assert_eq!(AnthropicSpeed::resolve_base_url(None), urls::ANTHROPIC_API_BASE);
    if let Some(previous) = previous {
        env.set_var(env_vars::ANTHROPIC_BASE_URL, previous);
    }
}

#[test]
fn anthropic_speed_roundtrips_and_defaults_to_absent() {
    let defaults = VTCodeConfig::default();
    assert_eq!(defaults.provider.anthropic.speed, None);
    for speed in [AnthropicSpeed::Standard, AnthropicSpeed::Fast] {
        let mut cfg = defaults.clone();
        cfg.provider.anthropic.speed = Some(speed);
        let serialized = toml::to_string(&cfg).unwrap();
        let reloaded: VTCodeConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reloaded.provider.anthropic.speed, Some(speed));
    }
    assert!(toml::from_str::<VTCodeConfig>("[provider.anthropic]\nspeed = 'turbo'").is_err());
}
