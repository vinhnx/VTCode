use std::env;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::progress::{ProgressReporter, ProgressState};
#[allow(unused_imports)]
use super::reasoning::{analyze_reasoning, is_giving_up_reasoning};

use anyhow::Result;
use tokio::sync::{Notify, RwLock, mpsc};
use tokio::task;
use tokio::time::sleep;

use vtcode_core::config::loader::layers::ConfigLayerSource;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::mcp::McpTransportConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::terminal_setup::detector::TerminalType;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::InlineHandle;

use super::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use super::state::{CtrlCState, SessionStats};

type LoadedSkillsMap = hashbrown::HashMap<String, vtcode_core::skills::types::Skill>;

pub(crate) struct SessionStatusContext<'a> {
    pub config: &'a CoreAgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub message_count: usize,
    pub stats: &'a SessionStats,
    pub available_tools: usize,
    pub async_mcp_manager: Option<&'a AsyncMcpManager>,
    pub loaded_skills: &'a Arc<RwLock<LoadedSkillsMap>>,
}

pub(crate) async fn display_session_status(
    renderer: &mut AnsiRenderer,
    ctx: SessionStatusContext<'_>,
) -> Result<()> {
    let session_id = current_session_id();
    let ide_info = detect_ide_info();
    let terminal_info = detect_terminal_info();
    let mcp_info = summarize_mcp_servers(ctx.vt_cfg, ctx.async_mcp_manager).await;
    let skills_info = summarize_loaded_skills(ctx.loaded_skills).await;
    let memory_info = summarize_project_memory(&ctx.config.workspace);
    let settings_info = summarize_setting_sources(&ctx.config.workspace);

    renderer.line(MessageStyle::Info, "Session status:")?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Version: {}", env!("CARGO_PKG_VERSION")),
    )?;
    renderer.line(MessageStyle::Info, &format!("  Session ID: {}", session_id))?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Directory: {}", ctx.config.workspace.display()),
    )?;
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Model: {} ({})", ctx.config.model, ctx.config.provider),
    )?;
    renderer.line(MessageStyle::Info, &format!("  IDE: {}", ide_info))?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Terminal: {}", terminal_info),
    )?;
    renderer.line(MessageStyle::Info, &format!("  MCP servers: {}", mcp_info))?;
    renderer.line(MessageStyle::Info, &format!("  Skills: {}", skills_info))?;
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Info, &format!("  Memory: {}", memory_info))?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Setting sources: {}", settings_info),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Reasoning effort: {}", ctx.config.reasoning_effort),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Messages so far: {}", ctx.message_count),
    )?;

    let used_tools = ctx.stats.sorted_tools();
    if used_tools.is_empty() {
        renderer.line(
            MessageStyle::Info,
            &format!("  Tools used: 0 / {}", ctx.available_tools),
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "  Tools used: {} / {} ({})",
                used_tools.len(),
                ctx.available_tools,
                used_tools.join(", ")
            ),
        )?;
    }

    Ok(())
}

fn current_session_id() -> String {
    crate::main_helpers::runtime_archive_session_id()
        .or_else(|| env::var("VT_SESSION_ID").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn detect_ide_info() -> String {
    let term_program = env::var("TERM_PROGRAM")
        .ok()
        .map(|value| value.to_ascii_lowercase());

    if env::var("ZED_CLI").is_ok() || env::var("VIMRUNTIME").is_ok() {
        return format_tool_version_label("Zed", detect_command_version("zed", &["--version"]));
    }

    if env::var("CURSOR_TRACE_ID").is_ok() || env::var("CURSOR_SESSION_ID").is_ok() {
        return format_tool_version_label(
            "Cursor",
            detect_command_version("cursor", &["--version"]),
        );
    }

    let in_vscode = env::var("VSCODE_PID").is_ok()
        || env::var("VSCODE_IPC_HOOK_CLI").is_ok()
        || term_program
            .as_deref()
            .is_some_and(|value| value.contains("vscode"));
    if in_vscode {
        return format_tool_version_label(
            "VS Code",
            detect_command_version("code", &["--version"]),
        );
    }

    if env::var("JETBRAINS_IDE").is_ok() {
        return "JetBrains (version unknown)".to_string();
    }

    "Not detected".to_string()
}

fn detect_terminal_info() -> String {
    let terminal_type = TerminalType::detect().unwrap_or(TerminalType::Unknown);
    let base_name = terminal_type.name();
    let version = env::var("TERM_PROGRAM_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("WEZTERM_VERSION")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });

    if terminal_type == TerminalType::Unknown {
        if let Ok(term) = env::var("TERM")
            && !term.trim().is_empty()
        {
            return format!("Unknown ({})", term.trim());
        }
        return "Unknown".to_string();
    }

    match version {
        Some(version) => format!("{} {}", base_name, version.trim()),
        None => format!("{} (version unknown)", base_name),
    }
}

fn detect_command_version(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let merged = if !stdout.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };

    let first_line = merged.lines().next()?.trim();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line.to_string())
    }
}

