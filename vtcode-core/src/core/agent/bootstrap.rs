//! Component builder and bootstrap utilities for the core agent.
//!
//! This module extracts the initialization logic from [`Agent`](super::core::Agent)
//! so it can be reused by downstream consumers. The builder pattern makes it easy
//! to override default components (for example when embedding VTCode in other
//! applications or exposing a reduced open-source surface area) without relying
//! on the binary crate's internal setup.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::config::models::ModelId;
use crate::config::types::{AgentConfig, SessionInfo};
use crate::core::agent::compaction::CompactionEngine;
use crate::core::conversation_summarizer::ConversationSummarizer;
use crate::core::decision_tracker::DecisionTracker;
use crate::core::error_recovery::ErrorRecoveryManager;
use crate::llm::{AnyClient, make_client};
use crate::tools::ToolRegistry;
use crate::tools::tree_sitter::TreeSitterAnalyzer;

/// Collection of dependencies required by the [`Agent`](super::core::Agent).
///
/// Consumers that want to reuse the agent loop can either construct this bundle
/// directly with [`AgentComponentBuilder`] or provide their own specialized
/// implementation when embedding VTCode.
pub struct AgentComponentSet {
    pub client: AnyClient,
    pub tool_registry: Arc<ToolRegistry>,
    pub decision_tracker: DecisionTracker,
    pub error_recovery: ErrorRecoveryManager,
    pub summarizer: ConversationSummarizer,
    pub tree_sitter_analyzer: TreeSitterAnalyzer,
    pub compaction_engine: Arc<CompactionEngine>,
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
    summarizer: Option<ConversationSummarizer>,
    tree_sitter_analyzer: Option<TreeSitterAnalyzer>,
    compaction_engine: Option<Arc<CompactionEngine>>,
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
            summarizer: None,
            tree_sitter_analyzer: None,
            compaction_engine: None,
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

    /// Override the conversation summarizer instance.
    pub fn with_summarizer(mut self, summarizer: ConversationSummarizer) -> Self {
        self.summarizer = Some(summarizer);
        self
    }

    /// Override the tree-sitter analyzer instance.
    pub fn with_tree_sitter_analyzer(mut self, analyzer: TreeSitterAnalyzer) -> Self {
        self.tree_sitter_analyzer = Some(analyzer);
        self
    }

    /// Override the compaction engine instance.
    pub fn with_compaction_engine(mut self, engine: Arc<CompactionEngine>) -> Self {
        self.compaction_engine = Some(engine);
        self
    }

    /// Override the session metadata.
    pub fn with_session_info(mut self, session_info: SessionInfo) -> Self {
        self.session_info = Some(session_info);
        self
    }

    /// Build the component set, lazily constructing any missing dependencies.
    pub async fn build(mut self) -> Result<AgentComponentSet> {
        let client = match self.client.take() {
            Some(client) => client,
            None => create_llm_client(self.config)?,
        };

        let tree_sitter_analyzer = match self.tree_sitter_analyzer.take() {
            Some(analyzer) => analyzer,
            None => TreeSitterAnalyzer::new()
                .context("Failed to initialize tree-sitter analyzer for agent components")?,
        };

        let tool_registry = match self.tool_registry {
            Some(registry) => registry,
            None => Arc::new(ToolRegistry::new(self.config.workspace.clone()).await),
        };

        let decision_tracker = self.decision_tracker.unwrap_or_else(DecisionTracker::new);

        let error_recovery = self
            .error_recovery
            .unwrap_or_else(ErrorRecoveryManager::new);

        let summarizer = self.summarizer.unwrap_or_else(ConversationSummarizer::new);

        let compaction_engine = self
            .compaction_engine
            .unwrap_or_else(|| Arc::new(CompactionEngine::new()));

        let session_info = match self.session_info.take() {
            Some(info) => info,
            None => create_session_info()
                .context("Failed to initialize agent session metadata for bootstrap")?,
        };

        Ok(AgentComponentSet {
            client,
            tool_registry,
            decision_tracker,
            error_recovery,
            summarizer,
            tree_sitter_analyzer,
            compaction_engine,
            session_info,
        })
    }
}

fn create_llm_client(config: &AgentConfig) -> Result<AnyClient> {
    let model_id = config
        .model
        .parse::<ModelId>()
        .with_context(|| format!("Invalid model identifier: {}", config.model))?;

    Ok(make_client(config.api_key.clone(), model_id))
}

fn create_session_info() -> Result<SessionInfo> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System time is before the UNIX epoch")?;

    Ok(SessionInfo {
        session_id: format!("session_{}", now.as_secs()),
        start_time: now.as_secs(),
        total_turns: 0,
        total_decisions: 0,
        error_count: 0,
    })
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
            model: models::GEMINI_2_5_FLASH_PREVIEW.to_string(),
            api_key: "test-api-key".to_string(),
            provider: Provider::Gemini.to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: temp_dir.path().to_path_buf(),
            verbose: false,
            theme: "default".to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::Inline,
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
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
            model: models::GEMINI_2_5_FLASH_PREVIEW.to_string(),
            api_key: "test-api-key".to_string(),
            provider: Provider::Gemini.to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: temp_dir.path().to_path_buf(),
            verbose: true,
            theme: "custom".to_string(),
            reasoning_effort: ReasoningEffortLevel::High,
            ui_surface: UiSurfacePreference::Alternate,
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        };

        let custom_session = SessionInfo {
            session_id: "session_custom".to_string(),
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
}
