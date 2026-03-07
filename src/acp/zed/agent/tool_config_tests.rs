use super::ZedAgent;
use crate::acp::zed::types::{NotificationEnvelope, ToolRuntime};
use assert_fs::TempDir;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use tokio::sync::mpsc;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::config::{AgentClientProtocolZedConfig, CommandsConfig, ToolsConfig};
use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use vtcode_core::core::interfaces::SessionMode;
use vtcode_core::llm::provider::{MessageRole, ToolDefinition};

async fn build_agent(workspace: &Path) -> ZedAgent {
    let core_config = CoreAgentConfig {
        model: "test-model".to_string(),
        api_key: String::new(),
        provider: "test-provider".to_string(),
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
    };

    let mut zed_config = AgentClientProtocolZedConfig::default();
    zed_config.tools.list_files = true;
    zed_config.tools.read_file = false;

    let (tx, mut rx) = mpsc::unbounded_channel::<NotificationEnvelope>();
    tokio::spawn(async move {
        while let Some(envelope) = rx.recv().await {
            let _ = envelope.completion.send(());
        }
    });

    ZedAgent::new(
        core_config,
        zed_config,
        ToolsConfig::default(),
        CommandsConfig::default(),
        String::new(),
        tx,
        Some("Zed".to_string()),
    )
    .await
}

fn definition_names(definitions: Vec<ToolDefinition>) -> Vec<String> {
    definitions
        .into_iter()
        .map(|definition| definition.function_name().to_string())
        .collect()
}

#[test]
fn parse_terminal_command_rejects_empty_array() {
    let args = json!({ "command": [] });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "command array cannot be empty");
}

#[test]
fn parse_terminal_command_rejects_empty_string() {
    let args = json!({ "command": "" });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "command string cannot be empty");
}

#[test]
fn parse_terminal_command_rejects_whitespace_only_string() {
    let args = json!({ "command": "   " });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "command string cannot be empty");
}

#[test]
fn parse_terminal_command_rejects_empty_executable_in_array() {
    let args = json!({ "command": ["", "arg1", "arg2"] });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "command executable cannot be empty");
}

#[test]
fn parse_terminal_command_rejects_whitespace_only_executable_in_array() {
    let args = json!({ "command": ["  ", "arg1"] });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "command executable cannot be empty");
}

#[test]
fn parse_terminal_command_accepts_valid_array() {
    let args = json!({ "command": ["ls", "-la"] });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_ok());
    let cmd = result.unwrap();
    assert_eq!(cmd, vec!["ls", "-la"]);
}

#[test]
fn parse_terminal_command_accepts_valid_string() {
    let args = json!({ "command": "echo test" });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_ok());
    let cmd = result.unwrap();
    assert_eq!(cmd, vec!["echo", "test"]);
}

#[test]
fn parse_terminal_command_rejects_missing_command_field() {
    let args = json!({});
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "run_pty_cmd requires a 'command' field (string/array or indexed command.N entries)"
    );
}

#[test]
fn parse_terminal_command_accepts_indexed_arguments_zero_based() {
    let args = json!({ "command.0": "python", "command.1": "-c", "command.2": "print('hi')" });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_ok());
    let cmd = result.unwrap();
    assert_eq!(cmd, vec!["python", "-c", "print('hi')"]);
}

#[test]
fn parse_terminal_command_accepts_indexed_arguments_one_based() {
    let args = json!({ "command.1": "ls", "command.2": "-a" });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_ok());
    let cmd = result.unwrap();
    assert_eq!(cmd, vec!["ls", "-a"]);
}

#[test]
fn parse_terminal_command_rejects_non_string_indexed_argument() {
    let args = json!({ "command.0": 1 });
    let result = ZedAgent::parse_terminal_command(&args);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "command array must contain only strings"
    );
}

#[tokio::test]
async fn read_only_modes_hide_local_tools() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let enabled_tools: Vec<_> = agent
        .tool_availability(true, false)
        .into_iter()
        .filter_map(|(tool, runtime)| match runtime {
            ToolRuntime::Enabled => Some(tool),
            ToolRuntime::Disabled(_) => None,
        })
        .collect();

    let ask_names = definition_names(
        agent
            .tool_definitions(true, &enabled_tools, SessionMode::Ask)
            .unwrap(),
    );
    let architect_names = definition_names(
        agent
            .tool_definitions(true, &enabled_tools, SessionMode::Architect)
            .unwrap(),
    );
    let code_names = definition_names(
        agent
            .tool_definitions(true, &enabled_tools, SessionMode::Code)
            .unwrap(),
    );

    assert_eq!(
        ask_names,
        vec!["list_files".to_string(), "switch_mode".to_string()]
    );
    assert_eq!(architect_names, ask_names);
    assert!(code_names.contains(&"switch_mode".to_string()));
    assert!(code_names.contains(&"list_files".to_string()));
    assert!(code_names.contains(&"unified_search".to_string()));
    assert!(code_names.contains(&"unified_file".to_string()));
    assert!(code_names.contains(&"unified_exec".to_string()));
}

#[tokio::test]
async fn resolved_messages_include_mode_prompt_for_read_only_modes() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();
    let session = agent.session_handle(&session_id).unwrap();

    assert!(agent.resolved_messages(&session).is_empty());

    agent.update_session_mode(&session, SessionMode::Architect);
    let messages = agent.resolved_messages(&session);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, MessageRole::System);
    let prompt = messages[0].content.as_text();
    assert!(prompt.contains("Architect mode"));
    assert!(prompt.contains("switch to Code mode"));
}