fn format_tool_version_label(name: &str, version: Option<String>) -> String {
    match version {
        Some(version) => format!("{} {}", name, version),
        None => format!("{} (version unknown)", name),
    }
}

async fn summarize_mcp_servers(
    vt_cfg: Option<&VTCodeConfig>,
    async_mcp_manager: Option<&AsyncMcpManager>,
) -> String {
    let mut providers: Vec<String> = Vec::new();
    if let Some(cfg) = vt_cfg {
        if !cfg.mcp.enabled {
            return "disabled".to_string();
        }

        for provider in cfg.mcp.providers.iter().filter(|provider| provider.enabled) {
            let (transport_label, version_label) = match &provider.transport {
                McpTransportConfig::Http(http) => {
                    ("http", format!("protocol {}", http.protocol_version.trim()))
                }
                McpTransportConfig::Stdio(_) => ("stdio", "version unknown".to_string()),
            };
            providers.push(format!(
                "{} ({}, {})",
                provider.name.trim(),
                transport_label,
                version_label
            ));
        }
    }

    let runtime_note = if let Some(manager) = async_mcp_manager {
        match manager.get_status().await {
            McpInitStatus::Ready { client } => {
                let runtime = client.get_status();
                if providers.is_empty() && !runtime.configured_providers.is_empty() {
                    providers.extend(
                        runtime
                            .configured_providers
                            .iter()
                            .map(|name| format!("{} (version unknown)", name.trim())),
                    );
                }
                Some(format!(
                    "active {}/{}",
                    runtime.active_connections, runtime.provider_count
                ))
            }
            McpInitStatus::Initializing { progress } => {
                Some(format!("initializing: {}", progress.trim()))
            }
            McpInitStatus::Error { message } => Some(format!(
                "error: {}",
                truncate_status_value(message.trim(), 80)
            )),
            McpInitStatus::Disabled => Some("disabled".to_string()),
        }
    } else {
        None
    };

    if providers.is_empty() {
        providers.push("none configured".to_string());
    }

    match runtime_note {
        Some(note) => format!("{} [{}]", providers.join(", "), note),
        None => providers.join(", "),
    }
}

async fn summarize_loaded_skills(loaded_skills: &Arc<RwLock<LoadedSkillsMap>>) -> String {
    let skills = loaded_skills.read().await;
    if skills.is_empty() {
        return "none loaded".to_string();
    }

    let mut names: Vec<&str> = skills.keys().map(String::as_str).collect();
    names.sort_unstable();

    let visible_limit = 5;
    if names.len() <= visible_limit {
        return format!("{} loaded ({})", names.len(), names.join(", "));
    }

    let remaining = names.len() - visible_limit;
    format!(
        "{} loaded ({} +{} more)",
        names.len(),
        names[..visible_limit].join(", "),
        remaining
    )
}

fn summarize_project_memory(workspace: &Path) -> String {
    let project_agents_path = workspace.join("AGENTS.md");
    if project_agents_path.exists() {
        "project (AGENTS.md)".to_string()
    } else {
        "project (AGENTS.md not found)".to_string()
    }
}

