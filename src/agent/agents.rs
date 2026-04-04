use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::config::ReasoningEffortLevel;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_core::core::interfaces::{SessionRuntime, SessionRuntimeParams};
use vtcode_core::core::threads::{
    ArchivedSessionIntent, ThreadBootstrap, loaded_skills_from_session_listing,
    messages_from_session_listing,
};
use vtcode_core::llm::provider::Message as ProviderMessage;
use vtcode_core::utils::session_archive::SessionArchiveMetadata;
use vtcode_core::utils::session_archive::{SessionListing, SessionSnapshot};
use vtcode_core::utils::terminal_color_probe::probe_and_cache_terminal_palette_harmony;

#[derive(Clone, Debug)]
pub(crate) struct SessionContinuation {
    listing: SessionListing,
    bootstrap: ThreadBootstrap,
    history: Vec<ProviderMessage>,
    loaded_skills: Vec<String>,
    intent: ArchivedSessionIntent,
    thread_label: String,
    root_thread: bool,
    vt_cfg_override: Option<VTCodeConfig>,
}

impl SessionContinuation {
    pub(crate) fn listing(&self) -> &SessionListing {
        &self.listing
    }

    pub(crate) fn identifier(&self) -> String {
        self.listing.identifier()
    }

    pub(crate) fn snapshot(&self) -> &SessionSnapshot {
        &self.listing.snapshot
    }

    pub(crate) fn path(&self) -> &PathBuf {
        &self.listing.path
    }

    pub(crate) fn is_fork(&self) -> bool {
        matches!(self.intent, ArchivedSessionIntent::ForkNewArchive { .. })
    }

    pub(crate) fn intent(&self) -> &ArchivedSessionIntent {
        &self.intent
    }

    pub(crate) fn bootstrap(&self) -> &ThreadBootstrap {
        &self.bootstrap
    }

    pub(crate) fn custom_suffix(&self) -> Option<&str> {
        match &self.intent {
            ArchivedSessionIntent::ForkNewArchive { custom_suffix, .. } => custom_suffix.as_deref(),
            ArchivedSessionIntent::ResumeInPlace => None,
        }
    }

    pub(crate) fn summarize_fork(&self) -> bool {
        match &self.intent {
            ArchivedSessionIntent::ForkNewArchive { summarize, .. } => *summarize,
            ArchivedSessionIntent::ResumeInPlace => false,
        }
    }

    pub(crate) fn history(&self) -> &[ProviderMessage] {
        &self.history
    }

    pub(crate) fn loaded_skills(&self) -> &[String] {
        &self.loaded_skills
    }

    pub(crate) fn message_count(&self) -> usize {
        self.history.len()
    }

    pub(crate) fn thread_label(&self) -> &str {
        &self.thread_label
    }

    pub(crate) fn is_root_thread(&self) -> bool {
        self.root_thread
    }

    pub(crate) fn vt_cfg_override(&self) -> Option<&VTCodeConfig> {
        self.vt_cfg_override.as_ref()
    }
    pub(crate) fn from_listing(listing: &SessionListing, intent: ArchivedSessionIntent) -> Self {
        Self {
            listing: listing.clone(),
            bootstrap: ThreadBootstrap::from_listing(listing.clone()),
            history: messages_from_session_listing(listing),
            loaded_skills: loaded_skills_from_session_listing(listing),
            intent,
            thread_label: "main".to_string(),
            root_thread: true,
            vt_cfg_override: None,
        }
    }
}

pub(crate) type ResumeSession = SessionContinuation;

pub(crate) async fn load_resume_session(
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

pub(crate) async fn run_single_agent_loop(
    config: &CoreAgentConfig,
    initial_vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    plan_mode_entry_source: PlanModeEntrySource,
    resume: Option<SessionContinuation>,
) -> Result<()> {
    // Probe terminal palette only for real interactive sessions so one-shot CLI
    // commands never emit OSC queries back into the user's shell.
    probe_and_cache_terminal_palette_harmony();

    let mut runtime_cfg = config.clone();
    if let Some(resume_session) = resume.as_ref() {
        apply_resume_runtime_overrides(&mut runtime_cfg, resume_session);
    }
    let vt_cfg = prepare_session_vt_config(initial_vt_cfg, &runtime_cfg, resume.as_ref());

    let runtime = if runtime_cfg
        .provider
        .eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER)
    {
        EitherRuntime::Codex(crate::codex_app_server::CodexSessionRuntime)
    } else {
        EitherRuntime::Unified(crate::agent::runloop::unified::UnifiedSessionRuntime)
    };
    let mut steering_receiver = None;
    let params = SessionRuntimeParams::new(
        &runtime_cfg,
        vt_cfg,
        skip_confirmations,
        full_auto,
        plan_mode_entry_source,
        resume,
        &mut steering_receiver,
    );
    runtime.run_session(params).await
}

enum EitherRuntime {
    Unified(crate::agent::runloop::unified::UnifiedSessionRuntime),
    Codex(crate::codex_app_server::CodexSessionRuntime),
}

