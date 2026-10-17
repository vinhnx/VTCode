use super::*;
use vtcode_config::types::ReasoningEffortLevel;

fn runtime_config() -> AgentConfig {
    AgentConfig {
        model: "test-model".to_string(),
        api_key: String::new(),
        provider: "anthropic".to_string(),
        api_key_env: "ANTHROPIC_API_KEY".to_string(),
        workspace: std::env::temp_dir(),
        verbose: false,
        quiet: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: vtcode_core::config::types::UiSurfacePreference::default(),
        prompt_cache: vtcode_core::config::core::PromptCachingConfig::default(),
        model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: std::collections::BTreeMap::new(),
        checkpointing_enabled: false,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: 1,
        checkpointing_max_age_days: None,
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    manager: ConfigManager,
    config: AgentConfig,
    cfg: Option<VTCodeConfig>,
    client: Box<dyn LLMProvider>,
}

fn setup(tier: Option<OpenAIServiceTier>) -> Fixture {
    let dir = tempfile::tempdir().expect("workspace");
    let mut cfg = VTCodeConfig::default();
    cfg.provider.openai.service_tier = tier;
    cfg.provider.openai.websocket_mode = true;
    cfg.agent.reasoning_effort = ReasoningEffortLevel::High;
    let path = dir.path().join("vtcode.toml");
    ConfigManager::save_config_to_path(&path, &cfg).expect("write config");
    let manager = ConfigManager::load_from_file(&path).expect("load config");
    let mut config = runtime_config();
    config.provider = "openai".to_string();
    config.model = "gpt-6.1-sol".to_string();
    config.api_key = "fixture-key".to_string();
    config.workspace = dir.path().to_path_buf();
    let client = ContextWindowProvider::wrap(
        create_provider_client(&config, Some(&cfg)).expect("client"),
        &config.model,
        Some(123456),
    );
    Fixture { _dir: dir, manager, config, cfg: Some(cfg), client }
}

#[test]
fn fast_mode_transitions_persist_only_tier_and_preserve_runtime_settings() {
    for initial in [
        None,
        Some(OpenAIServiceTier::Priority),
        Some(OpenAIServiceTier::Ultrafast),
    ] {
        let Fixture { _dir, mut manager, config, mut cfg, mut client } = setup(initial);
        let original_runtime = toml::Value::try_from(cfg.as_ref().unwrap()).unwrap();
        let original_persisted = toml::Value::try_from(manager.config()).unwrap();
        for enable in [Some(true), Some(true), None, Some(false), None, Some(false)] {
            let was_enabled = cfg.as_ref().unwrap().provider.openai.service_tier == Some(OpenAIServiceTier::Ultrafast);
            let expected = enable.unwrap_or(!was_enabled).then_some(OpenAIServiceTier::Ultrafast);
            assert_eq!(
                apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, enable).unwrap(),
                expected.map(OpenAIServiceTier::as_str)
            );
            assert_eq!(client.effective_context_size(&config.model), 123456);
            assert_eq!(cfg.as_ref().unwrap().provider.openai.service_tier, expected);
            let saved = ConfigManager::load_from_file(manager.config_path().unwrap()).unwrap();
            assert_eq!(saved.config().provider.openai.service_tier, expected);
        }
        let mut expected_runtime = original_runtime;
        expected_runtime["provider"]["openai"]
            .as_table_mut()
            .unwrap()
            .remove("service_tier");
        let mut expected_persisted = original_persisted;
        expected_persisted["provider"]["openai"]
            .as_table_mut()
            .unwrap()
            .remove("service_tier");
        assert_eq!(toml::Value::try_from(cfg.as_ref().unwrap()).unwrap(), expected_runtime);
        assert_eq!(toml::Value::try_from(manager.config()).unwrap(), expected_persisted);
        assert_eq!(config.model, "gpt-6.1-sol");
        assert_eq!(config.api_key, "fixture-key");
    }
}

#[test]
fn fast_mode_can_be_cleared_after_switching_to_unsupported_model() {
    let Fixture { _dir, mut manager, mut config, mut cfg, mut client } = setup(Some(OpenAIServiceTier::Ultrafast));
    config.model = "gpt-5".to_string();
    assert!(apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(true)).is_err());
    assert_eq!(apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(false)).unwrap(), None);
    assert_eq!(cfg.unwrap().provider.openai.service_tier, None);
}