fn summarize_setting_sources(workspace: &Path) -> String {
    let mut user_sources = Vec::new();
    let mut project_sources = Vec::new();

    if let Ok(manager) = ConfigManager::load_from_workspace(workspace) {
        for layer in manager.layer_stack().layers() {
            match &layer.source {
                ConfigLayerSource::User { file } => {
                    user_sources.push(file.display().to_string());
                }
                ConfigLayerSource::Project { file } | ConfigLayerSource::Workspace { file } => {
                    project_sources.push(file.display().to_string());
                }
                ConfigLayerSource::System { .. } | ConfigLayerSource::Runtime => {}
            }
        }
    }

    let user_label = user_sources
        .first()
        .cloned()
        .unwrap_or_else(|| "defaults".to_string());
    let project_label = project_sources
        .first()
        .cloned()
        .unwrap_or_else(|| workspace.join("vtcode.toml").display().to_string());
    let vtcode_path = workspace.join(".vtcode");
    let vtcode_label = if vtcode_path.exists() {
        vtcode_path.display().to_string()
    } else {
        format!("{} (missing)", vtcode_path.display())
    };

    format!(
        "user: {}, project: {}, .vtcode: {}",
        user_label, project_label, vtcode_label
    )
}

fn truncate_status_value(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.to_string();
    }

    let mut truncated = String::new();
    for ch in value.chars().take(max_len.saturating_sub(3)) {
        truncated.push(ch);
    }
    truncated.push_str("...");
    truncated
}

#[allow(dead_code)]
pub(crate) async fn display_token_cost(
    renderer: &mut AnsiRenderer,
    _max_tokens: usize,
    prefix: &str,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!("{prefix}Token tracking is disabled."),
    )?;
    Ok(())
}

pub(crate) struct PlaceholderGuard {
    handle: InlineHandle,
    restore: Option<String>,
}

impl PlaceholderGuard {
    pub(crate) fn new(handle: &InlineHandle, restore: Option<String>) -> Self {
        Self {
            handle: handle.clone(),
            restore,
        }
    }
}

impl Drop for PlaceholderGuard {
    fn drop(&mut self) {
        self.handle.set_placeholder(self.restore.clone());
    }
}

const SPINNER_UPDATE_INTERVAL_MS: u64 = 150;

#[allow(dead_code)]
pub(crate) struct PlaceholderSpinner {
    handle: InlineHandle,
    restore_left: Option<String>,
    restore_right: Option<String>,
    active: Arc<AtomicBool>,
    task: task::JoinHandle<()>,
    progress_state: Option<Arc<ProgressState>>,
    message_sender: Option<mpsc::UnboundedSender<String>>,
    defer_restore: Arc<AtomicBool>,
}

