use anyhow::Result;
use async_trait::async_trait;
use vtcode_core::core::interfaces::acp::{AcpClientAdapter, AcpLaunchParams};

mod agent;
pub(crate) mod constants;
mod helpers;
mod session;
mod types;

pub(crate) use agent::ZedAgent;
use session::run_acp_agent;

#[derive(Debug, Default, Clone, Copy)]
pub struct ZedAcpAdapter;

#[async_trait(?Send)]
impl AcpClientAdapter for ZedAcpAdapter {
    async fn serve(&self, params: AcpLaunchParams<'_>) -> Result<()> {
        run_acp_agent(
            params.agent_config,
            params.runtime_config,
            Some("Zed".to_string()),
        )
        .await
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StandardAcpAdapter;

#[async_trait(?Send)]
impl AcpClientAdapter for StandardAcpAdapter {
    async fn serve(&self, params: AcpLaunchParams<'_>) -> Result<()> {
        run_acp_agent(params.agent_config, params.runtime_config, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::tooling::{
        TOOL_LIST_FILES_ITEMS_KEY, TOOL_LIST_FILES_RESULT_KEY, TOOL_LIST_FILES_URI_ARG,
    };
    use crate::acp::zed::types::NotificationEnvelope;
    use agent_client_protocol::{Agent, LoadSessionRequest, SessionModeId, ToolCallStatus};
    use assert_fs::TempDir;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::path::Path;
    use tokio::fs;
    use tokio::sync::mpsc;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel,
        UiSurfacePreference,
    };
    use vtcode_core::config::{AgentClientProtocolZedConfig, CommandsConfig, ToolsConfig};
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

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
        };

        let mut zed_config = AgentClientProtocolZedConfig::default();
        zed_config.tools.list_files = true;
        zed_config.tools.read_file = false;

        let tools_config = ToolsConfig::default();
        let (tx, mut rx) = mpsc::unbounded_channel::<NotificationEnvelope>();
        // Spawn a task to handle session updates so send_update doesn't fail
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

    fn list_items_from_payload(payload: &Value) -> Vec<Value> {
        payload
            .get(TOOL_LIST_FILES_RESULT_KEY)
            .and_then(Value::as_object)
            .and_then(|result| result.get(TOOL_LIST_FILES_ITEMS_KEY))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn run_list_files_defaults_to_workspace_root() {
        let temp = TempDir::new().unwrap();
        let subdir = temp.path().join("src");
        fs::create_dir(&subdir).await.unwrap();
        let file_path = subdir.join("example.txt");
        fs::write(&file_path, "hello").await.unwrap();

        let agent = build_agent(temp.path()).await;
        let report = agent.run_list_files(&json!({"path": "src"})).await.unwrap();

        assert!(matches!(report.status, ToolCallStatus::Completed));
        let payload = report.raw_output.unwrap();
        let items = list_items_from_payload(&payload);
        assert!(items.iter().any(|item| {
            item.get("name")
                .and_then(Value::as_str)
                .map(|name| name == "example.txt")
                .unwrap_or(false)
        }));
    }

    #[tokio::test]
    async fn run_list_files_accepts_uri_argument() {
        let temp = TempDir::new().unwrap();
        let nested = temp.path().join("nested");
        fs::create_dir_all(&nested).await.unwrap();
        let inner = nested.join("inner.txt");
        fs::write(&inner, "data").await.unwrap();

        let agent = build_agent(temp.path()).await;
        let uri = format!("file://{}", nested.to_string_lossy());
        let report = agent
            .run_list_files(&json!({ TOOL_LIST_FILES_URI_ARG: uri }))
            .await
            .unwrap();

        assert!(matches!(report.status, ToolCallStatus::Completed));
        let payload = report.raw_output.unwrap();
        let items = list_items_from_payload(&payload);
        assert!(items.iter().any(|item| {
            item.get("path")
                .and_then(Value::as_str)
                .map(|path| path.contains("inner.txt"))
                .unwrap_or(false)
        }));
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
    async fn load_session_returns_existing_session_state() {
        let temp = TempDir::new().unwrap();
        let agent = build_agent(temp.path()).await;
        let session_id = agent.register_session();

        // Change the mode of the registered session
        {
            let session = agent.session_handle(&session_id).unwrap();
            let mut data = session.data.borrow_mut();
            data.current_mode = SessionModeId::new("architect");
        }

        let args = LoadSessionRequest::new(session_id, temp.path());
        let response = agent.load_session(args).await.unwrap();

        assert_eq!(response.modes.unwrap().current_mode_id, SessionModeId::new("architect"));
    }

    #[tokio::test]
    async fn run_switch_mode_updates_session_mode() {
        let temp = TempDir::new().unwrap();
        let agent = build_agent(temp.path()).await;
        let session_id = agent.register_session();

        // Verify initial mode is "code" (default in register_session)
        {
            let session = agent.session_handle(&session_id).unwrap();
            assert_eq!(
                session.data.borrow().current_mode,
                SessionModeId::new("code")
            );
        }

        // Switch to "architect"
        let args = json!({ "mode_id": "architect" });
        let report = agent.run_switch_mode(&session_id, &args).await.unwrap();

        assert!(matches!(report.status, ToolCallStatus::Completed));

        // Verify session mode was updated
        {
            let session = agent.session_handle(&session_id).unwrap();
            assert_eq!(
                session.data.borrow().current_mode,
                SessionModeId::new("architect")
            );
        }
    }
}
