pub(super) use super::*;

fn test_startup_context(workspace: &Path) -> StartupContext {
    use std::collections::BTreeMap;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
    };
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

    StartupContext {
        workspace: workspace.to_path_buf(),
        config: VTCodeConfig::default(),
        agent_config: CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            api_key: "test-key".to_string(),
            provider: "gemini".to_string(),
            openai_chatgpt_auth: None,
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: workspace.to_path_buf(),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
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
        },
        skip_confirmations: false,
        full_auto_requested: false,
        automation_prompt: None,
        primary_agent_explicitly_configured: false,
        session_resume: None,
        resume_show_all: false,
        custom_session_id: None,
        summarize_fork: false,
        planning_entry_source: vtcode_core::core::interfaces::session::PlanningEntrySource::None,
    }
}

fn test_service_context(
    startup: StartupContext,
    workspace: PathBuf,
) -> (ServiceContext, mpsc::Receiver<protocol::Notification>) {
    let (notifications, receiver) = mpsc::channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let context = ServiceContext {
        startup,
        workspace,
        state: std::sync::Arc::new(Mutex::new(ServerState::default())),
        notifications,
        turn_finished: std::sync::Arc::new(Notify::new()),
        webmcp: Mutex::new(None),
    };
    (context, receiver)
}

fn test_git(workspace: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(workspace)
        .args(args)
        .output()
        .expect("Git executable");
    assert!(output.status.success(), "Git setup failed: {}", String::from_utf8_lossy(&output.stderr));
}

async fn test_webmcp_server(context: &ServiceContext) -> WebmcpServer {
    let workspace = FilesystemWorkspace::new(&context.workspace, [context.workspace.clone()], false)
        .await
        .expect("workspace adapter");
    let adapter = DesktopWebmcpRuntime {
        workspace,
        state: std::sync::Arc::clone(&context.state),
        notifications: context.notifications.clone(),
    };
    let settings = &context.startup.config.webmcp;
    WebmcpServer::new(
        std::sync::Arc::new(adapter),
        WebmcpServerConfig {
            host: settings.host.clone(),
            port: settings.port,
            allowed_origins: settings.allowed_origins.clone(),
            pairing_ttl_secs: settings.pairing_ttl_secs,
            max_frame_bytes: settings.max_frame_bytes,
            max_in_flight_requests: settings.max_in_flight_requests,
            ..Default::default()
        },
    )
    .expect("WebMCP server")
}

#[cfg(unix)]
fn review_item(id: &str, patch: &str) -> vtcode_core::exec::events::ThreadItem {
    use vtcode_core::exec::events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            changes: vec![FileUpdateChange {
                path: "src/app.rs".into(),
                kind: PatchChangeKind::Update,
            }],
            status: PatchApplyStatus::Completed,
            unified_diff: Some(patch.into()),
            diff_incomplete: Some(false),
            additions: None,
            deletions: None,
        })),
    }
}

#[cfg(unix)]
fn tool_output_item(id: &str, spool_path: Option<&str>, inline_preview: &str) -> vtcode_core::exec::events::ThreadItem {
    use vtcode_core::exec::events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: format!("{id}-call"),
            tool_call_id: None,
            spool_path: spool_path.map(str::to_owned),
            output: inline_preview.into(),
            exit_code: Some(0),
            status: ToolCallStatus::Completed,
        })),
    }
}

mod model_review;
mod session_history;
mod shutdown_turns;
mod spooled_scope;
mod startup_interaction;
mod terminal_pump;
