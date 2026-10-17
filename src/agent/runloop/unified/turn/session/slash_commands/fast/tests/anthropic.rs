use super::*;
use crate::agent::runloop::unified::fast_mode::effective_speed_label;

fn anthropic_fixture(speed: Option<AnthropicSpeed>) -> Fixture {
    let mut fixture = setup(Some(OpenAIServiceTier::Ultrafast));
    fixture.config.provider = "anthropic".to_owned();
    fixture.config.model = "claude-opus-5-5".to_owned();
    let cfg = fixture.cfg.as_mut().unwrap();
    cfg.provider.anthropic.speed = speed;
    cfg.provider_overrides.entry("anthropic".to_owned()).or_default().base_url =
        Some("https://api.anthropic.com/v1".to_owned());
    fixture.manager.save_config(cfg).unwrap();
    fixture.client = ContextWindowProvider::wrap(
        create_provider_client(&fixture.config, Some(cfg)).unwrap(),
        &fixture.config.model,
        Some(123456),
    );
    fixture
}

#[test]
fn anthropic_fast_toggle_on_off_persist_independently_and_rebuild_stale_client() {
    for initial in [None, Some(AnthropicSpeed::Standard), Some(AnthropicSpeed::Fast)] {
        let Fixture { _dir, mut manager, config, mut cfg, mut client } = anthropic_fixture(initial);
        for enable in [Some(true), Some(true), None, Some(false), None, Some(false)] {
            let expected = enable
                .unwrap_or(cfg.as_ref().unwrap().provider.anthropic.speed != Some(AnthropicSpeed::Fast))
                .then_some(AnthropicSpeed::Fast);
            let old_client = std::ptr::from_ref(client.as_ref());
            assert_eq!(
                apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, enable).unwrap(),
                expected.map(AnthropicSpeed::as_str)
            );
            assert!(!std::ptr::eq(old_client, client.as_ref()));
            assert_eq!(client.effective_context_size(&config.model), 123456);
            let reloaded = ConfigManager::load_from_file(manager.config_path().unwrap()).unwrap();
            assert_eq!(reloaded.config().provider.anthropic.speed, expected);
            assert_eq!(reloaded.config().provider.openai.service_tier, Some(OpenAIServiceTier::Ultrafast));
            assert_eq!(cfg.as_ref().unwrap().provider.anthropic.speed, expected);
        }
    }
}

#[test]
fn anthropic_fast_off_clears_dormant_preference_after_model_or_endpoint_switch() {
    for endpoint_switch in [false, true] {
        let Fixture { _dir, mut manager, mut config, mut cfg, mut client } =
            anthropic_fixture(Some(AnthropicSpeed::Fast));
        if endpoint_switch {
            cfg.as_mut().unwrap().provider_overrides.get_mut("anthropic").unwrap().base_url =
                Some("https://gateway.example/v1".to_owned());
        } else {
            config.model = "claude-sonnet-5-5".to_owned();
        }
        assert!(apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(true)).is_err());
        assert_eq!(effective_speed_label(&config, cfg.as_ref().unwrap()), None);
        assert_eq!(apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(false)).unwrap(), None);
        assert_eq!(cfg.as_ref().unwrap().provider.anthropic.speed, None);
        assert_eq!(
            ConfigManager::load_from_file(manager.config_path().unwrap())
                .unwrap()
                .config()
                .provider
                .anthropic
                .speed,
            None
        );
        assert_eq!(cfg.unwrap().provider.openai.service_tier, Some(OpenAIServiceTier::Ultrafast));
    }
}

#[test]
fn anthropic_fast_save_failure_preserves_active_client_and_both_preferences() {
    let Fixture { _dir, mut manager, config, mut cfg, mut client } = anthropic_fixture(Some(AnthropicSpeed::Fast));
    let original = toml::Value::try_from(cfg.as_ref().unwrap()).unwrap();
    let old_client = std::ptr::from_ref(client.as_ref());
    let path = manager.config_path().unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::create_dir(path).unwrap();
    assert!(apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(false)).is_err());
    assert_eq!(toml::Value::try_from(cfg.as_ref().unwrap()).unwrap(), original);
    assert!(std::ptr::eq(old_client, client.as_ref()));
    assert_eq!(manager.config().provider.anthropic.speed, Some(AnthropicSpeed::Fast));
}

#[test]
fn effective_fast_badge_follows_active_provider_and_reactivates_saved_preferences() {
    let Fixture { mut config, mut cfg, .. } = anthropic_fixture(Some(AnthropicSpeed::Fast));
    let cfg = cfg.as_mut().unwrap();
    for (provider, model, expected) in [
        ("anthropic", "claude-opus-5-5", Some("fast")),
        ("anthropic", "claude-sonnet-5-5", None),
        ("openai", "gpt-6.1-sol", Some("ultrafast")),
        ("openai", "gpt-5", None),
        ("openrouter", "anthropic/claude-opus-5", None),
        ("anthropic", "claude-opus-4-8", Some("fast")),
    ] {
        config.provider = provider.to_owned();
        config.model = model.to_owned();
        assert_eq!(effective_speed_label(&config, cfg), expected, "{provider}/{model}");
    }
    for provider in ["openrouter", "invalid-provider"] {
        config.provider = provider.to_owned();
        let original = toml::Value::try_from(&*cfg).unwrap();
        let mut active = Some(cfg.clone());
        let mut manager_fixture = anthropic_fixture(None);
        assert!(
            apply_fast_mode(
                &config,
                &mut active,
                &mut manager_fixture.client,
                &mut manager_fixture.manager,
                Some(false)
            )
            .is_err()
        );
        assert_eq!(toml::Value::try_from(active.as_ref().unwrap()).unwrap(), original);
    }
}

#[test]
fn anthropic_fast_off_rebuilds_client_after_config_reload_clears_preference() {
    let Fixture { _dir, mut manager, config, mut cfg, mut client } = anthropic_fixture(Some(AnthropicSpeed::Fast));
    cfg.as_mut().unwrap().provider.anthropic.speed = None;
    manager.save_config(cfg.as_ref().unwrap()).unwrap();
    let stale_client = std::ptr::from_ref(client.as_ref());
    apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(false)).unwrap();
    assert!(!std::ptr::eq(stale_client, client.as_ref()));
    assert_eq!(cfg.unwrap().provider.anthropic.speed, None);
    assert_eq!(client.effective_context_size(&config.model), 123456);
}
