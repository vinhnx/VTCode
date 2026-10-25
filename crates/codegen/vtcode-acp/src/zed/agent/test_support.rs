//! Shared ZedAgent fixture construction for the agent-module test suites.

use std::collections::BTreeMap;
use std::path::Path;

use vtcode_config::auth::AuthCredentialsStoreMode;
use vtcode_config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_config::{
    AgentClientProtocolZedConfig, CommandsConfig, SubagentDiscoveryInput, ToolsConfig, discover_subagents,
};
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::core::agent::snapshots::{DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS};

use super::ZedAgent;
use crate::zed::helpers::PrimaryAgentCatalog;

pub(super) async fn build_agent(workspace: &Path) -> ZedAgent {
    build_agent_with_default_primary_agent(workspace, "build").await
}

pub(super) async fn build_agent_with_default_primary_agent(workspace: &Path, default_primary_agent: &str) -> ZedAgent {
    build_agent_with_commands(workspace, default_primary_agent, CommandsConfig::default()).await
}

/// Build the fixture with an explicit command policy so a regression can run a
/// command shape the default allow-list rejects.
pub(super) async fn build_agent_with_commands(
    workspace: &Path,
    default_primary_agent: &str,
    commands_config: CommandsConfig,
) -> ZedAgent {
    let core_config = CoreAgentConfig {
        model: "gpt-5.6-sol".to_string(),
        api_key: String::new(),
        provider: "openai".to_string(),
        api_key_env: "TEST_API_KEY".to_string(),
        workspace: workspace.to_path_buf(),
        verbose: false,
        quiet: false,
        theme: "test".to_string(),
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

    let mut discovery_input = SubagentDiscoveryInput::new(workspace.to_path_buf());
    discovery_input.include_user_agents = false;
    let discovered = discover_subagents(&discovery_input).expect("discover primary agents");
    let primary_agents = PrimaryAgentCatalog::from_specs_with_default(&discovered.effective, default_primary_agent);

    ZedAgent::new(
        core_config,
        false,
        AuthCredentialsStoreMode::default(),
        AgentClientProtocolZedConfig::default(),
        ToolsConfig::default(),
        commands_config,
        String::new(),
        Some("Zed".to_string()),
        primary_agents,
    )
    .await
}
