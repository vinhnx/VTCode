pub(super) use super::*;
pub(super) use crate::agent::runloop::unified::stop_requests;

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
        provider_auth_error: None,
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

/// Hermetic config workspace: a temp dir containing a `vtcode.toml` so
/// `ConfigManager::save_config` targets the workspace file. Without a
/// workspace file `config_path` falls back to the real user config and saves
/// stomp it (cross-test flakes plus user-config corruption). Same precedent
/// as the `theme_preferences` writer test.
pub(super) fn hermetic_workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::write(
        workspace.path().join("vtcode.toml"),
        "# Hermetic desktop-server test workspace; see hermetic_workspace.\n",
    )
    .expect("workspace config");
    workspace
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

/// Isolated session-store scope: `VT_SESSION_DIR` rooted at a temp dir.
/// Restores the environment on drop. Tests using this must be `#[serial]`.
struct IsolatedSessionStore {
    _temp: tempfile::TempDir,
    _env: vtcode_commons::env_lock::EnvGuard,
    previous: Option<std::ffi::OsString>,
}

impl IsolatedSessionStore {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("sessions dir");
        let env = vtcode_commons::env_lock::lock();
        let previous = std::env::var_os(vtcode_core::utils::session_archive::SESSION_DIR_ENV);
        env.set_var(vtcode_core::utils::session_archive::SESSION_DIR_ENV, temp.path());
        Self { _temp: temp, _env: env, previous }
    }

    fn path(&self) -> &Path {
        self._temp.path()
    }
}

impl Drop for IsolatedSessionStore {
    fn drop(&mut self) {
        self._env
            .restore_var(vtcode_core::utils::session_archive::SESSION_DIR_ENV, self.previous.clone());
    }
}

/// Run a runloop-booting test body on a multi-thread runtime with
/// production-like stacks. The desktop service runs its workers at 8 MiB
/// (`main.rs`), and unoptimized test builds overflow the default 2 MiB test
/// thread; 16 MiB matches `MAIN_THREAD_STACK_BYTES` and `scripts/run.sh`.
pub(super) fn block_on_with_desktop_stack<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .expect("test runtime")
        .block_on(future)
}

pub(super) async fn seed_archive(
    sessions: &Path,
    session_id: &str,
    workspace: &Path,
    messages: Vec<vtcode_core::utils::session_archive::SessionMessage>,
) {
    let snapshot = vtcode_core::utils::session_archive::SessionSnapshot {
        metadata: vtcode_core::utils::session_archive::SessionArchiveMetadata::new(
            "workspace",
            workspace.to_string_lossy(),
            "model",
            "provider",
            "theme",
            "medium",
        ),
        started_at: chrono::Utc::now(),
        ended_at: chrono::Utc::now(),
        total_messages: messages.len(),
        distinct_tools: Vec::new(),
        transcript: vec!["hello".into()],
        messages,
        progress: None,
        error_logs: Vec::new(),
    };
    vtcode_commons::fs::write_private_json_file(&sessions.join(format!("{session_id}.json")), &snapshot)
        .await
        .expect("seed archive");
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
            app_previews: Vec::new(),
        })),
    }
}

mod cancel_watchdog;
mod config_auth;
mod config_restart;
mod exec_commands;
mod model_review;
mod secret_store;
mod session_history;
mod session_load_probe;
mod shutdown_turns;
mod spooled_scope;
mod startup_interaction;
mod stop_signal;
mod storage;
mod terminal_pump;
mod turn_queue;
mod update_status;