#[test]
fn fast_mode_failures_leave_active_state_unchanged() {
    for fail_save in [false, true] {
        let Fixture { _dir, mut manager, mut config, mut cfg, mut client } = setup(Some(OpenAIServiceTier::Ultrafast));
        let original = toml::Value::try_from(cfg.as_ref().unwrap()).unwrap();
        let original_client = std::ptr::from_ref(client.as_ref());
        if fail_save {
            let path = manager.config_path().unwrap();
            std::fs::remove_file(path).unwrap();
            std::fs::create_dir(path).unwrap();
        } else {
            config.provider = "invalid-provider".to_string();
        }
        let error = apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(false)).unwrap_err();
        assert!(format!("{error:#}").contains(if fail_save {
            "Failed to save service tier"
        } else {
            "/fast supports native OpenAI and Anthropic only"
        }));
        assert_eq!(toml::Value::try_from(cfg.as_ref().unwrap()).unwrap(), original);
        assert!(std::ptr::eq(original_client, client.as_ref()));
        assert_eq!(manager.config().provider.openai.service_tier, Some(OpenAIServiceTier::Ultrafast));
    }
}

#[test]
fn fast_mode_requires_native_openai_and_supported_model() {
    let Fixture { mut config, cfg, .. } = setup(None);
    for (provider, model, supported) in [
        ("openai", "gpt-6-astra", true),
        ("openai", "gpt-6.1-sol", true),
        ("openai", "gpt-5.6-sol", false),
        ("openai", "gpt-5", false),
        ("merge-gateway", "openai/gpt-6.1-sol", false),
        ("openrouter", "gpt-6.1-sol", false),
        ("custom", "gpt-6-astra", false),
    ] {
        config.provider = provider.to_string();
        config.model = model.to_string();
        assert_eq!(supports_fast_mode(&config, cfg.as_ref().unwrap()), supported, "{provider}/{model}");
    }
    config.provider = "openai".to_string();
    config.model = "gpt-6.1-sol".to_string();
    let mut cfg = cfg.unwrap();
    cfg.provider_overrides.entry("openai".to_string()).or_default().base_url =
        Some("https://gateway.example/v1".to_string());
    assert!(!supports_fast_mode(&config, &cfg));
}

#[test]
fn fast_mode_off_rebuilds_client_after_tier_is_cleared_by_config_reload() {
    let Fixture { _dir, mut manager, config, mut cfg, mut client } = setup(Some(OpenAIServiceTier::Ultrafast));
    // Config reload updates settings, but retains the client created at startup.
    cfg.as_mut().unwrap().provider.openai.service_tier = None;
    manager.save_config(cfg.as_ref().unwrap()).unwrap();
    let stale_client = std::ptr::from_ref(client.as_ref());

    assert_eq!(apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(false)).unwrap(), None);

    assert!(!std::ptr::eq(stale_client, client.as_ref()), "off must replace a client with a captured default");
    assert_eq!(client.effective_context_size(&config.model), 123456);
    assert_eq!(cfg.unwrap().provider.openai.service_tier, None);
    assert_eq!(
        ConfigManager::load_from_file(manager.config_path().unwrap())
            .unwrap()
            .config()
            .provider
            .openai
            .service_tier,
        None
    );
}

#[test]
fn fast_mode_explicit_setting_reconciles_disk_when_runtime_already_matches() {
    for (initial, persisted, enable, expected) in [
        (Some(OpenAIServiceTier::Ultrafast), None, true, Some(OpenAIServiceTier::Ultrafast)),
        (None, Some(OpenAIServiceTier::Priority), false, None),
    ] {
        let Fixture { _dir, mut manager, config, mut cfg, mut client } = setup(initial);
        let mut disk_cfg = manager.config().clone();
        disk_cfg.provider.openai.service_tier = persisted;
        manager.save_config(&disk_cfg).unwrap();

        assert_eq!(
            apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(enable)).unwrap(),
            expected.map(OpenAIServiceTier::as_str)
        );

        assert_eq!(cfg.unwrap().provider.openai.service_tier, expected);
        assert_eq!(
            ConfigManager::load_from_file(manager.config_path().unwrap())
                .unwrap()
                .config()
                .provider
                .openai
                .service_tier,
            expected
        );
    }
}

