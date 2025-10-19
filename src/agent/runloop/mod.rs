use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::core::interfaces::turn::{TurnDriver, TurnDriverParams};
use vtcode_core::llm::provider::Message as ProviderMessage;
use vtcode_core::utils::session_archive::SessionSnapshot;

mod context;
mod git;
mod mcp_events;
mod model_picker;
mod prompt;
mod slash_commands;
mod telemetry;
mod text_tools;
mod tool_output;
mod ui;
mod unified;
mod welcome;

#[derive(Clone, Debug)]
pub struct ResumeSession {
    pub identifier: String,
    pub snapshot: SessionSnapshot,
    pub history: Vec<ProviderMessage>,
    pub path: PathBuf,
}

impl ResumeSession {
    pub fn message_count(&self) -> usize {
        self.history.len()
    }
}

pub async fn run_single_agent_loop(
    config: &CoreAgentConfig,
    skip_confirmations: bool,
    full_auto: bool,
    resume: Option<ResumeSession>,
) -> Result<()> {
    let mut vt_cfg = ConfigManager::load_from_workspace(&config.workspace)
        .ok()
        .map(|manager| manager.config().clone());

    apply_runtime_overrides(vt_cfg.as_mut(), config);

    let driver = unified::UnifiedTurnDriver;
    let params = TurnDriverParams::new(config, vt_cfg, skip_confirmations, full_auto, resume);
    driver.drive_turn(params).await
}

pub(crate) fn is_context_overflow_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("context length")
        || lower.contains("context window")
        || lower.contains("maximum context")
        || lower.contains("model is overloaded")
        || lower.contains("reduce the amount")
        || lower.contains("token limit")
        || lower.contains("503")
}

fn apply_runtime_overrides(vt_cfg: Option<&mut VTCodeConfig>, runtime_cfg: &CoreAgentConfig) {
    if let Some(cfg) = vt_cfg {
        cfg.agent.provider = runtime_cfg.provider.clone();

        if matches!(runtime_cfg.model_source, ModelSelectionSource::CliOverride) {
            let override_model = runtime_cfg.model.clone();
            cfg.agent.default_model = override_model.clone();
            cfg.router.models.simple = override_model.clone();
            cfg.router.models.standard = override_model.clone();
            cfg.router.models.complex = override_model.clone();
            cfg.router.models.codegen_heavy = override_model.clone();
            cfg.router.models.retrieval_heavy = override_model;
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
    fn cli_model_override_updates_router_models() {
        const OVERRIDE_MODEL: &str = "override-model";

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.default_model = "config-model".to_string();
        vt_cfg.router.models.simple = "config-simple".to_string();
        vt_cfg.router.models.standard = "config-standard".to_string();
        vt_cfg.router.models.complex = "config-complex".to_string();
        vt_cfg.router.models.codegen_heavy = "config-codegen".to_string();
        vt_cfg.router.models.retrieval_heavy = "config-retrieval".to_string();

        let runtime_cfg = CoreAgentConfig {
            model: OVERRIDE_MODEL.to_string(),
            api_key: String::new(),
            provider: "cli-provider".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: std::env::current_dir().unwrap(),
            verbose: false,
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
        };

        apply_runtime_overrides(Some(&mut vt_cfg), &runtime_cfg);

        assert_eq!(vt_cfg.agent.default_model, OVERRIDE_MODEL);
        assert_eq!(vt_cfg.router.models.simple, OVERRIDE_MODEL);
        assert_eq!(vt_cfg.router.models.standard, OVERRIDE_MODEL);
        assert_eq!(vt_cfg.router.models.complex, OVERRIDE_MODEL);
        assert_eq!(vt_cfg.router.models.codegen_heavy, OVERRIDE_MODEL);
        assert_eq!(vt_cfg.router.models.retrieval_heavy, OVERRIDE_MODEL);
        assert_eq!(vt_cfg.agent.provider, "cli-provider");
    }

    #[test]
    fn workspace_config_preserves_router_models() {
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.router.models.standard = "config-standard".to_string();

        let runtime_cfg = CoreAgentConfig {
            model: "config-standard".to_string(),
            api_key: String::new(),
            provider: "config-provider".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: std::env::current_dir().unwrap(),
            verbose: false,
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
        };

        apply_runtime_overrides(Some(&mut vt_cfg), &runtime_cfg);

        assert_eq!(vt_cfg.router.models.standard, "config-standard");
        assert_eq!(vt_cfg.agent.provider, "config-provider");
    }
}
