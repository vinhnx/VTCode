//! Persist desktop fast-mode changes through the CLI's layered config writer.
use std::path::PathBuf;

use anyhow::{Context, Result};
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::types::AgentConfig;

use crate::agent::runloop::unified::fast_mode::set_fast_mode_preference;

pub(super) async fn persist(workspace: PathBuf, agent: AgentConfig, enabled: bool) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut manager = ConfigManager::load_from_workspace(&workspace)
            .context("failed to load CLI config for fast-mode synchronization")?;
        let mut config = manager.config().clone();
        set_fast_mode_preference(&agent, &mut config, enabled)?;
        manager
            .save_config(&config)
            .context("failed to save fast-mode preference to CLI configuration")
    })
    .await
    .context("CLI fast-mode persistence task failed")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn desktop_fast_mode_uses_cli_config_writer() {
        let directory = tempfile::tempdir().expect("temporary workspace");
        let mut agent = AgentConfig {
            model: "gpt-6.1-sol".into(),
            api_key: "test-key".into(),
            provider: "openai".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            workspace: directory.path().to_path_buf(),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.into(),
            reasoning_effort: vtcode_config::types::ReasoningEffortLevel::default(),
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
        };

        persist(directory.path().to_path_buf(), agent.clone(), true)
            .await
            .expect("save eligible CLI fast-mode preference");

        let manager = ConfigManager::load_from_workspace(directory.path()).expect("load saved CLI config");
        assert_eq!(manager.config().provider.openai.service_tier, Some(vtcode_config::OpenAIServiceTier::Ultrafast));
        persist(directory.path().to_path_buf(), agent.clone(), false)
            .await
            .expect("disable fast processing");
        let manager = ConfigManager::load_from_workspace(directory.path()).expect("reload saved CLI config");
        assert_eq!(manager.config().provider.openai.service_tier, None);

        agent.model = "gpt-5.6-sol".into();
        assert!(persist(directory.path().to_path_buf(), agent, true).await.is_err());
        let manager = ConfigManager::load_from_workspace(directory.path()).expect("reload after rejected route");
        assert_eq!(manager.config().provider.openai.service_tier, None);
    }
}
