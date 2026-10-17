use anyhow::Result;

use vtcode_config::api_keys::{clear_credential_with_mode, credential_metadata_key, store_credential_with_mode};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::utils::dot_config::update_model_preference;

use super::ModelSelectionResult;

fn synced_openai_service_tier(selection: &ModelSelectionResult) -> Option<vtcode_config::OpenAIServiceTier> {
    selection.service_tier_supported.then_some(selection.service_tier).flatten()
}

pub(super) async fn persist_selection(
    workspace: &std::path::Path,
    selection: &ModelSelectionResult,
) -> Result<VTCodeConfig> {
    let mut manager = crate::main_helpers::load_workspace_config(workspace)?;
    let config = manager.config();
    if !config.providers_whitelist.is_empty()
        && !config
            .providers_whitelist
            .iter()
            .any(|w| w.eq_ignore_ascii_case(&selection.provider))
    {
        anyhow::bail!("Cannot persist selection: provider '{}' is not in providers_whitelist", selection.provider);
    }

    selection.validate_speed(config)?;
    let mut config = config.clone();
    config.agent.provider = selection.provider.clone();
    apply_api_key_state(&mut config, selection)?;
    config.agent.default_model = selection.model.clone();
    config.agent.reasoning_effort = selection.reasoning;
    apply_provider_preferences(&mut config, selection);
    if let Some(enabled) = selection.decisions_probe {
        config.permissions.auto_permission.use_decisions_probe = enabled;
    }

    // Store MiMo auth method when provider is MiMo
    if selection.provider_enum == Some(Provider::MiMo) {
        config.provider.mimo_auth_method = selection.mimo_auth_method;
    }

    manager.save_config(&config)?;
    update_model_preference(&selection.provider, &selection.model).await.ok();
    Ok(config)
}

fn apply_provider_preferences(config: &mut VTCodeConfig, selection: &ModelSelectionResult) {
    if selection.service_tier_supported {
        config.provider.openai.service_tier = synced_openai_service_tier(selection);
    }
    if let Some(speed) = selection.anthropic_speed {
        config.provider.anthropic.speed = Some(speed);
    }
}

fn apply_api_key_state(config: &mut VTCodeConfig, selection: &ModelSelectionResult) -> Result<()> {
    if selection.provider_enum == Some(Provider::OpenAI) && selection.uses_chatgpt_auth {
        config.agent.api_key_env = selection.env_key.clone();
        config.agent.custom_api_keys.remove(&selection.provider);
        if let Some(metadata_key) = credential_metadata_key(&selection.provider, &selection.env_key)? {
            config.agent.custom_api_keys.remove(&metadata_key);
        }
        return Ok(());
    }

    if uses_provider_api_key(selection) {
        config.agent.api_key_env = selection.env_key.clone();
        if selection.api_key.is_some() && selection.credential_source.is_none() {
            sync_stored_api_key(config, selection)?;
        }
        return Ok(());
    }

    config.agent.api_key_env.clear();
    if selection.api_key.is_some() && selection.credential_source.is_none() {
        clear_stored_api_key(config, selection)?;
    }
    Ok(())
}

fn uses_provider_api_key(selection: &ModelSelectionResult) -> bool {
    if selection.provider_enum.is_some_and(|provider| provider.uses_managed_auth()) {
        return false;
    }

    match selection.provider_enum {
        Some(Provider::OllamaCloud) => true,
        Some(Provider::LmStudio | Provider::LlamaCpp | Provider::Ollama) => false,
        _ => true,
    }
}

fn sync_stored_api_key(config: &mut VTCodeConfig, selection: &ModelSelectionResult) -> Result<()> {
    if selection.provider_enum == Some(Provider::OpenAI) && selection.uses_chatgpt_auth {
        return Ok(());
    }

    if let Some(api_key) = selection.api_key.as_deref() {
        let storage_mode = config.agent.credential_storage_mode;
        store_credential_with_mode(&selection.provider, &selection.env_key, api_key, storage_mode)?;
        config.agent.custom_api_keys.remove(&selection.provider);
        if let Some(metadata_key) = credential_metadata_key(&selection.provider, &selection.env_key)? {
            config.agent.custom_api_keys.insert(metadata_key, String::new());
        }
        return Ok(());
    }

    clear_stored_api_key(config, selection)
}

