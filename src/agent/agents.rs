use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::core::interfaces::{SessionRuntime, SessionRuntimeParams};
use vtcode_core::core::threads::{
    ArchivedSessionIntent, loaded_skills_from_session_listing, messages_from_session_listing,
};
use vtcode_core::llm::provider::Message as ProviderMessage;
use vtcode_core::utils::session_archive::{SessionListing, SessionSnapshot};

#[derive(Clone, Debug)]
pub struct SessionContinuation {
    listing: SessionListing,
    history: Vec<ProviderMessage>,
    loaded_skills: Vec<String>,
    intent: ArchivedSessionIntent,
}

impl SessionContinuation {
    pub fn listing(&self) -> &SessionListing {
        &self.listing
    }

    pub fn identifier(&self) -> String {
        self.listing.identifier()
    }

    pub fn snapshot(&self) -> &SessionSnapshot {
        &self.listing.snapshot
    }

    pub fn path(&self) -> &PathBuf {
        &self.listing.path
    }

    pub fn is_fork(&self) -> bool {
        matches!(self.intent, ArchivedSessionIntent::ForkNewArchive { .. })
    }

    pub fn intent(&self) -> &ArchivedSessionIntent {
        &self.intent
    }

    pub fn custom_suffix(&self) -> Option<&str> {
        match &self.intent {
            ArchivedSessionIntent::ForkNewArchive { custom_suffix } => custom_suffix.as_deref(),
            ArchivedSessionIntent::ResumeInPlace => None,
        }
    }

    pub fn history(&self) -> &[ProviderMessage] {
        &self.history
    }

    pub fn loaded_skills(&self) -> &[String] {
        &self.loaded_skills
    }

    pub fn message_count(&self) -> usize {
        self.history.len()
    }

    pub fn from_listing(listing: &SessionListing, intent: ArchivedSessionIntent) -> Self {
        Self {
            listing: listing.clone(),
            history: messages_from_session_listing(listing),
            loaded_skills: loaded_skills_from_session_listing(listing),
            intent,
        }
    }
}

pub type ResumeSession = SessionContinuation;

pub async fn load_resume_session(
    identifier: &str,
    intent: ArchivedSessionIntent,
) -> Result<Option<SessionContinuation>> {
    let Some(listing) =
        vtcode_core::utils::session_archive::find_session_by_identifier(identifier).await?
    else {
        return Ok(None);
    };
    Ok(Some(SessionContinuation::from_listing(&listing, intent)))
}

pub async fn run_single_agent_loop(
    config: &CoreAgentConfig,
    initial_vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    plan_mode: bool,
    resume: Option<SessionContinuation>,
) -> Result<()> {
    let vt_cfg = prepare_session_vt_config(initial_vt_cfg, config);

    let runtime = crate::agent::runloop::unified::UnifiedSessionRuntime;
    let mut steering_receiver = None;
    let params = SessionRuntimeParams::new(
        config,
        vt_cfg,
        skip_confirmations,
        full_auto,
        plan_mode,
        resume,
        &mut steering_receiver,
    );
    runtime.run_session(params).await
}

fn prepare_session_vt_config(
    initial_vt_cfg: Option<VTCodeConfig>,
    runtime_cfg: &CoreAgentConfig,
) -> Option<VTCodeConfig> {
    let mut vt_cfg = initial_vt_cfg.or_else(|| {
        ConfigManager::load_from_workspace(&runtime_cfg.workspace)
            .ok()
            .map(|manager| manager.config().clone())
    });

    apply_runtime_overrides(vt_cfg.as_mut(), runtime_cfg);
    vt_cfg
}

pub fn apply_runtime_overrides(vt_cfg: Option<&mut VTCodeConfig>, runtime_cfg: &CoreAgentConfig) {
    if let Some(cfg) = vt_cfg {
        cfg.agent.provider = runtime_cfg.provider.clone();

        if matches!(runtime_cfg.model_source, ModelSelectionSource::CliOverride) {
            cfg.agent.default_model = runtime_cfg.model.clone();
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

    #[test]
    fn initial_session_config_is_preserved() {
        let mut initial_vt_cfg = VTCodeConfig::default();
        initial_vt_cfg.agent.default_model = "startup-model".to_string();
        initial_vt_cfg.agent.provider = "startup-provider".to_string();

        let current_dir = std::env::current_dir().unwrap();
        let runtime_cfg = CoreAgentConfig {
            model: "startup-model".to_string(),
            api_key: String::new(),
            provider: "startup-provider".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: current_dir,
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

        let prepared = prepare_session_vt_config(Some(initial_vt_cfg), &runtime_cfg)
            .expect("startup config should be preserved");

        assert_eq!(prepared.agent.default_model, "startup-model");
        assert_eq!(prepared.agent.provider, "startup-provider");
    }
}
