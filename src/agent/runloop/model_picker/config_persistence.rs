use anyhow::{Context, Result};

use vtcode_config::auth::CustomApiKeyStorage;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::utils::dot_config::update_model_preference;

use super::ModelSelectionResult;

fn synced_openai_service_tier(
    selection: &ModelSelectionResult,
) -> Option<vtcode_config::OpenAIServiceTier> {
    (selection.provider_enum == Some(Provider::OpenAI) && selection.service_tier_supported)
        .then_some(selection.service_tier)
        .flatten()
}

pub(super) async fn persist_selection(
    workspace: &std::path::Path,
    selection: &ModelSelectionResult,
) -> Result<VTCodeConfig> {
    let mut manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "Failed to load vtcode configuration for workspace {}",
            workspace.display()
        )
    })?;
    let mut config = manager.config().clone();
    config.agent.provider = selection.provider.clone();
    apply_api_key_state(&mut config, selection);
    config.agent.default_model = selection.model.clone();
    config.agent.reasoning_effort = selection.reasoning;
    config.provider.openai.service_tier = synced_openai_service_tier(selection);

    manager.save_config(&config)?;
    update_model_preference(&selection.provider, &selection.model)
        .await
        .ok();
    Ok(config)
}

fn apply_api_key_state(config: &mut VTCodeConfig, selection: &ModelSelectionResult) {
    if selection.provider_enum == Some(Provider::OpenAI) && selection.uses_chatgpt_auth {
        config.agent.api_key_env = selection.env_key.clone();
        config.agent.custom_api_keys.remove(&selection.provider);
        return;
    }

    if uses_provider_api_key(selection) {
        config.agent.api_key_env = selection.env_key.clone();
        sync_stored_api_key(config, selection);
        return;
    }

    config.agent.api_key_env.clear();
    clear_stored_api_key(config, &selection.provider);
}

fn uses_provider_api_key(selection: &ModelSelectionResult) -> bool {
    selection.provider_enum != Some(Provider::Ollama) || is_cloud_ollama_model(&selection.model)
}

fn is_cloud_ollama_model(model: &str) -> bool {
    model.contains(":cloud") || model.contains("-cloud")
}

fn sync_stored_api_key(config: &mut VTCodeConfig, selection: &ModelSelectionResult) {
    if selection.provider_enum == Some(Provider::OpenAI) && selection.uses_chatgpt_auth {
        return;
    }

    if let Some(api_key) = selection.api_key.as_deref() {
        let storage_mode = config.agent.credential_storage_mode;
        let key_storage = CustomApiKeyStorage::new(&selection.provider);
        if let Err(err) = key_storage.store(api_key, storage_mode) {
            tracing::warn!(
                "Failed to store API key for provider '{}' securely: {}",
                selection.provider,
                err
            );
        }
        config
            .agent
            .custom_api_keys
            .insert(selection.provider.clone(), String::new());
        return;
    }

    clear_stored_api_key(config, &selection.provider);
}

fn clear_stored_api_key(config: &mut VTCodeConfig, provider: &str) {
    config.agent.custom_api_keys.remove(provider);
    let storage_mode = config.agent.credential_storage_mode;
    let key_storage = CustomApiKeyStorage::new(provider);
    let _ = key_storage.clear(storage_mode);
}

#[cfg(test)]
mod tests {
    use super::{is_cloud_ollama_model, synced_openai_service_tier, uses_provider_api_key};
    use crate::agent::runloop::model_picker::ModelSelectionResult;
    use vtcode_config::OpenAIServiceTier;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::ReasoningEffortLevel;

    fn selection(
        provider_enum: Option<Provider>,
        provider: &str,
        model: &str,
    ) -> ModelSelectionResult {
        ModelSelectionResult {
            provider: provider.to_string(),
            provider_label: provider.to_string(),
            provider_enum,
            model: model.to_string(),
            model_display: model.to_string(),
            known_model: false,
            reasoning_supported: false,
            reasoning: ReasoningEffortLevel::Medium,
            reasoning_changed: false,
            service_tier_supported: false,
            service_tier: None,
            service_tier_changed: false,
            api_key: None,
            env_key: "TEST_API_KEY".to_string(),
            requires_api_key: false,
            uses_chatgpt_auth: false,
        }
    }

    #[test]
    fn detects_cloud_ollama_models() {
        assert!(is_cloud_ollama_model("llama3:cloud"));
        assert!(is_cloud_ollama_model("deepseek-cloud"));
        assert!(!is_cloud_ollama_model("llama3"));
    }

    #[test]
    fn local_ollama_models_skip_provider_api_key_state() {
        assert!(!uses_provider_api_key(&selection(
            Some(Provider::Ollama),
            "ollama",
            "qwen3-coder"
        )));
    }

    #[test]
    fn cloud_ollama_models_keep_provider_api_key_state() {
        assert!(uses_provider_api_key(&selection(
            Some(Provider::Ollama),
            "ollama",
            "qwen3-coder:cloud"
        )));
    }

    #[test]
    fn non_ollama_providers_keep_provider_api_key_state() {
        assert!(uses_provider_api_key(&selection(
            Some(Provider::OpenAI),
            "openai",
            "gpt-5.2"
        )));
    }

    #[test]
    fn synced_openai_service_tier_tracks_supported_openai_selection() {
        let mut selected = selection(Some(Provider::OpenAI), "openai", "gpt-5.4");
        selected.service_tier_supported = true;
        selected.service_tier = Some(OpenAIServiceTier::Priority);

        assert_eq!(
            synced_openai_service_tier(&selected),
            Some(OpenAIServiceTier::Priority)
        );
    }

    #[test]
    fn synced_openai_service_tier_clears_stale_values_outside_supported_openai() {
        let mut selected = selection(Some(Provider::Ollama), "ollama", "qwen3-coder");
        selected.service_tier_supported = true;
        selected.service_tier = Some(OpenAIServiceTier::Priority);

        assert_eq!(synced_openai_service_tier(&selected), None);

        let mut unsupported_openai = selection(Some(Provider::OpenAI), "openai", "gpt-oss-20b");
        unsupported_openai.service_tier_supported = false;
        unsupported_openai.service_tier = Some(OpenAIServiceTier::Priority);

        assert_eq!(synced_openai_service_tier(&unsupported_openai), None);
    }
}