#[test]
fn fast_mode_rejects_subscription_auth_but_allows_clearing_without_changing_auth() {
    let Fixture { _dir, mut manager, mut config, mut cfg, mut client } = setup(Some(OpenAIServiceTier::Ultrafast));
    config.openai_chatgpt_auth = Some(subscription_auth());
    let original_cfg = toml::Value::try_from(cfg.as_ref().unwrap()).unwrap();
    let original_client = std::ptr::from_ref(client.as_ref());

    let error = apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(true)).unwrap_err();

    assert!(format!("{error:#}").contains("API-key"));
    assert_eq!(toml::Value::try_from(cfg.as_ref().unwrap()).unwrap(), original_cfg);
    assert!(std::ptr::eq(original_client, client.as_ref()));
    assert_eq!(manager.config().provider.openai.service_tier, Some(OpenAIServiceTier::Ultrafast));
    assert_eq!(apply_fast_mode(&config, &mut cfg, &mut client, &mut manager, Some(false)).unwrap(), None);
    let auth = config.openai_chatgpt_auth.unwrap().snapshot().unwrap();
    assert_eq!(auth.access_token, "fixture-access");
    assert_eq!(auth.account_id.as_deref(), Some("fixture-account"));
    assert_eq!(config.api_key, "fixture-key");
}

fn subscription_auth() -> vtcode_config::auth::OpenAIChatGptAuthHandle {
    use vtcode_config::auth::{OpenAIChatGptAuthHandle, OpenAIChatGptSession};

    OpenAIChatGptAuthHandle::new(
        OpenAIChatGptSession {
            openai_api_key: String::new(),
            id_token: "fixture-id".to_string(),
            access_token: "fixture-access".to_string(),
            refresh_token: "fixture-refresh".to_string(),
            account_id: Some("fixture-account".to_string()),
            email: None,
            plan: None,
            obtained_at: 1,
            refreshed_at: 1,
            expires_at: None,
        },
        vtcode_config::OpenAIAuthConfig::default(),
        vtcode_config::auth::AuthCredentialsStoreMode::default(),
    )
}

mod suggestions {
    use super::*;
    use crate::agent::runloop::unified::fast_mode::maybe_render_fast_mode_suggestion;
    use crate::agent::runloop::unified::state::SessionStats;
    use vtcode_core::ui::InlineCommand;
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_ui::tui::app::InlineHandle;

    struct SuggestionUi {
        renderer: AnsiRenderer,
        commands: tokio::sync::mpsc::UnboundedReceiver<InlineCommand>,
    }

    impl SuggestionUi {
        fn new() -> Self {
            let (sender, commands) = tokio::sync::mpsc::unbounded_channel();
            let renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
            Self { renderer, commands }
        }

        fn lines(&mut self) -> Vec<String> {
            let mut lines = Vec::new();
            while let Ok(command) = self.commands.try_recv() {
                if let InlineCommand::AppendLine { segments, .. } = command {
                    lines.push(segments.into_iter().map(|segment| segment.text).collect());
                }
            }
            lines
        }
    }

    fn api_key_config() -> AgentConfig {
        let mut config = runtime_config();
        config.provider = "openai".to_string();
        config.model = "gpt-6-astra".to_string();
        config.api_key = "fixture-key".to_string();
        config
    }

    fn native_config() -> VTCodeConfig {
        let mut cfg = VTCodeConfig::default();
        cfg.provider_overrides.entry("openai".to_string()).or_default().base_url =
            Some("https://api.openai.com/v1".to_string());
        cfg
    }

    #[test]
    fn anthropic_fast_suggestion_ignores_dormant_openai_tier_and_mentions_cache_and_price() {
        let mut config = api_key_config();
        config.provider = "anthropic".to_owned();
        config.model = "claude-opus-5-5".to_owned();
        let mut cfg = native_config();
        cfg.provider_overrides.entry("anthropic".to_owned()).or_default().base_url =
            Some("https://api.anthropic.com/v1".to_owned());
        cfg.provider.openai.service_tier = Some(OpenAIServiceTier::Ultrafast);
        let mut stats = SessionStats::default();
        let mut ui = SuggestionUi::new();
        maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
        let lines = ui.lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/fast on"));
        assert!(lines[0].contains("premium pricing"));
        assert!(lines[0].contains("prompt-cache"));
        maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
        assert!(ui.lines().is_empty());
    }

