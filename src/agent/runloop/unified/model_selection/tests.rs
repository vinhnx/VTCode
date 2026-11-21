use super::{best_effort_reasoning, read_workspace_api_key, resolve_runtime_api_key, selected_provider_config};
use crate::agent::runloop::model_picker::ModelSelectionResult;
use tempfile::tempdir;
use vtcode_config::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;

fn selection(
    provider: &str,
    provider_enum: Option<Provider>,
    env_key: &str,
    api_key: Option<&str>,
    requires_api_key: bool,
) -> ModelSelectionResult {
    ModelSelectionResult {
        provider: provider.to_string(),
        provider_label: provider.to_string(),
        provider_enum,
        model: "test-model".to_string(),
        model_display: "test-model".to_string(),
        known_model: false,
        context_window: None,
        reasoning_supported: false,
        reasoning: ReasoningEffortLevel::Medium,
        reasoning_changed: false,
        service_tier_supported: false,
        service_tier: None,
        service_tier_changed: false,
        anthropic_speed: None,
        decisions_probe: None,
        decisions_probe_changed: false,
        api_key: api_key.map(ToString::to_string),
        credential_source: None,
        env_key: env_key.to_string(),
        requires_api_key,
        uses_chatgpt_auth: false,
        mimo_auth_method: None,
    }
}

