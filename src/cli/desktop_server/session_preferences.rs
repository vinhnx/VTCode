//! Persist desktop runtime preference changes (mode, fast-mode, reasoning
//! effort, model selection) through the CLI's layered config writer.
use std::path::PathBuf;

use anyhow::{Context, Result};
use vtcode_config::types::ReasoningEffortLevel;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::types::AgentConfig;

use crate::agent::runloop::unified::fast_mode::set_fast_mode_preference;

pub(super) async fn persist(
    workspace: PathBuf,
    agent: AgentConfig,
    fast_mode: Option<bool>,
    mode: Option<String>,
    effort: Option<ReasoningEffortLevel>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut manager = ConfigManager::load_from_workspace(&workspace)
            .context("failed to load CLI config for runtime preference synchronization")?;
        let mut config = manager.config().clone();
        if let Some(enabled) = fast_mode {
            set_fast_mode_preference(&agent, &mut config, enabled)?;
        }
        if let Some(mode) = mode {
            config.default_primary_agent = mode;
        }
        if let Some(effort) = effort {
            config.agent.reasoning_effort = effort;
        }
        manager
            .save_config(&config)
            .context("failed to save runtime preferences to CLI configuration")
    })
    .await
    .context("CLI runtime preference persistence task failed")?
}

/// Persist a desktop provider/model selection (CLI
/// `model_picker::config_persistence::persist_selection` parity:
/// `agent.provider`, `agent.default_model`, `agent.reasoning_effort`) so the
/// choice survives service restarts instead of living as a memory-only
/// override. A `None` effort leaves the stored effort untouched: the new
/// route supports nothing mappable, so there is nothing to persist.
pub(super) async fn persist_model_selection(
    workspace: PathBuf,
    provider: String,
    model: String,
    effort: Option<ReasoningEffortLevel>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut manager = ConfigManager::load_from_workspace(&workspace)
            .context("failed to load CLI config for model selection synchronization")?;
        let config = manager.config();
        crate::agent::runloop::ensure_provider_whitelisted(config, &provider)?;
        let mut config = config.clone();
        config.agent.provider = provider;
        config.agent.default_model = model;
        if let Some(effort) = effort {
            config.agent.reasoning_effort = effort;
        }
        manager
            .save_config(&config)
            .context("failed to save model selection to CLI configuration")
    })
    .await
    .context("CLI model selection persistence task failed")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn desktop_fast_mode_uses_cli_config_writer() {
        let directory = super::super::tests::hermetic_workspace();
        let mut agent = AgentConfig {
            model: "gpt-6.1-sol".into(),
            api_key: "test-key".into(),
            provider: "openai".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            workspace: directory.path().to_path_buf(),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.into(),
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
        };

        persist(directory.path().to_path_buf(), agent.clone(), Some(true), Some("duck".into()), None)
            .await
            .expect("save eligible CLI fast-mode preference");

        let manager = ConfigManager::load_from_workspace(directory.path()).expect("load saved CLI config");
        assert_eq!(manager.config().provider.openai.service_tier, Some(vtcode_config::OpenAIServiceTier::Ultrafast));
        assert_eq!(manager.config().default_primary_agent, "duck");
        persist(
            directory.path().to_path_buf(),
            agent.clone(),
            Some(false),
            Some("build".into()),
            Some(ReasoningEffortLevel::High),
        )
        .await
        .expect("disable fast processing");
        let manager = ConfigManager::load_from_workspace(directory.path()).expect("reload saved CLI config");
        assert_eq!(manager.config().provider.openai.service_tier, None);
        assert_eq!(manager.config().default_primary_agent, "build");
        assert_eq!(manager.config().agent.reasoning_effort, ReasoningEffortLevel::High);

        agent.model = "gpt-5.6-sol".into();
        assert!(
            persist(directory.path().to_path_buf(), agent, Some(true), None, None)
                .await
                .is_err()
        );
        let manager = ConfigManager::load_from_workspace(directory.path()).expect("reload after rejected route");
        assert_eq!(manager.config().provider.openai.service_tier, None);
    }
}