impl EitherRuntime {
    async fn run_session(self, params: SessionRuntimeParams<'_, ResumeSession>) -> Result<()> {
        match self {
            Self::Unified(runtime) => runtime.run_session(params).await,
            Self::Codex(runtime) => runtime.run_session(params).await,
        }
    }
}

fn prepare_session_vt_config(
    initial_vt_cfg: Option<VTCodeConfig>,
    runtime_cfg: &CoreAgentConfig,
    resume: Option<&SessionContinuation>,
) -> Option<VTCodeConfig> {
    let mut vt_cfg = resume
        .and_then(|session| session.vt_cfg_override().cloned())
        .or(initial_vt_cfg)
        .or_else(|| {
            ConfigManager::load_from_workspace(&runtime_cfg.workspace)
                .ok()
                .map(|manager| manager.config().clone())
        });

    apply_runtime_overrides(vt_cfg.as_mut(), runtime_cfg);
    vt_cfg
}

fn apply_resume_runtime_overrides(runtime_cfg: &mut CoreAgentConfig, resume: &SessionContinuation) {
    apply_persisted_resume_metadata(runtime_cfg, Some(&resume.snapshot().metadata));
}

pub(crate) fn apply_runtime_overrides(
    vt_cfg: Option<&mut VTCodeConfig>,
    runtime_cfg: &CoreAgentConfig,
) {
    if let Some(cfg) = vt_cfg {
        cfg.agent.provider = runtime_cfg.provider.clone();
        cfg.agent.reasoning_effort = runtime_cfg.reasoning_effort;
        cfg.agent.theme = runtime_cfg.theme.clone();

        if matches!(runtime_cfg.model_source, ModelSelectionSource::CliOverride) {
            cfg.agent.default_model = runtime_cfg.model.clone();
        }
    }
}

pub(crate) fn apply_persisted_resume_metadata(
    runtime_cfg: &mut CoreAgentConfig,
    metadata: Option<&SessionArchiveMetadata>,
) {
    let Some(metadata) = metadata else {
        return;
    };

    if matches!(runtime_cfg.model_source, ModelSelectionSource::CliOverride) {
        return;
    }

    let persisted_model = metadata.model.trim();
    if persisted_model.is_empty() {
        return;
    }

    runtime_cfg.model = persisted_model.to_owned();
    if !metadata.provider.trim().is_empty() {
        runtime_cfg.provider = metadata.provider.clone();
    }
    if let Some(reasoning_effort) = ReasoningEffortLevel::parse(&metadata.reasoning_effort) {
        runtime_cfg.reasoning_effort = reasoning_effort;
    }
    runtime_cfg.model_source = ModelSelectionSource::CliOverride;
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
            openai_chatgpt_auth: None,
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
            openai_chatgpt_auth: None,
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
            openai_chatgpt_auth: None,
        };

        let prepared = prepare_session_vt_config(Some(initial_vt_cfg), &runtime_cfg, None)
            .expect("startup config should be preserved");

        assert_eq!(prepared.agent.default_model, "startup-model");
        assert_eq!(prepared.agent.provider, "startup-provider");
    }

    #[test]
    fn persisted_resume_metadata_reuses_model_and_reasoning_effort() {
        let mut runtime_cfg = CoreAgentConfig {
            model: "current-model".to_string(),
            api_key: String::new(),
            provider: "current-provider".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: std::env::current_dir().unwrap(),
            verbose: false,
            quiet: false,
            theme: String::new(),
            reasoning_effort: ReasoningEffortLevel::Low,
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
            openai_chatgpt_auth: None,
        };
        let metadata = SessionArchiveMetadata::new(
            "workspace",
            "/tmp/workspace",
            "persisted-model",
            "persisted-provider",
            "theme",
            "high",
        );

        apply_persisted_resume_metadata(&mut runtime_cfg, Some(&metadata));

        assert_eq!(runtime_cfg.model, "persisted-model");
        assert_eq!(runtime_cfg.provider, "persisted-provider");
        assert_eq!(runtime_cfg.reasoning_effort, ReasoningEffortLevel::High);
        assert_eq!(runtime_cfg.model_source, ModelSelectionSource::CliOverride);
    }

    #[test]
    fn persisted_resume_metadata_does_not_override_explicit_model_selection() {
        let mut runtime_cfg = CoreAgentConfig {
            model: "explicit-model".to_string(),
            api_key: String::new(),
            provider: "explicit-provider".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: std::env::current_dir().unwrap(),
            verbose: false,
            quiet: false,
            theme: String::new(),
            reasoning_effort: ReasoningEffortLevel::Low,
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
            openai_chatgpt_auth: None,
        };
        let metadata = SessionArchiveMetadata::new(
            "workspace",
            "/tmp/workspace",
            "persisted-model",
            "persisted-provider",
            "theme",
            "high",
        );

        apply_persisted_resume_metadata(&mut runtime_cfg, Some(&metadata));

        assert_eq!(runtime_cfg.model, "explicit-model");
        assert_eq!(runtime_cfg.provider, "explicit-provider");
        assert_eq!(runtime_cfg.reasoning_effort, ReasoningEffortLevel::Low);
        assert_eq!(runtime_cfg.model_source, ModelSelectionSource::CliOverride);
    }
}