fn runtime_config() -> vtcode_core::config::types::AgentConfig {
    vtcode_core::config::types::AgentConfig {
        model: "test-model".to_string(),
        api_key: String::new(),
        provider: "anthropic".to_string(),
        api_key_env: "ANTHROPIC_API_KEY".to_string(),
        workspace: std::env::temp_dir(),
        verbose: false,
        quiet: false,
        theme: vtcode_tui_runtime::ui::theme::DEFAULT_THEME_ID.to_string(),
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

#[test]
fn resolve_runtime_api_key_prefers_workspace_env_file() {
    let dir = tempdir().expect("temp dir");
    std::fs::write(dir.path().join(".env"), "OPENAI_API_KEY=workspace-key\n").expect("workspace env");
    let selection = selection("openai", Some(Provider::OpenAI), "OPENAI_API_KEY", None, true);

    let resolved = tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(resolve_runtime_api_key(dir.path(), None, &selection))
        .expect("workspace env should resolve");

    assert_eq!(resolved.0, "workspace-key");
}

#[test]
fn resolve_runtime_api_key_does_not_write_user_supplied_key_to_workspace_env() {
    let dir = tempdir().expect("temp dir");
    let selection = selection("openai", Some(Provider::OpenAI), "OPENAI_API_KEY", Some("user-key"), true);

    let resolved = tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(resolve_runtime_api_key(dir.path(), None, &selection))
        .expect("user key should resolve");
    let written = read_workspace_api_key(dir.path(), "OPENAI_API_KEY").expect("workspace env read");

    assert_eq!(resolved.0, "user-key");
    assert_eq!(written, None);
}

#[test]
fn resolve_runtime_api_key_errors_for_missing_custom_provider_key() {
    let dir = tempdir().expect("temp dir");
    let selection = selection("custom", None, "CUSTOM_API_KEY", None, true);

    let err = tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(resolve_runtime_api_key(dir.path(), None, &selection))
        .expect_err("missing custom provider key should fail");

    assert!(err.to_string().contains("CUSTOM_API_KEY"));
}

#[test]
fn resolve_runtime_api_key_accepts_custom_provider_command_auth_without_key() {
    let dir = tempdir().expect("temp dir");
    let mut config = VTCodeConfig::default();
    config.custom_providers.push(vtcode_config::core::CustomProviderConfig {
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        context_window: None,
        api_key_env: String::new(),
        auth: Some(vtcode_config::core::CustomProviderCommandAuthConfig {
            command: "print-token".to_string(),
            args: Vec::new(),
            cwd: None,
            timeout_ms: 1_000,
            refresh_interval_ms: 60_000,
        }),
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        ..vtcode_config::core::CustomProviderConfig::default()
    });
    let selection = selection("mycorp", None, "", None, false);

    let resolved = tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(resolve_runtime_api_key(dir.path(), Some(&config), &selection))
        .expect("command-auth custom provider should not require a static key");

    assert!(resolved.0.is_empty());
    assert!(resolved.1.is_none());
}

#[test]
fn selected_provider_config_carries_workspace_anthropic_settings() {
    let mut auth_cfg = VTCodeConfig::default();
    auth_cfg.provider.anthropic.count_tokens_enabled.toggle();
    auth_cfg.provider.anthropic.task_budget_tokens = Some(64_000);
    let selection = selection("anthropic", Some(Provider::Anthropic), "ANTHROPIC_API_KEY", None, true);
    let config = runtime_config();

    let provider_config = selected_provider_config(&selection, &auth_cfg, &config, "key", None);
    let anthropic = provider_config
        .anthropic
        .expect("a model switch must pass [provider.anthropic] like the startup client");

    assert_eq!(anthropic.count_tokens_enabled, auth_cfg.provider.anthropic.count_tokens_enabled);
    assert_eq!(anthropic.task_budget_tokens, Some(64_000));
    assert_eq!(provider_config.model.as_deref(), Some("test-model"));
    assert_eq!(provider_config.api_key.as_deref(), Some("key"));
}

#[test]
fn selected_provider_config_applies_claude_speed_and_endpoint_without_changing_openai() {
    let mut auth_cfg = VTCodeConfig::default();
    auth_cfg.provider.openai.service_tier = Some(vtcode_config::OpenAIServiceTier::Ultrafast);
    auth_cfg.provider.anthropic.speed = Some(vtcode_config::AnthropicSpeed::Fast);
    auth_cfg.provider_overrides.entry("anthropic".to_owned()).or_default().base_url =
        Some("https://gateway.example/v1".to_owned());
    let mut selection = selection("anthropic", Some(Provider::Anthropic), "ANTHROPIC_API_KEY", None, true);
    selection.anthropic_speed = Some(vtcode_config::AnthropicSpeed::Standard);
    let provider_config = selected_provider_config(&selection, &auth_cfg, &runtime_config(), "key", None);
    assert_eq!(provider_config.base_url.as_deref(), Some("https://gateway.example/v1"));
    assert_eq!(provider_config.anthropic.unwrap().speed, Some(vtcode_config::AnthropicSpeed::Standard));
    assert_eq!(provider_config.openai.unwrap().service_tier, Some(vtcode_config::OpenAIServiceTier::Ultrafast));
}

#[test]
fn selected_provider_config_applies_picked_openai_service_tier() {
    let auth_cfg = VTCodeConfig::default();
    let mut selection = selection("openai", Some(Provider::OpenAI), "OPENAI_API_KEY", None, true);
    selection.service_tier_supported = true;
    selection.service_tier = Some(vtcode_config::OpenAIServiceTier::Priority);
    let config = runtime_config();

    let provider_config = selected_provider_config(&selection, &auth_cfg, &config, "key", None);

    assert_eq!(
        provider_config.openai.and_then(|openai| openai.service_tier),
        Some(vtcode_config::OpenAIServiceTier::Priority)
    );
}

#[test]
fn best_effort_downgrades_inherited_medium_to_low_on_zai_route() {
    assert_eq!(
        best_effort_reasoning(ReasoningEffortLevel::Medium, &["low", "high", "max"]),
        Some(ReasoningEffortLevel::Low)
    );
}

#[test]
fn best_effort_upgrades_inherited_minimal_to_low_when_no_lower_exists() {
    assert_eq!(
        best_effort_reasoning(ReasoningEffortLevel::Minimal, &["low", "high", "max"]),
        Some(ReasoningEffortLevel::Low)
    );
}

#[test]
fn best_effort_keeps_supported_effort_unchanged() {
    assert_eq!(
        best_effort_reasoning(ReasoningEffortLevel::Max, &["low", "high", "max"]),
        Some(ReasoningEffortLevel::Max)
    );
    assert_eq!(best_effort_reasoning(ReasoningEffortLevel::None, &[]), Some(ReasoningEffortLevel::None));
}

#[test]
fn best_effort_returns_none_when_route_exposes_no_efforts() {
    assert_eq!(best_effort_reasoning(ReasoningEffortLevel::Medium, &[]), None);
}
