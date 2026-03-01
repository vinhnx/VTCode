use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::core::interfaces::turn::{TurnDriver, TurnDriverParams};
use vtcode_core::llm::provider::Message as ProviderMessage;
use vtcode_core::utils::session_archive::{SessionListing, SessionSnapshot};

#[derive(Clone, Debug)]
pub struct ResumeSession {
    pub identifier: String,
    pub snapshot: SessionSnapshot,
    pub history: Vec<ProviderMessage>,
    pub path: PathBuf,
    pub is_fork: bool,
}

impl ResumeSession {
    pub fn message_count(&self) -> usize {
        self.history.len()
    }

    pub fn from_listing(listing: &SessionListing, is_fork: bool) -> Self {
        let history_source = if !listing.snapshot.messages.is_empty() {
            listing.snapshot.messages.iter()
        } else if let Some(progress) = &listing.snapshot.progress {
            progress.recent_messages.iter()
        } else {
            [].iter()
        };

        let history = history_source.map(ProviderMessage::from).collect();

        Self {
            identifier: listing.identifier(),
            snapshot: listing.snapshot.clone(),
            history,
            path: listing.path.clone(),
            is_fork,
        }
    }
}

pub async fn run_single_agent_loop(
    config: &CoreAgentConfig,
    skip_confirmations: bool,
    full_auto: bool,
    plan_mode: bool,
    team_context: Option<vtcode_core::agent_teams::TeamContext>,
    resume: Option<ResumeSession>,
) -> Result<()> {
    // Cache the workspace path to avoid repeated current_dir calls
    let workspace_path = &config.workspace;

    // Load configuration once and cache it
    let mut vt_cfg = ConfigManager::load_from_workspace(workspace_path)
        .ok()
        .map(|manager| manager.config().clone());

    apply_runtime_overrides(vt_cfg.as_mut(), config);

    let driver = crate::agent::runloop::unified::UnifiedTurnDriver;
    let mut steering_receiver = None;
    let params = TurnDriverParams::new(
        config,
        vt_cfg,
        skip_confirmations,
        full_auto,
        plan_mode,
        team_context,
        resume,
        &mut steering_receiver,
    );
    driver.drive_turn(params).await
}

pub fn apply_runtime_overrides(vt_cfg: Option<&mut VTCodeConfig>, runtime_cfg: &CoreAgentConfig) {
    if let Some(cfg) = vt_cfg {
        cfg.agent.provider = runtime_cfg.provider.clone();

        if matches!(runtime_cfg.model_source, ModelSelectionSource::CliOverride) {
            let override_model = runtime_cfg.model.clone();
            cfg.agent.default_model = override_model.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{ReasoningEffortLevel, UiSurfacePreference};
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

    #[test]
    fn cli_model_override_updates_default_model() {
        const OVERRIDE_MODEL: &str = "override-model";

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.default_model = "config-model".to_string();

        // Cache the current directory to avoid repeated calls
        let current_dir = std::env::current_dir().unwrap();
        let runtime_cfg = CoreAgentConfig {
            model: OVERRIDE_MODEL.to_string(),
            api_key: String::new(),
            provider: "cli-provider".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: current_dir.clone(),
            verbose: false,
            quiet: false,
            theme: String::new(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::CliOverride,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
        };

        apply_runtime_overrides(Some(&mut vt_cfg), &runtime_cfg);

        assert_eq!(vt_cfg.agent.default_model, OVERRIDE_MODEL);
        assert_eq!(vt_cfg.agent.provider, "cli-provider");
    }

    #[test]
    fn workspace_config_preserves_default_model() {
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.default_model = "config-model".to_string();

        // Cache the current directory to avoid repeated calls
        let current_dir = std::env::current_dir().unwrap();
        let runtime_cfg = CoreAgentConfig {
            model: "config-standard".to_string(),
            api_key: String::new(),
            provider: "config-provider".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: current_dir.clone(),
            verbose: false,
            quiet: false,
            theme: String::new(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
        };

        apply_runtime_overrides(Some(&mut vt_cfg), &runtime_cfg);

        assert_eq!(vt_cfg.agent.default_model, "config-model");
        assert_eq!(vt_cfg.agent.provider, "config-provider");
    }
}
