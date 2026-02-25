//! Component builder and bootstrap utilities for the core agent.
//!
//! This module extracts the initialization logic from [`Agent`](super::core::Agent)
//! so it can be reused by downstream consumers. The builder pattern makes it easy
//! to override default components (for example when embedding VT Code in other
//! applications or exposing a reduced open-source surface area) without relying
//! on the binary crate's internal setup.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::config::models::{ModelId, Provider};
use crate::config::types::{AgentConfig, SessionInfo};
use crate::models_manager::ModelsManager;
use crate::utils::error_messages::{ERR_CREATE_DIR, ERR_GET_METADATA};

use crate::core::decision_tracker::DecisionTracker;
use crate::core::error_recovery::ErrorRecoveryManager;
use crate::ctx_err;
use crate::llm::{AnyClient, make_client};
use crate::tools::ToolRegistry;
use tracing::warn;

/// Collection of dependencies required by the [`Agent`](super::core::Agent).
///
/// Consumers that want to reuse the agent loop can either construct this bundle
/// directly with [`AgentComponentBuilder`] or provide their own specialized
/// implementation when embedding VT Code.
pub struct AgentComponentSet {
    pub client: AnyClient,
    pub tool_registry: Arc<ToolRegistry>,
    pub decision_tracker: DecisionTracker,
    pub error_recovery: ErrorRecoveryManager,
    pub models_manager: Arc<ModelsManager>,

    pub session_info: SessionInfo,
}

/// Builder for [`AgentComponentSet`].
///
/// The builder exposes hooks for overriding individual components which makes
/// the agent easier to adapt for open-source scenarios or bespoke deployments.
pub struct AgentComponentBuilder<'config> {
    config: &'config AgentConfig,
    client: Option<AnyClient>,
    tool_registry: Option<Arc<ToolRegistry>>,
    decision_tracker: Option<DecisionTracker>,
    error_recovery: Option<ErrorRecoveryManager>,
    models_manager: Option<Arc<ModelsManager>>,

    session_info: Option<SessionInfo>,
}

impl<'config> AgentComponentBuilder<'config> {
    /// Create a new builder scoped to the provided configuration.
    pub fn new(config: &'config AgentConfig) -> Self {
        Self {
            config,
            client: None,
            tool_registry: None,
            decision_tracker: None,
            error_recovery: None,
            models_manager: None,
            session_info: None,
        }
    }

    /// Override the LLM client instance.
    pub fn with_client(mut self, client: AnyClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Override the tool registry instance.
    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// Override the decision tracker instance.
    pub fn with_decision_tracker(mut self, tracker: DecisionTracker) -> Self {
        self.decision_tracker = Some(tracker);
        self
    }

    /// Override the error recovery manager instance.
    pub fn with_error_recovery(mut self, manager: ErrorRecoveryManager) -> Self {
        self.error_recovery = Some(manager);
        self
    }

    /// Override the models manager instance.
    pub fn with_models_manager(mut self, manager: Arc<ModelsManager>) -> Self {
        self.models_manager = Some(manager);
        self
    }

    /// Override the session metadata.
    pub fn with_session_info(mut self, session_info: SessionInfo) -> Self {
        self.session_info = Some(session_info);
        self
    }

    /// Build the component set, lazily constructing any missing dependencies.
    pub async fn build(mut self) -> Result<AgentComponentSet> {
        ensure_workspace_ready(&self.config.workspace)?;

        let client = match self.client.take() {
            Some(client) => client,
            None => create_llm_client(self.config)?,
        };

        let session_info = match self.session_info.take() {
            Some(info) => info,
            None => create_session_info()
                .context("Failed to initialize agent session metadata for bootstrap")?,
        };

        let tool_registry = match self.tool_registry {
            Some(registry) => registry,
            None => {
                let registry = ToolRegistry::new(self.config.workspace.clone()).await;
                registry.set_harness_session(session_info.session_id.clone());
                Arc::new(registry)
            }
        };

        // Prefer custom manager if provided, otherwise reuse global singleton.
        // The global singleton is provider-agnostic; provider filtering happens at query time.
        let models_manager = self.models_manager.take().unwrap_or_else(|| {
            // Clone Arc from global - this is cheap since ModelsManager is behind LazyLock
            Arc::new(ModelsManager::with_provider(
                self.config
                    .provider
                    .parse::<Provider>()
                    .ok()
                    .unwrap_or_default(),
            ))
        });

        let decision_tracker = self.decision_tracker.unwrap_or_default();

        let error_recovery = self.error_recovery.unwrap_or_default();

        Ok(AgentComponentSet {
            client,
            tool_registry,
            decision_tracker,
            error_recovery,
            models_manager,

            session_info,
        })
    }
}

fn create_llm_client(config: &AgentConfig) -> Result<AnyClient> {
    let model_id = config
        .model
        .parse::<ModelId>()
        .with_context(|| format!("Invalid model identifier: {}", config.model))?;

    Ok(make_client(config.api_key.clone(), model_id)?)
}

fn create_session_info() -> Result<SessionInfo> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.duration());

    Ok(build_session_info(now))
}

