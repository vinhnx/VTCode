use super::ZedAgent;
use crate::tooling::ToolDescriptor;
use crate::zed::types::{NotificationEnvelope, ToolRuntime};
use assert_fs::TempDir;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;
use vtcode_core::config::constants::tools;
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
    build_agent_with_tools_config(workspace, ToolsConfig::default()).await
}

async fn build_agent_with_tools_config(workspace: &Path, tools_config: ToolsConfig) -> ZedAgent {
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
        openai_chatgpt_auth: None,
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
        tools_config,
        CommandsConfig::default(),
        String::new(),
        tx,
        Some("Zed".to_string()),
    )
    .await
}

#[test]
fn tool_call_delay_for_rate_ignores_unset_or_zero_limits() {
    assert_eq!(ZedAgent::tool_call_delay_for_rate(None), None);
    assert_eq!(ZedAgent::tool_call_delay_for_rate(Some(0)), None);
}

#[test]
fn tool_call_delay_for_rate_uses_per_second_interval() {
    assert_eq!(
        ZedAgent::tool_call_delay_for_rate(Some(4)),
        Some(Duration::from_millis(250))
    );
}

#[tokio::test]
async fn tool_loop_limit_uses_tools_config() {
    let temp = TempDir::new().unwrap();
    let mut tools_config = ToolsConfig::default();
    tools_config.max_tool_loops = 2;
    let agent = build_agent_with_tools_config(temp.path(), tools_config).await;

    assert!(!agent.tool_loop_limit_reached(0));
    assert!(!agent.tool_loop_limit_reached(1));
    assert!(agent.tool_loop_limit_reached(2));
    assert!(
        agent
            .tool_loop_limit_message()
            .contains("maximum tool loops (2)")
    );
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
fn parse_terminal_command_accepts_cmd_alias() {
    let args = json!({ "cmd": "echo test" });
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
        "command execution requires a 'command' field (string/array or indexed command.N entries)"
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
async fn resolve_terminal_working_dir_accepts_workdir_alias() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let args = json!({ "workdir": "src" });

    let working_dir = agent
        .resolve_terminal_working_dir(&args)
        .expect("workdir alias should resolve")
        .expect("working directory should be present");

    assert_eq!(working_dir, temp.path().join("src"));
}

#[tokio::test]
async fn read_only_modes_hide_local_tools() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let enabled_tools: Vec<_> = agent
        .tool_availability(true, false, "test-provider", "test-model")
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
        vec![tools::LIST_FILES.to_string(), "switch_mode".to_string()]
    );
    assert_eq!(architect_names, ask_names);
    assert!(code_names.contains(&"switch_mode".to_string()));
    assert!(code_names.contains(&tools::LIST_FILES.to_string()));
    assert!(code_names.contains(&"unified_search".to_string()));
    assert!(code_names.contains(&"unified_file".to_string()));
    assert!(code_names.contains(&"unified_exec".to_string()));
}

#[tokio::test]
async fn local_tool_execution_uses_registry_request_path() {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src").join("sample.txt"), "hello").unwrap();
    let agent = build_agent(temp.path()).await;

    let report = agent
        .execute_local_tool(
            tools::UNIFIED_SEARCH,
            &json!({
                "action": "list",
                "path": "src",
            }),
            "call-local-list",
        )
        .await;

    assert_eq!(report.status, crate::acp::ToolCallStatus::Completed);
    let payload = report.raw_output.expect("successful tool output");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["tool"], tools::UNIFIED_SEARCH);
    assert!(payload["result"].to_string().contains("sample.txt"));
}

#[tokio::test]
async fn local_tool_execution_reports_registry_failure() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;

    let report = agent
        .execute_local_tool("unknown_tool", &json!({}), "call-local-missing")
        .await;

    assert_eq!(report.status, crate::acp::ToolCallStatus::Failed);
    assert!(report.llm_response.contains("unknown_tool"));
}

#[tokio::test]
async fn local_tool_metadata_uses_core_action_labels_and_kinds() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let exec_args = json!({
        "action": "run",
        "command": "cargo check",
    });
    let search_args = json!({
        "action": "list",
        "path": "src",
    });
    let read_args = json!({
        "action": "read",
        "path": "src/lib.rs",
    });
    let write_args = json!({
        "action": "write",
        "path": "src/lib.rs",
        "content": "updated",
    });

    assert_eq!(
        agent.acp_tool_registry.render_title(
            ToolDescriptor::Local,
            tools::UNIFIED_EXEC,
            &exec_args
        ),
        "Run command"
    );
    assert_eq!(
        agent
            .acp_tool_registry
            .tool_kind_for_call(tools::UNIFIED_EXEC, Some(&exec_args)),
        crate::acp::ToolKind::Execute
    );
    assert_eq!(
        agent.acp_tool_registry.render_title(
            ToolDescriptor::Local,
            tools::UNIFIED_SEARCH,
            &search_args
        ),
        "List files"
    );
    assert_eq!(
        agent
            .acp_tool_registry
            .tool_kind_for_call(tools::UNIFIED_SEARCH, Some(&search_args)),
        crate::acp::ToolKind::Search
    );
    assert_eq!(
        agent.acp_tool_registry.render_title(
            ToolDescriptor::Local,
            tools::UNIFIED_FILE,
            &read_args
        ),
        "Read file"
    );
    assert_eq!(
        agent
            .acp_tool_registry
            .tool_kind_for_call(tools::UNIFIED_FILE, Some(&read_args)),
        crate::acp::ToolKind::Read
    );
    assert_eq!(
        agent.acp_tool_registry.render_title(
            ToolDescriptor::Local,
            tools::UNIFIED_FILE,
            &write_args
        ),
        "Write file"
    );
    assert_eq!(
        agent
            .acp_tool_registry
            .tool_kind_for_call(tools::UNIFIED_FILE, Some(&write_args)),
        crate::acp::ToolKind::Edit
    );
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