fn clear_stored_api_key(config: &mut VTCodeConfig, selection: &ModelSelectionResult) -> Result<()> {
    config.agent.custom_api_keys.remove(&selection.provider);
    if let Some(metadata_key) = credential_metadata_key(&selection.provider, &selection.env_key)? {
        config.agent.custom_api_keys.remove(&metadata_key);
    }
    let storage_mode = config.agent.credential_storage_mode;
    clear_credential_with_mode(&selection.provider, &selection.env_key, storage_mode)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{synced_openai_service_tier, uses_provider_api_key};
    use crate::agent::runloop::model_picker::ModelSelectionResult;
    use vtcode_config::OpenAIServiceTier;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::ReasoningEffortLevel;

    fn selection(provider_enum: Option<Provider>, provider: &str, model: &str) -> ModelSelectionResult {
        ModelSelectionResult {
            provider: provider.to_string(),
            provider_label: provider.to_string(),
            provider_enum,
            model: model.to_string(),
            model_display: model.to_string(),
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
            api_key: None,
            credential_source: None,
            env_key: "TEST_API_KEY".to_string(),
            requires_api_key: false,
            uses_chatgpt_auth: false,
            mimo_auth_method: None,
        }
    }

    #[test]
    fn provider_preferences_remain_independent_across_switches_and_restart() {
        let mut config = vtcode_config::VTCodeConfig::default();
        config.provider.openai.service_tier = Some(OpenAIServiceTier::Ultrafast);
        config.provider.anthropic.speed = Some(vtcode_config::AnthropicSpeed::Fast);
        let mut claude = selection(Some(Provider::Anthropic), "anthropic", "claude-opus-5-5");
        claude.anthropic_speed = Some(vtcode_config::AnthropicSpeed::Standard);
        super::apply_provider_preferences(&mut config, &claude);
        assert_eq!(config.provider.openai.service_tier, Some(OpenAIServiceTier::Ultrafast));
        assert_eq!(config.provider.anthropic.speed, Some(vtcode_config::AnthropicSpeed::Standard));
        let unsupported = selection(Some(Provider::Anthropic), "anthropic", "claude-sonnet-5-5");
        super::apply_provider_preferences(&mut config, &unsupported);
        let mut openai = selection(Some(Provider::OpenAI), "openai", "gpt-6.1-sol");
        openai.service_tier_supported = true;
        openai.service_tier = None;
        super::apply_provider_preferences(&mut config, &openai);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vtcode.toml");
        vtcode_config::ConfigManager::save_config_to_path(&path, &config).unwrap();
        let reloaded = vtcode_config::ConfigManager::load_from_file(&path).unwrap();
        assert_eq!(reloaded.config().provider.anthropic.speed, Some(vtcode_config::AnthropicSpeed::Standard));
        assert_eq!(reloaded.config().provider.openai.service_tier, None);
    }

    #[test]
    fn local_ollama_models_skip_provider_api_key_state() {
        assert!(!uses_provider_api_key(&selection(Some(Provider::Ollama), "ollama", "qwen3-coder")));
    }

    #[test]
    fn ollama_cloud_models_keep_provider_api_key_state() {
        assert!(uses_provider_api_key(&selection(Some(Provider::OllamaCloud), "ollama-cloud", "qwen3-coder:cloud")));
    }

    #[test]
    fn non_ollama_providers_keep_provider_api_key_state() {
        assert!(uses_provider_api_key(&selection(Some(Provider::OpenAI), "openai", "gpt-5.6")));
    }

    #[test]
    fn managed_auth_providers_skip_provider_api_key_state() {
        assert!(!uses_provider_api_key(&selection(
            Some(Provider::Copilot),
            "copilot",
            vtcode_core::config::constants::models::copilot::DEFAULT_MODEL
        )));
    }

    #[test]
    fn synced_openai_service_tier_tracks_supported_openai_selection() {
        let mut selected = selection(Some(Provider::OpenAI), "openai", "gpt-5.6-sol");
        selected.service_tier_supported = true;
        selected.service_tier = Some(OpenAIServiceTier::Priority);

        assert_eq!(synced_openai_service_tier(&selected), Some(OpenAIServiceTier::Priority));
    }

    #[test]
    fn synced_openai_service_tier_tracks_flex_selection() {
        let mut selected = selection(Some(Provider::OpenAI), "openai", "gpt-5.6-sol");
        selected.service_tier_supported = true;
        selected.service_tier = Some(OpenAIServiceTier::Flex);

        assert_eq!(synced_openai_service_tier(&selected), Some(OpenAIServiceTier::Flex));
    }

    #[test]
    fn synced_openai_service_tier_clears_stale_values_outside_supported_openai() {
        // Compat providers now share the canonical `provider.openai.service_tier`
        // store: any selection whose picker flagged tier support persists it.
        let mut compat = selection(Some(Provider::Ollama), "ollama", "qwen3-coder");
        compat.service_tier_supported = true;
        compat.service_tier = Some(OpenAIServiceTier::Priority);

        assert_eq!(synced_openai_service_tier(&compat), Some(OpenAIServiceTier::Priority));

        let mut unsupported_openai = selection(Some(Provider::OpenAI), "openai", "gpt-oss-20b");
        unsupported_openai.service_tier_supported = false;
        unsupported_openai.service_tier = Some(OpenAIServiceTier::Priority);

        assert_eq!(synced_openai_service_tier(&unsupported_openai), None);

        let mut unsupported_native = selection(Some(Provider::Anthropic), "anthropic", "claude-sonnet-5");
        unsupported_native.service_tier_supported = false;
        unsupported_native.service_tier = Some(OpenAIServiceTier::Priority);

        assert_eq!(synced_openai_service_tier(&unsupported_native), None);
    }
}