    #[test]
    fn fast_mode_suggestion_renders_actionable_info_for_both_models_and_inactive_tiers() {
        for model in ["gpt-6-astra", "gpt-6.1-sol"] {
            for tier in [None, Some(OpenAIServiceTier::Flex), Some(OpenAIServiceTier::Priority)] {
                let mut config = api_key_config();
                config.model = model.to_string();
                let mut cfg = native_config();
                cfg.provider.openai.service_tier = tier;
                let mut stats = SessionStats::default();
                let mut ui = SuggestionUi::new();

                maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();

                let lines = ui.lines();
                assert_eq!(lines.len(), 1, "{model} {tier:?}");
                assert!(lines[0].contains(model));
                assert!(lines[0].contains("/fast on"));
                assert!(lines[0].contains("higher cost"));
                assert_eq!(cfg.provider.openai.service_tier, tier);
            }
        }
    }

    #[test]
    fn fast_mode_suggestion_waits_for_eligible_selection_and_is_once_per_session() {
        let mut config = api_key_config();
        let cfg = native_config();
        let mut stats = SessionStats::default();
        let mut ui = SuggestionUi::new();
        config.model = "gpt-5.6-sol".to_string();
        maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
        assert!(ui.lines().is_empty());

        config.model = "gpt-6.1-sol".to_string();
        maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
        assert_eq!(ui.lines().len(), 1);
        for model in ["gpt-6.1-sol", "gpt-6-astra"] {
            config.model = model.to_string();
            maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
            assert!(ui.lines().is_empty());
        }
    }

    #[test]
    fn fast_mode_suggestion_requires_tui_and_native_api_key_auth() {
        let cfg = native_config();
        let config = api_key_config();
        let mut stats = SessionStats::default();
        let mut ui = SuggestionUi::new();
        maybe_render_fast_mode_suggestion(&mut AnsiRenderer::stdout(), &config, Some(&cfg), &mut stats).unwrap();
        maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, None, &mut stats).unwrap();
        assert!(ui.lines().is_empty());

        for provider in ["anthropic", "custom", "openrouter", "merge-gateway"] {
            let mut other = config.clone();
            other.provider = provider.to_string();
            maybe_render_fast_mode_suggestion(&mut ui.renderer, &other, Some(&cfg), &mut stats).unwrap();
            assert!(ui.lines().is_empty(), "{provider}");
        }
        for key in ["", "  "] {
            let mut no_key = config.clone();
            no_key.api_key = key.to_string();
            maybe_render_fast_mode_suggestion(&mut ui.renderer, &no_key, Some(&cfg), &mut stats).unwrap();
            assert!(ui.lines().is_empty());
        }
        let mut subscription = config.clone();
        subscription.openai_chatgpt_auth = Some(subscription_auth());
        maybe_render_fast_mode_suggestion(&mut ui.renderer, &subscription, Some(&cfg), &mut stats).unwrap();
        assert!(ui.lines().is_empty());

        let mut gateway = cfg.clone();
        gateway.provider_overrides.get_mut("openai").unwrap().base_url = Some("https://gateway.example/v1".to_string());
        maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&gateway), &mut stats).unwrap();
        assert!(ui.lines().is_empty());

        maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
        assert_eq!(ui.lines().len(), 1, "ineligible checks must not consume the tip");
    }

    #[test]
    fn fast_mode_suggestion_stays_hidden_once_enabled_or_explicitly_dismissed() {
        let config = api_key_config();
        for already_enabled in [false, true] {
            let mut cfg = native_config();
            let mut stats = SessionStats::default();
            let mut ui = SuggestionUi::new();
            if already_enabled {
                cfg.provider.openai.service_tier = Some(OpenAIServiceTier::Ultrafast);
            } else {
                stats.dismiss_fast_mode_suggestion();
            }
            maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
            assert!(ui.lines().is_empty());
            cfg.provider.openai.service_tier = None;
            maybe_render_fast_mode_suggestion(&mut ui.renderer, &config, Some(&cfg), &mut stats).unwrap();
            assert!(ui.lines().is_empty(), "do not remind after using or disabling fast mode");
        }
    }
}

#[path = "tests/anthropic.rs"]
mod anthropic;