impl PlaceholderSpinner {
    pub(crate) fn with_progress(
        handle: &InlineHandle,
        restore_left: Option<String>,
        restore_right: Option<String>,
        message: impl Into<String>,
        progress_reporter: Option<&ProgressReporter>,
    ) -> Self {
        let base_message = message.into();
        let message_with_hint = if base_message.is_empty() {
            "Press Ctrl+C to cancel".to_string()
        } else {
            format!("{} (Press Ctrl+C to cancel)", base_message)
        };

        let active = Arc::new(AtomicBool::new(true));
        let spinner_active = active.clone();
        let spinner_handle = handle.clone();
        let restore_on_stop_left = restore_left.clone();
        let restore_on_stop_right = restore_right.clone();
        let status_right = restore_right.clone();
        let progress_reporter_arc = progress_reporter.cloned().map(Arc::new);

        let (message_sender, mut message_receiver) = mpsc::unbounded_channel::<String>();
        let message_sender_clone = message_sender.clone();
        let initial_display = message_with_hint.clone();

        spinner_handle.set_input_status(Some(initial_display.clone()), status_right.clone());

        let task = task::spawn(async move {
            let mut current_message = message_with_hint;
            let mut last_display = initial_display;
            while spinner_active.load(Ordering::SeqCst) {
                while let Ok(new_message) = message_receiver.try_recv() {
                    current_message = if new_message.is_empty() {
                        "Press Ctrl+C to cancel".to_string()
                    } else {
                        format!("{} (Press Ctrl+C to cancel)", new_message)
                    };
                }

                let progress_info = if let Some(progress_reporter) = progress_reporter_arc.as_ref()
                {
                    let progress = progress_reporter.progress_info().await;
                    let mut parts = vec![progress.message.clone()];

                    if progress.total > 0 && progress.percentage > 0 {
                        // Removed progress bar visualization from status bar
                        parts.push(format!("{:.0}%", progress.percentage));
                    }

                    let eta = progress.eta_formatted();
                    if eta != "Calculating..." && eta != "0s" {
                        parts.push(eta);
                    }
                    parts.join("  ")
                } else {
                    String::new()
                };

                let display = if progress_info.is_empty() {
                    current_message.clone()
                } else {
                    format!("{}: {}", current_message, progress_info)
                };

                if display != last_display {
                    spinner_handle.set_input_status(Some(display.clone()), status_right.clone());
                    last_display = display;
                }
                sleep(Duration::from_millis(SPINNER_UPDATE_INTERVAL_MS)).await;
            }

            spinner_handle.set_input_status(restore_on_stop_left, restore_on_stop_right);
        });

        Self {
            handle: handle.clone(),
            restore_left,
            restore_right,
            active,
            task,
            progress_state: progress_reporter.map(|r| r.get_state().clone()),
            message_sender: Some(message_sender_clone),
            defer_restore: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn new(
        handle: &InlineHandle,
        restore_left: Option<String>,
        restore_right: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        let mut spinner = Self::with_progress(handle, restore_left, restore_right, message, None);
        spinner.message_sender = None;
        spinner
    }

    pub(crate) fn set_defer_restore(&self, defer: bool) {
        self.defer_restore.store(defer, Ordering::SeqCst);
    }

    #[allow(dead_code)]
    pub(crate) fn progress_state(&self) -> Option<Arc<ProgressState>> {
        self.progress_state.clone()
    }

    #[allow(dead_code)]
    pub(crate) fn update_message(&self, message: impl Into<String>) {
        if let Some(sender) = &self.message_sender {
            let _ = sender.send(message.into());
        }
    }

    pub(crate) fn finish(&self) {
        self.finish_with_restore(!self.defer_restore.load(Ordering::SeqCst));
    }

    pub(crate) fn set_reasoning_stage(&self, stage: Option<String>) {
        self.handle.set_reasoning_stage(stage);
    }

    pub(crate) fn finish_with_restore(&self, restore: bool) {
        if self.active.swap(false, Ordering::SeqCst) {
            self.task.abort();
            if restore {
                self.handle
                    .set_input_status(self.restore_left.clone(), self.restore_right.clone());
            }
        }
    }
}

impl Drop for PlaceholderSpinner {
    fn drop(&mut self) {
        self.finish();
        self.task.abort();
    }
}

#[derive(Default, Clone, Copy)]
pub(crate) struct StreamSpinnerOptions {
    pub defer_finish: bool,
    pub strip_proposed_plan_blocks: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum StreamProgressEvent {
    OutputDelta(String),
    ReasoningDelta(String),
    ReasoningStage(String),
}

#[allow(dead_code)]
pub(crate) async fn stream_and_render_response(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    stream_and_render_response_with_options(
        provider,
        request,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        StreamSpinnerOptions::default(),
    )
    .await
}

pub(crate) async fn stream_and_render_response_with_options(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    stream_and_render_response_with_options_and_progress(
        provider,
        request,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        options,
        None,
    )
    .await
}

pub(crate) async fn stream_and_render_response_with_options_and_progress(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
    on_progress: Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    super::ui_interaction_stream::stream_and_render_response_with_options_impl(
        provider,
        request,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        options,
        on_progress,
    )
    .await
}