fn build_session_info(duration: Result<Duration, Duration>) -> SessionInfo {
    let (start_time, session_id) = match duration {
        Ok(duration) => {
            let secs = duration.as_secs();
            (secs, format!("session_{}", secs))
        }
        Err(delta) => {
            let fallback = delta.as_secs();
            warn!(
                fallback_seconds = fallback,
                "System time is before UNIX epoch; using fallback session id"
            );
            (fallback, format!("session_fallback_{}", fallback))
        }
    };

    SessionInfo {
        session_id,
        start_time,
        total_turns: 0,
        total_decisions: 0,
        error_count: 0,
    }
}

fn ensure_workspace_ready(workspace_root: &Path) -> Result<()> {
    if workspace_root.exists() {
        let metadata = fs::metadata(workspace_root)
            .with_context(|| ctx_err!(ERR_GET_METADATA, workspace_root.display()))?;

        anyhow::ensure!(
            metadata.is_dir(),
            "Workspace path is not a directory: {}",
            workspace_root.display()
        );
    } else {
        fs::create_dir_all(workspace_root)
            .with_context(|| ctx_err!(ERR_CREATE_DIR, workspace_root.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::models;
    use crate::config::core::PromptCachingConfig;
    use crate::config::models::Provider;
    use crate::config::types::{ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference};
    use crate::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn builds_default_component_set() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let agent_config = AgentConfig {
            model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            api_key: "test-api-key".to_owned(),
            provider: Provider::Gemini.to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: temp_dir.path().to_path_buf(),
            verbose: false,
            theme: "default".to_owned(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::Inline,
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            quiet: false,
            max_conversation_turns: 1000,
            model_behavior: None,
        };

        let components = AgentComponentBuilder::new(&agent_config)
            .build()
            .await
            .expect("component build succeeds");

        assert!(components.session_info.session_id.starts_with("session_"));
        assert_eq!(components.session_info.total_turns, 0);
        assert!(!components.tool_registry.available_tools().await.is_empty());
    }

    #[tokio::test]
    async fn allows_overriding_components() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let agent_config = AgentConfig {
            model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            api_key: "test-api-key".to_owned(),
            provider: Provider::Gemini.to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: temp_dir.path().to_path_buf(),
            verbose: true,
            theme: "custom".to_owned(),
            reasoning_effort: ReasoningEffortLevel::High,
            ui_surface: UiSurfacePreference::Alternate,
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            quiet: false,
            max_conversation_turns: 1000,
            model_behavior: None,
        };

        let custom_session = SessionInfo {
            session_id: "session_custom".to_owned(),
            start_time: 42,
            total_turns: 1,
            total_decisions: 2,
            error_count: 3,
        };

        let registry = Arc::new(ToolRegistry::new(agent_config.workspace.clone()).await);

        let components = AgentComponentBuilder::new(&agent_config)
            .with_session_info(custom_session.clone())
            .with_tool_registry(Arc::clone(&registry))
            .build()
            .await
            .expect("component build succeeds with overrides");

        assert_eq!(
            components.session_info.session_id,
            custom_session.session_id
        );
        assert_eq!(
            components.session_info.start_time,
            custom_session.start_time
        );
        assert_eq!(
            Arc::as_ptr(&components.tool_registry),
            Arc::as_ptr(&registry)
        );
    }

    #[test]
    fn session_info_uses_fallback_when_clock_is_before_epoch() {
        let info = build_session_info(Err(Duration::from_secs(42)));
        assert_eq!(info.session_id, "session_fallback_42");
        assert_eq!(info.start_time, 42);
        assert_eq!(info.total_turns, 0);
    }

    #[tokio::test]
    async fn rejects_non_directory_workspace() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let file_path = temp_dir.path().join("not_dir");
        std::fs::write(&file_path, "not a dir").expect("write file");

        let agent_config = AgentConfig {
            workspace: file_path.clone(),
            ..sample_config(temp_dir.path())
        };

        let result = AgentComponentBuilder::new(&agent_config).build().await;
        assert!(result.is_err(), "expected workspace validation to fail");
    }

    fn sample_config(workspace: &std::path::Path) -> AgentConfig {
        AgentConfig {
            model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            api_key: "test-api-key".to_owned(),
            provider: Provider::Gemini.to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: workspace.to_path_buf(),
            verbose: false,
            theme: "default".to_owned(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::Inline,
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            quiet: false,
            max_conversation_turns: 1000,
            model_behavior: None,
        }
    }
}
