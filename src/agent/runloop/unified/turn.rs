use anyhow::{Context, Result, anyhow};
use chrono::Local;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::task;

use toml::Value as TomlValue;
#[cfg(debug_assertions)]
use tracing::debug;
use tracing::warn;
use vtcode_core::SimpleIndexer;
use vtcode_core::config::constants::{defaults, ui};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};
use vtcode_core::core::decision_tracker::{Action as DTAction, DecisionOutcome, DecisionTracker};
use vtcode_core::core::router::{Router, TaskClass};
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, classify_error};
use vtcode_core::ui::slash::{SLASH_COMMANDS, SlashCommandInfo};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{InlineEvent, InlineHandle, spawn_session, theme_from_styles};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::{
    self, SessionArchive, SessionArchiveMetadata, SessionMessage,
};
use vtcode_core::utils::transcript;

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::is_context_overflow_error;
use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::prompt::refine_user_prompt_if_enabled;
use crate::agent::runloop::slash_commands::{
    McpCommandAction, SlashCommandOutcome, handle_slash_command,
};
use crate::agent::runloop::text_tools::{detect_textual_tool_call, extract_code_fence_blocks};
use crate::agent::runloop::tool_output::render_code_fence_blocks;
use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};

use super::context_manager::ContextManager;
use super::curator::{build_curator_tools, format_provider_label, resolve_mode_label};
use super::diagnostics::run_doctor_diagnostics;
use super::display::{display_user_message, ensure_turn_bottom_gap, persist_theme_preference};
use super::mcp_support::{
    display_mcp_providers, display_mcp_status, display_mcp_tools, refresh_mcp_tools,
    render_mcp_login_guidance,
};
use super::model_selection::finalize_model_selection;
use super::palettes::{
    ActivePalette, apply_prompt_style, handle_palette_cancel, handle_palette_selection,
    show_help_palette, show_sessions_palette, show_theme_palette,
};
use super::session_setup::{SessionState, initialize_session};
use super::shell::{derive_recent_tool_output, should_short_circuit_shell};
use super::state::{CtrlCSignal, CtrlCState, SessionStats};
use super::status_line::{InputStatusState, update_input_status_if_changed};
use super::tool_pipeline::{ToolExecutionStatus, execute_tool_with_timeout};
use super::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use super::tool_summary::{describe_tool_action, humanize_tool_name, render_tool_call_summary};
use super::ui_interaction::{
    PlaceholderSpinner, display_session_status, display_token_cost, stream_and_render_response,
};
use super::workspace_links::{
    LinkedDirectory, handle_workspace_directory_command, remove_directory_symlink,
};
use crate::agent::runloop::mcp_events;

enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
}

const CONFIG_MODAL_TITLE: &str = "VTCode Configuration";
const MODAL_CLOSE_HINT: &str = "Press Esc to close the configuration modal.";
const SENSITIVE_KEYWORDS: [&str; 5] = ["key", "token", "secret", "password", "credential"];

struct ConfigModalContent {
    title: String,
    source_label: String,
    config_lines: Vec<String>,
}

async fn bootstrap_config_files(workspace: PathBuf, force: bool) -> Result<Vec<String>> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || VTCodeConfig::bootstrap_project(&workspace, force))
        .await
        .map_err(|err| anyhow!("failed to join configuration bootstrap task: {}", err))?;
    result.with_context(|| format!("failed to initialize configuration in {}", label))
}

async fn build_workspace_index(workspace: PathBuf) -> Result<()> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || -> Result<()> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("failed to join workspace indexing task: {}", err))?;
    result.with_context(|| format!("failed to build workspace index in {}", label))
}

async fn load_config_modal_content(
    workspace: PathBuf,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    task::spawn_blocking(move || prepare_config_modal_content(&workspace, vt_cfg))
        .await
        .map_err(|err| anyhow!("failed to join configuration load task: {}", err))?
}

fn prepare_config_modal_content(
    workspace: &Path,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    let manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "failed to resolve configuration for workspace {}",
            workspace.display()
        )
    })?;

    let config_path = manager.config_path().map(Path::to_path_buf);
    let config_data = if config_path.is_some() {
        manager.config().clone()
    } else if let Some(snapshot) = vt_cfg {
        snapshot
    } else {
        manager.config().clone()
    };

    let mut value = TomlValue::try_from(config_data)
        .context("failed to serialize configuration for display")?;
    mask_sensitive_config(&mut value);

    let formatted =
        toml::to_string_pretty(&value).context("failed to render configuration to TOML")?;
    let config_lines = formatted.lines().map(|line| line.to_string()).collect();

    let source_label = if let Some(path) = config_path {
        format!("Configuration source: {}", path.display())
    } else {
        "No vtcode.toml file found; showing runtime defaults.".to_string()
    };

    Ok(ConfigModalContent {
        title: CONFIG_MODAL_TITLE.to_string(),
        source_label,
        config_lines,
    })
}

fn mask_sensitive_config(value: &mut TomlValue) {
    match value {
        TomlValue::Table(table) => {
            for (key, entry) in table.iter_mut() {
                if is_sensitive_key(key) {
                    *entry = TomlValue::String("********".to_string());
                } else {
                    mask_sensitive_config(entry);
                }
            }
        }
        TomlValue::Array(items) => {
            for item in items {
                mask_sensitive_config(item);
            }
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SENSITIVE_KEYWORDS
        .iter()
        .any(|keyword| lowered.contains(keyword))
}

pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    mut vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    resume: Option<ResumeSession>,
) -> Result<()> {
    // Set up panic handler to ensure MCP cleanup on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        eprintln!("Application panic occurred: {:?}", panic_info);
        // Note: We can't easily access the MCP client here due to move semantics
        // The cleanup will happen in the Drop implementations
        original_hook(panic_info);
    }));
    let mut config = config.clone();
    let resume_ref = resume.as_ref();

    let SessionState {
        session_bootstrap,
        mut provider_client,
        mut tool_registry,
        tools,
        trim_config,
        mut conversation_history,
        decision_ledger,
        trajectory: traj,
        base_system_prompt,
        full_auto_allowlist,
        #[allow(unused_variables)]
        mcp_client,
        mut mcp_panel_state,
        token_budget,
        token_budget_enabled,
        mut curator,
    } = initialize_session(&config, vt_cfg.as_ref(), full_auto, resume_ref).await?;

    let curator_tool_catalog = build_curator_tools(&tools);
    let mut context_manager = ContextManager::new(
        base_system_prompt,
        trim_config,
        token_budget,
        token_budget_enabled,
        curator,
        curator_tool_catalog,
    );
    let trim_config = context_manager.trim_config();
    let token_budget_enabled = context_manager.token_budget_enabled();

    let active_styles = theme::active_styles();
    let theme_spec = theme_from_styles(&active_styles);
    let mut default_placeholder = session_bootstrap
        .placeholder
        .clone()
        .or_else(|| Some(ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.to_string()));
    let mut follow_up_placeholder = if session_bootstrap.placeholder.is_none() {
        Some(ui::CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.to_string())
    } else {
        None
    };
    let inline_rows = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.inline_viewport_rows)
        .unwrap_or(ui::DEFAULT_INLINE_VIEWPORT_ROWS);
    let show_timeline_pane = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.show_timeline_pane)
        .unwrap_or(ui::INLINE_SHOW_TIMELINE_PANE);
    let session = spawn_session(
        theme_spec.clone(),
        default_placeholder.clone(),
        config.ui_surface,
        inline_rows,
        show_timeline_pane,
    )
    .context("failed to launch inline session")?;
    let handle = session.handle.clone();
    let highlight_config = vt_cfg
        .as_ref()
        .map(|cfg| cfg.syntax_highlighting.clone())
        .unwrap_or_default();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);

    transcript::clear();

    if let Some(session) = resume.as_ref() {
        let ended_local = session
            .snapshot
            .ended_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M");
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Resuming session {} · ended {} · {} messages",
                session.identifier,
                ended_local,
                session.message_count()
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("Previous archive: {}", session.path.display()),
        )?;
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    let workspace_label = config
        .workspace
        .file_name()
        .and_then(|component| component.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let workspace_path = config.workspace.to_string_lossy().into_owned();
    let provider_label = if config.provider.trim().is_empty() {
        format_provider_label(provider_client.name())
    } else {
        format_provider_label(&config.provider)
    };
    let header_provider_label = provider_label.clone();
    let archive_metadata = SessionArchiveMetadata::new(
        workspace_label,
        workspace_path,
        config.model.clone(),
        provider_label,
        config.theme.clone(),
        config.reasoning_effort.as_str().to_string(),
    );
    let mut session_archive_error: Option<String> = None;
    let mut session_archive = match SessionArchive::new(archive_metadata) {
        Ok(archive) => Some(archive),
        Err(err) => {
            session_archive_error = Some(err.to_string());
            None
        }
    };

    let mut checkpoint_config = SnapshotConfig::new(config.workspace.clone());
    checkpoint_config.enabled = config.checkpointing_enabled;
    checkpoint_config.storage_dir = config.checkpointing_storage_dir.clone();
    checkpoint_config.max_snapshots = config.checkpointing_max_snapshots;
    checkpoint_config.max_age_days = config.checkpointing_max_age_days;

    let checkpoint_manager = match SnapshotManager::new(checkpoint_config) {
        Ok(manager) => Some(manager),
        Err(err) => {
            warn!("Failed to initialize checkpoint manager: {}", err);
            None
        }
    };
    let mut next_checkpoint_turn = checkpoint_manager
        .as_ref()
        .and_then(|manager| manager.next_turn_number().ok())
        .unwrap_or(1);

    handle.set_theme(theme_spec);
    apply_prompt_style(&handle);
    handle.set_placeholder(default_placeholder.clone());

    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());

    render_session_banner(
        &mut renderer,
        &config,
        &session_bootstrap,
        &config.model,
        &reasoning_label,
    )?;
    let mode_label = resolve_mode_label(config.ui_surface, full_auto);
    let header_context = build_inline_header_context(
        &config,
        &session_bootstrap,
        header_provider_label,
        config.model.clone(),
        mode_label,
        reasoning_label.clone(),
    )?;
    handle.set_header_context(header_context);
    // MCP events are now rendered as message blocks in the conversation history

    if let Some(message) = session_archive_error.take() {
        renderer.line(
            MessageStyle::Info,
            &format!("Session archiving disabled: {}", message),
        )?;
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    if full_auto {
        if let Some(allowlist) = full_auto_allowlist.as_ref() {
            if allowlist.is_empty() {
                renderer.line(
                    MessageStyle::Info,
                    "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
                )?;
            } else {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Full-auto mode enabled. Permitted tools: {}",
                        allowlist.join(", ")
                    ),
                )?;
            }
        }
    }

    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mcp_client_for_signal = mcp_client.clone();
    {
        let state = ctrl_c_state.clone();
        let notify = ctrl_c_notify.clone();
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    break;
                }

                let signal = state.register_signal();
                notify.notify_waiters();

                // Shutdown MCP client on interrupt
                if let Some(mcp_client) = &mcp_client_for_signal {
                    if let Err(e) = mcp_client.shutdown().await {
                        let error_msg = e.to_string();
                        if error_msg.contains("EPIPE")
                            || error_msg.contains("Broken pipe")
                            || error_msg.contains("write EPIPE")
                        {
                            eprintln!(
                                "Info: MCP client shutdown encountered pipe errors during interrupt (normal): {}",
                                e
                            );
                        } else {
                            eprintln!("Warning: Failed to shutdown MCP client on interrupt: {}", e);
                        }
                    }
                }

                if matches!(signal, CtrlCSignal::Exit) {
                    break;
                }
            }
        });
    }

    let mut session_stats = SessionStats::default();
    let mut linked_directories: Vec<LinkedDirectory> = Vec::new();
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut events = session.events;
    let mut last_forced_redraw = Instant::now();
    let mut input_status_state = InputStatusState::default();
    loop {
        if let Err(error) = update_input_status_if_changed(
            &handle,
            &config.workspace,
            &config.model,
            config.reasoning_effort.as_str(),
            vt_cfg.as_ref().map(|cfg| &cfg.ui.status_line),
            &mut input_status_state,
        )
        .await
        {
            warn!(
                workspace = %config.workspace.display(),
                error = ?error,
                "Failed to refresh status line"
            );
        }
        if ctrl_c_state.is_exit_requested() {
            break;
        }

        let maybe_event = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => None,
            event = events.recv() => event,
        };

        if ctrl_c_state.is_cancel_requested() {
            if ctrl_c_state.is_exit_requested() {
                break;
            }

            renderer.line_if_not_empty(MessageStyle::Output)?;
            renderer.line(
                MessageStyle::Info,
                "Interrupted current task. Press Ctrl+C again to exit.",
            )?;
            handle.clear_input();
            handle.set_placeholder(default_placeholder.clone());
            ctrl_c_state.clear_cancel();
            continue;
        }

        let Some(event) = maybe_event else {
            break;
        };

        ctrl_c_state.disarm_exit();

        let submitted = match event {
            InlineEvent::Submit(text) => text,
            InlineEvent::ListModalSubmit(selection) => {
                if let Some(picker) = model_picker_state.as_mut() {
                    let progress =
                        picker.handle_list_selection(&mut renderer, selection.clone())?;
                    match progress {
                        ModelPickerProgress::InProgress => {}
                        ModelPickerProgress::Cancelled => {
                            model_picker_state = None;
                            renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                        }
                        ModelPickerProgress::Completed(selection) => {
                            let picker_state = model_picker_state.take().unwrap();
                            if let Err(err) = finalize_model_selection(
                                &mut renderer,
                                &picker_state,
                                selection,
                                &mut config,
                                &mut vt_cfg,
                                &mut provider_client,
                                &session_bootstrap,
                                &handle,
                                full_auto,
                            ) {
                                renderer.line(
                                    MessageStyle::Error,
                                    &format!("Failed to apply model selection: {}", err),
                                )?;
                            }
                        }
                    }
                }
                if let Some(active) = palette_state.take() {
                    let restore =
                        handle_palette_selection(active, selection, &mut renderer, &handle)?;
                    if let Some(state) = restore {
                        palette_state = Some(state);
                    }
                }
                continue;
            }
            InlineEvent::ListModalCancel => {
                if let Some(_) = model_picker_state.take() {
                    renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                } else if let Some(active) = palette_state.take() {
                    handle_palette_cancel(active, &mut renderer)?;
                }
                continue;
            }
            InlineEvent::Cancel => {
                renderer.line(
                    MessageStyle::Info,
                    "Cancellation request noted. No active run to stop.",
                )?;
                continue;
            }
            InlineEvent::Exit => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            InlineEvent::Interrupt => {
                break;
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown => continue,
        };

        let input_owned = submitted.trim().to_string();

        if input_owned.is_empty() {
            continue;
        }

        if let Some(next_placeholder) = follow_up_placeholder.take() {
            handle.set_placeholder(Some(next_placeholder.clone()));
            default_placeholder = Some(next_placeholder);
        }

        match input_owned.as_str() {
            "" => continue,
            "exit" | "quit" => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            "help" => {
                renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                continue;
            }
            input if input.starts_with('/') => {
                // Handle slash commands
                if let Some(command_input) = input.strip_prefix('/') {
                    match handle_slash_command(command_input, &mut renderer)? {
                        SlashCommandOutcome::Handled => {
                            continue;
                        }
                        SlashCommandOutcome::ThemeChanged(theme_id) => {
                            persist_theme_preference(&mut renderer, &theme_id)?;
                            let styles = theme::active_styles();
                            handle.set_theme(theme_from_styles(&styles));
                            apply_prompt_style(&handle);
                            continue;
                        }
                        SlashCommandOutcome::StartThemePalette { mode } => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Close the active model picker before selecting a theme.",
                                )?;
                                continue;
                            }
                            if palette_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
                                )?;
                                continue;
                            }
                            if show_theme_palette(&mut renderer, mode)? {
                                palette_state = Some(ActivePalette::Theme { mode });
                            }
                            continue;
                        }
                        SlashCommandOutcome::StartSessionsPalette { limit } => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Close the active model picker before browsing sessions.",
                                )?;
                                continue;
                            }
                            if palette_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Another selection modal is already open. Press Esc to close it before continuing.",
                                )?;
                                continue;
                            }

                            match session_archive::list_recent_sessions(limit) {
                                Ok(listings) => {
                                    if show_sessions_palette(&mut renderer, &listings, limit)? {
                                        palette_state =
                                            Some(ActivePalette::Sessions { listings, limit });
                                    }
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to load session archives: {}", err),
                                    )?;
                                }
                            }
                            continue;
                        }
                        SlashCommandOutcome::StartHelpPalette => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Close the active model picker before opening help.",
                                )?;
                                continue;
                            }
                            if palette_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
                                )?;
                                continue;
                            }
                            let commands: Vec<&'static SlashCommandInfo> =
                                SLASH_COMMANDS.iter().collect();
                            if show_help_palette(&mut renderer, &commands)? {
                                palette_state = Some(ActivePalette::Help);
                            }
                            continue;
                        }
                        SlashCommandOutcome::StartModelSelection => {
                            if model_picker_state.is_some() {
                                renderer.line(
                                    MessageStyle::Error,
                                    "A model picker session is already active. Complete or type 'cancel' to exit it before starting another.",
                                )?;
                                continue;
                            }
                            let reasoning = vt_cfg
                                .as_ref()
                                .map(|cfg| cfg.agent.reasoning_effort)
                                .unwrap_or(config.reasoning_effort);
                            let workspace_hint = Some(config.workspace.clone());
                            match ModelPickerState::new(&mut renderer, reasoning, workspace_hint) {
                                Ok(picker) => {
                                    model_picker_state = Some(picker);
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to start model picker: {}", err),
                                    )?;
                                }
                            }
                            continue;
                        }
                        SlashCommandOutcome::InitializeWorkspace { force } => {
                            let workspace_path = config.workspace.clone();
                            let workspace_label = workspace_path.display().to_string();
                            renderer.line(
                                MessageStyle::Info,
                                &format!(
                                    "Initializing vtcode configuration in {}...",
                                    workspace_label
                                ),
                            )?;

                            let created_files =
                                match bootstrap_config_files(workspace_path.clone(), force).await {
                                    Ok(files) => files,
                                    Err(err) => {
                                        renderer.line(
                                            MessageStyle::Error,
                                            &format!("Failed to initialize configuration: {}", err),
                                        )?;
                                        continue;
                                    }
                                };

                            if created_files.is_empty() {
                                renderer.line(
                                    MessageStyle::Info,
                                    "Existing configuration detected; no files were changed.",
                                )?;
                            } else {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Created {}: {}",
                                        if created_files.len() == 1 {
                                            "file"
                                        } else {
                                            "files"
                                        },
                                        created_files.join(", "),
                                    ),
                                )?;
                            }

                            renderer.line(
                                MessageStyle::Info,
                                "Indexing workspace context (this may take a moment)...",
                            )?;

                            match build_workspace_index(workspace_path.clone()).await {
                                Ok(()) => {
                                    renderer.line(
                                        MessageStyle::Info,
                                        "Workspace indexing complete. Stored under .vtcode/index.",
                                    )?;
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to index workspace: {}", err),
                                    )?;
                                }
                            }

                            continue;
                        }
                        SlashCommandOutcome::ShowConfig => {
                            let workspace_path = config.workspace.clone();
                            let vt_snapshot = vt_cfg.clone();
                            match load_config_modal_content(workspace_path, vt_snapshot).await {
                                Ok(content) => {
                                    if renderer.prefers_untruncated_output() {
                                        let mut modal_lines = Vec::new();
                                        modal_lines.push(content.source_label.clone());
                                        modal_lines.push(String::new());
                                        modal_lines.extend(content.config_lines.clone());
                                        modal_lines.push(String::new());
                                        modal_lines.push(MODAL_CLOSE_HINT.to_string());
                                        handle.close_modal();
                                        handle.show_modal(content.title.clone(), modal_lines, None);
                                        renderer.line(
                                            MessageStyle::Info,
                                            &format!(
                                                "Opened {} modal ({}).",
                                                content.title, content.source_label
                                            ),
                                        )?;
                                        renderer.line(MessageStyle::Info, MODAL_CLOSE_HINT)?;
                                    } else {
                                        renderer.line(MessageStyle::Info, &content.source_label)?;
                                        for line in content.config_lines {
                                            renderer.line(MessageStyle::Info, &line)?;
                                        }
                                    }
                                }
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!(
                                            "Failed to load configuration for display: {}",
                                            err
                                        ),
                                    )?;
                                }
                            }
                            continue;
                        }
                        SlashCommandOutcome::ExecuteTool { name, args } => {
                            // Handle tool execution from slash command
                            match ensure_tool_permission(
                                &mut tool_registry,
                                &name,
                                Some(&args),
                                &mut renderer,
                                &handle,
                                &mut events,
                                default_placeholder.clone(),
                                &ctrl_c_state,
                                &ctrl_c_notify,
                            )
                            .await
                            {
                                Ok(ToolPermissionFlow::Approved) => {
                                    // Tool execution logic
                                    continue;
                                }
                                Ok(ToolPermissionFlow::Denied) => continue,
                                Ok(ToolPermissionFlow::Exit) => break,
                                Ok(ToolPermissionFlow::Interrupted) => break,
                                Err(err) => {
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!(
                                            "Failed to evaluate policy for tool '{}': {}",
                                            name, err
                                        ),
                                    )?;
                                    continue;
                                }
                            }
                        }
                        SlashCommandOutcome::ClearConversation => {
                            conversation_history.clear();
                            session_stats = SessionStats::default();
                            context_manager.clear_curator_state();
                            {
                                let mut ledger = decision_ledger.write().await;
                                *ledger = DecisionTracker::new();
                            }
                            context_manager.reset_token_budget().await;
                            transcript::clear();
                            renderer.line(
                                MessageStyle::Info,
                                "Cleared conversation history and token statistics.",
                            )?;
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            continue;
                        }
                        SlashCommandOutcome::ShowStatus => {
                            let token_budget = context_manager.token_budget();
                            display_session_status(
                                &mut renderer,
                                &config,
                                conversation_history.len(),
                                &session_stats,
                                token_budget.as_ref(),
                                token_budget_enabled,
                                trim_config.max_tokens,
                                tools.len(),
                            )
                            .await?;
                            continue;
                        }
                        SlashCommandOutcome::ShowCost => {
                            let token_budget = context_manager.token_budget();
                            renderer.line(MessageStyle::Info, "Token usage summary:")?;
                            display_token_cost(
                                &mut renderer,
                                token_budget.as_ref(),
                                token_budget_enabled,
                                trim_config.max_tokens,
                                "",
                            )
                            .await?;
                            continue;
                        }
                        SlashCommandOutcome::ManageMcp { action } => {
                            match action {
                                McpCommandAction::Overview => {
                                    display_mcp_status(
                                        &mut renderer,
                                        &session_bootstrap,
                                        &mut tool_registry,
                                        mcp_client.as_ref(),
                                        &mcp_panel_state,
                                    )
                                    .await?;
                                }
                                McpCommandAction::ListProviders => {
                                    display_mcp_providers(
                                        &mut renderer,
                                        &session_bootstrap,
                                        mcp_client.as_ref(),
                                    )?;
                                }
                                McpCommandAction::ListTools => {
                                    display_mcp_tools(&mut renderer, &mut tool_registry).await?;
                                }
                                McpCommandAction::RefreshTools => {
                                    refresh_mcp_tools(&mut renderer, &mut tool_registry).await?;
                                }
                                McpCommandAction::Login(name) => {
                                    render_mcp_login_guidance(&mut renderer, name, true)?;
                                }
                                McpCommandAction::Logout(name) => {
                                    render_mcp_login_guidance(&mut renderer, name, false)?;
                                }
                            }
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            continue;
                        }
                        SlashCommandOutcome::RunDoctor => {
                            let provider_runtime = provider_client.name().to_string();
                            run_doctor_diagnostics(
                                &mut renderer,
                                &config,
                                vt_cfg.as_ref(),
                                &provider_runtime,
                                mcp_client.as_ref(),
                                &linked_directories,
                            )?;
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            continue;
                        }
                        SlashCommandOutcome::ManageWorkspaceDirectories { command } => {
                            handle_workspace_directory_command(
                                &mut renderer,
                                &config.workspace,
                                command,
                                &mut linked_directories,
                            )?;
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            continue;
                        }
                        SlashCommandOutcome::Exit => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break;
                        }
                    }
                }
                continue;
            }
            _ => {}
        }

        if let Some(picker) = model_picker_state.as_mut() {
            let progress = picker.handle_input(&mut renderer, input_owned.as_str())?;
            match progress {
                ModelPickerProgress::InProgress => continue,
                ModelPickerProgress::Cancelled => {
                    model_picker_state = None;
                    continue;
                }
                ModelPickerProgress::Completed(selection) => {
                    let picker_state = model_picker_state.take().unwrap();
                    if let Err(err) = finalize_model_selection(
                        &mut renderer,
                        &picker_state,
                        selection,
                        &mut config,
                        &mut vt_cfg,
                        &mut provider_client,
                        &session_bootstrap,
                        &handle,
                        full_auto,
                    ) {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to apply model selection: {}", err),
                        )?;
                    }
                    continue;
                }
            }
        }

        let input = input_owned.as_str();

        let refined_user = refine_user_prompt_if_enabled(input, &config, vt_cfg.as_ref()).await;
        // Display the user message with inline border decoration
        display_user_message(&mut renderer, &refined_user)?;
        conversation_history.push(uni::Message::user(refined_user.clone()));
        let _pruned_tools = context_manager.prune_tool_responses(&mut conversation_history);
        // Removed: Tool response pruning message
        let trim_result = context_manager.enforce_context_window(&mut conversation_history);
        if trim_result.is_trimmed() {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                    trim_result.removed_messages, trim_config.max_tokens,
                ),
            )?;
        }

        let mut working_history = conversation_history.clone();
        let max_tool_loops = vt_cfg
            .as_ref()
            .map(|cfg| cfg.tools.max_tool_loops)
            .filter(|&value| value > 0)
            .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);

        let mut loop_guard = 0usize;
        let mut any_write_effect = false;
        let mut last_tool_stdout: Option<String> = None;
        let mut bottom_gap_applied = false;
        let mut turn_modified_files: BTreeSet<PathBuf> = BTreeSet::new();
        let tool_repeat_limit = vt_cfg
            .as_ref()
            .map(|cfg| cfg.tools.max_repeated_tool_calls)
            .filter(|&value| value > 0)
            .unwrap_or(defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS);
        let mut repeated_tool_attempts: HashMap<String, usize> = HashMap::new();

        let turn_result = 'outer: loop {
            if ctrl_c_state.is_cancel_requested() {
                break TurnLoopResult::Cancelled;
            }
            if loop_guard == 0 {
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
            loop_guard += 1;
            if loop_guard >= max_tool_loops {
                if !bottom_gap_applied {
                    renderer.line(MessageStyle::Output, "")?;
                }
                let notice = format!(
                    "I reached the configured tool-call limit of {} for this turn and paused further tool execution. Increase `tools.max_tool_loops` in vtcode.toml if you need more, then ask me to continue.",
                    max_tool_loops
                );
                renderer.line(MessageStyle::Error, &notice)?;
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(notice));
                break TurnLoopResult::Completed;
            }

            let _ = context_manager.enforce_context_window(&mut working_history);

            let decision = if let Some(cfg) = vt_cfg.as_ref().filter(|cfg| cfg.router.enabled) {
                Router::route_async(cfg, &config, &config.api_key, input).await
            } else {
                Router::route(&VTCodeConfig::default(), &config, input)
            };
            traj.log_route(
                working_history.len(),
                &decision.selected_model,
                match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                },
                &input.chars().take(120).collect::<String>(),
            );

            let active_model = decision.selected_model;
            let (max_tokens_opt, parallel_cfg_opt) = if let Some(vt) = vt_cfg.as_ref() {
                let key = match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                };
                let budget = vt.router.budgets.get(key);
                let max_tokens = budget.and_then(|b| b.max_tokens).map(|value| value as u32);
                let parallel = budget.and_then(|b| b.max_parallel_tools).map(|value| {
                    vtcode_core::llm::provider::ParallelToolConfig {
                        disable_parallel_tool_use: value <= 1,
                        max_parallel_tools: Some(value),
                        encourage_parallel: value > 1,
                    }
                });
                (max_tokens, parallel)
            } else {
                (None, None)
            };

            {
                let mut ledger = decision_ledger.write().await;
                ledger.start_turn(
                    working_history.len(),
                    working_history
                        .last()
                        .map(|message| message.content.clone()),
                );
                let tool_names: Vec<String> = tools
                    .iter()
                    .map(|tool| tool.function.name.clone())
                    .collect();
                ledger.update_available_tools(tool_names);
            }

            let mut attempt_history = working_history.clone();
            let mut retry_attempts = 0usize;
            let (response, response_streamed) = loop {
                retry_attempts += 1;
                let _ = context_manager.enforce_context_window(&mut attempt_history);
                context_manager.reset_token_budget().await;
                let system_prompt = context_manager
                    .build_system_prompt(&attempt_history, retry_attempts)
                    .await?;

                let use_streaming = provider_client.supports_streaming();
                let reasoning_effort = vt_cfg.as_ref().and_then(|cfg| {
                    if provider_client.supports_reasoning_effort(&active_model) {
                        Some(cfg.agent.reasoning_effort)
                    } else {
                        None
                    }
                });
                let request = uni::LLMRequest {
                    messages: attempt_history.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    tools: Some(tools.clone()),
                    model: active_model.clone(),
                    max_tokens: max_tokens_opt.or(Some(2000)),
                    temperature: Some(0.7),
                    stream: use_streaming,
                    tool_choice: Some(uni::ToolChoice::auto()),
                    parallel_tool_calls: None,
                    parallel_tool_config: if provider_client
                        .supports_parallel_tool_config(&active_model)
                    {
                        parallel_cfg_opt.clone()
                    } else {
                        None
                    },
                    reasoning_effort,
                };

                let thinking_spinner =
                    PlaceholderSpinner::new(&handle, default_placeholder.clone(), "Thinking...");
                task::yield_now().await;
                #[cfg(debug_assertions)]
                let request_timer = Instant::now();
                #[cfg(debug_assertions)]
                {
                    let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
                    debug!(
                        target = "vtcode::agent::llm",
                        model = %request.model,
                        streaming = use_streaming,
                        attempt = retry_attempts,
                        messages = request.messages.len(),
                        tools = tool_count,
                        "Dispatching provider request"
                    );
                }
                let result = if use_streaming {
                    let outcome = stream_and_render_response(
                        provider_client.as_ref(),
                        request,
                        &thinking_spinner,
                        &mut renderer,
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    )
                    .await;
                    outcome
                } else {
                    let provider_name = provider_client.name().to_string();
                    let generate_future = provider_client.generate(request);
                    tokio::pin!(generate_future);
                    let cancel_notifier = ctrl_c_notify.notified();
                    tokio::pin!(cancel_notifier);
                    let outcome = tokio::select! {
                        res = &mut generate_future => {
                            thinking_spinner.finish();
                            res.map(|resp| (resp, false))
                        }
                        _ = &mut cancel_notifier => {
                            thinking_spinner.finish();
                            Err(uni::LLMError::Provider(error_display::format_llm_error(
                                &provider_name,
                                "Interrupted by user",
                            )))
                        }
                    };
                    outcome
                };

                #[cfg(debug_assertions)]
                {
                    debug!(
                        target = "vtcode::agent::llm",
                        model = %active_model,
                        streaming = use_streaming,
                        attempt = retry_attempts,
                        elapsed_ms = request_timer.elapsed().as_millis(),
                        succeeded = result.is_ok(),
                        "Provider request finished"
                    );
                }

                match result {
                    Ok((result, streamed_tokens)) => {
                        working_history = attempt_history.clone();
                        break (result, streamed_tokens);
                    }
                    Err(error) => {
                        if ctrl_c_state.is_cancel_requested() {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        let error_text = error.to_string();
                        if is_context_overflow_error(&error_text)
                            && retry_attempts <= vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT
                        {
                            let removed_tool_messages =
                                context_manager.prune_tool_responses(&mut attempt_history);
                            let removed_turns =
                                context_manager.aggressive_trim(&mut attempt_history);
                            let total_removed = removed_tool_messages + removed_turns;
                            if total_removed > 0 {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Context overflow detected; removed {} older messages (retry {}/{}).",
                                        total_removed,
                                        retry_attempts,
                                        vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT,
                                    ),
                                )?;
                                conversation_history.clone_from(&attempt_history);
                                continue;
                            }
                        }

                        let has_tool = working_history
                            .iter()
                            .any(|msg| msg.role == uni::MessageRole::Tool);

                        if has_tool {
                            eprintln!("Provider error (suppressed): {error_text}");
                            let reply = derive_recent_tool_output(&working_history)
                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                            renderer.line(MessageStyle::Response, &reply)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            working_history.push(uni::Message::assistant(reply));
                            let _ = last_tool_stdout.take();
                            break 'outer TurnLoopResult::Completed;
                        } else {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Provider error: {error_text}"),
                            )?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            break 'outer TurnLoopResult::Aborted;
                        }
                    }
                }
            };

            let mut final_text = response.content.clone();
            let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
            let mut interpreted_textual_call = false;
            let reasoning_trace = response.reasoning.clone();

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
                && let Some((name, args)) = detect_textual_tool_call(&text)
            {
                let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                let code_blocks = extract_code_fence_blocks(&text);
                if !code_blocks.is_empty() {
                    render_code_fence_blocks(&mut renderer, &code_blocks)?;
                    renderer.line(MessageStyle::Output, "")?;
                }
                let (headline, _) = describe_tool_action(&name, &args);
                let notice = if headline.is_empty() {
                    format!("Detected {} request", humanize_tool_name(&name))
                } else {
                    format!("Detected {headline}")
                };
                renderer.line(MessageStyle::Info, &notice)?;
                let call_id = format!("call_textual_{}", working_history.len());
                tool_calls.push(uni::ToolCall::function(
                    call_id.clone(),
                    name.clone(),
                    args_json.clone(),
                ));
                interpreted_textual_call = true;
                final_text = None;
            }

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
            {
                let message = uni::Message::assistant(text).with_reasoning(reasoning_trace.clone());
                working_history.push(message);
            } else {
                let assistant_text = if interpreted_textual_call {
                    String::new()
                } else {
                    final_text.clone().unwrap_or_default()
                };
                let message =
                    uni::Message::assistant_with_tools(assistant_text, tool_calls.clone())
                        .with_reasoning(reasoning_trace.clone());
                working_history.push(message);
                for call in &tool_calls {
                    let name = call.function.name.as_str();
                    let args_val = call
                        .parsed_arguments()
                        .unwrap_or_else(|_| serde_json::json!({}));
                    let signature_key = format!(
                        "{}::{}",
                        name,
                        serde_json::to_string(&args_val).unwrap_or_else(|_| "{}".to_string())
                    );
                    let attempts = repeated_tool_attempts
                        .entry(signature_key.clone())
                        .or_insert(0);
                    *attempts += 1;
                    let current_attempts = *attempts;
                    if current_attempts > tool_repeat_limit {
                        renderer.line(
                            MessageStyle::Error,
                            &format!(
                                "Aborting repeated tool call '{}' after {} unsuccessful attempts with identical arguments.",
                                name,
                                current_attempts - 1
                            ),
                        )?;
                        ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                        working_history.push(uni::Message::assistant(
                            format!(
                                "I stopped because tool '{}' kept failing with the same arguments. Please adjust the request or ask me to continue a different way.",
                                name
                            ),
                        ));
                        break 'outer TurnLoopResult::Completed;
                    }

                    // Render MCP tool calls as assistant messages instead of user input
                    if name.starts_with("mcp_") {
                        let tool_name = &name[4..]; // Remove "mcp_" prefix
                        let (headline, _) = describe_tool_action(tool_name, &args_val);

                        // Render MCP tool call as a single message block
                        renderer.line(MessageStyle::Info, &headline)?;
                        renderer.line(MessageStyle::Info, &format!("MCP: {}", tool_name))?;

                        // Force immediate TUI refresh to ensure proper layout
                        handle.force_redraw();
                        tokio::time::sleep(Duration::from_millis(10)).await;

                        // Also capture for logging
                        {
                            let mut mcp_event = mcp_events::McpEvent::new(
                                "mcp".to_string(),
                                tool_name.to_string(),
                                Some(args_val.to_string()),
                            );
                            mcp_event.success(None);
                            mcp_panel_state.add_event(mcp_event);
                        }
                    } else {
                        render_tool_call_summary(&mut renderer, name, &args_val)?;
                    }
                    let dec_id = {
                        let mut ledger = decision_ledger.write().await;
                        ledger.record_decision(
                            format!("Execute tool '{}' to progress task", name),
                            DTAction::ToolCall {
                                name: name.to_string(),
                                args: args_val.clone(),
                                expected_outcome: "Use tool output to decide next step".to_string(),
                            },
                            None,
                        )
                    };

                    match ensure_tool_permission(
                        &mut tool_registry,
                        name,
                        Some(&args_val),
                        &mut renderer,
                        &handle,
                        &mut events,
                        default_placeholder.clone(),
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    )
                    .await
                    {
                        Ok(ToolPermissionFlow::Approved) => {
                            let tool_spinner = PlaceholderSpinner::new(
                                &handle,
                                default_placeholder.clone(),
                                format!("Running tool: {}", name),
                            );

                            // Force TUI refresh to ensure display stability
                            safe_force_redraw(&handle, &mut last_forced_redraw);

                            let pipeline_outcome = execute_tool_with_timeout(
                                &mut tool_registry,
                                name,
                                args_val.clone(),
                            )
                            .await;

                            match pipeline_outcome {
                                ToolExecutionStatus::Success {
                                    output,
                                    stdout,
                                    modified_files,
                                    command_success,
                                    has_more,
                                } => {
                                    tool_spinner.finish();

                                    safe_force_redraw(&handle, &mut last_forced_redraw);
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    session_stats.record_tool(name);
                                    repeated_tool_attempts.remove(&signature_key);
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        true,
                                    );

                                    if name.starts_with("mcp_") {
                                        let tool_name = &name[4..];
                                        renderer.line_if_not_empty(MessageStyle::Output)?;
                                        renderer.line(
                                            MessageStyle::Info,
                                            &format!("MCP tool {} completed", tool_name),
                                        )?;
                                        handle.force_redraw();
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        let mut mcp_event = mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            tool_name.to_string(),
                                            Some(args_val.to_string()),
                                        );
                                        mcp_event.success(None);
                                        mcp_panel_state.add_event(mcp_event);
                                    }

                                    render_tool_output(
                                        &mut renderer,
                                        Some(name),
                                        &output,
                                        vt_cfg.as_ref(),
                                    )?;
                                    last_tool_stdout = if command_success {
                                        stdout.clone()
                                    } else {
                                        None
                                    };

                                    if matches!(
                                        name,
                                        "write_file"
                                            | "edit_file"
                                            | "create_file"
                                            | "delete_file"
                                            | "srgn"
                                    ) {
                                        any_write_effect = true;
                                    }

                                    if !modified_files.is_empty()
                                        && confirm_changes_with_git_diff(
                                            &modified_files,
                                            skip_confirmations,
                                        )
                                        .await?
                                    {
                                        renderer.line(
                                            MessageStyle::Info,
                                            "Changes applied successfully.",
                                        )?;
                                        turn_modified_files
                                            .extend(modified_files.iter().map(PathBuf::from));
                                    } else if !modified_files.is_empty() {
                                        renderer.line(MessageStyle::Info, "Changes discarded.")?;
                                    }

                                    let mut notice_lines: Vec<String> = Vec::new();
                                    if !modified_files.is_empty() {
                                        notice_lines.push("Files touched:".to_string());
                                        for file in &modified_files {
                                            notice_lines.push(format!("  - {}", file));
                                        }
                                        if let Some(stdout_preview) = &last_tool_stdout {
                                            let preview: String =
                                                stdout_preview.chars().take(80).collect();
                                            notice_lines
                                                .push(format!("stdout preview: {}", preview));
                                        }
                                    }
                                    if let Some(notice) =
                                        output.get("notice").and_then(|value| value.as_str())
                                    {
                                        if !notice.trim().is_empty() {
                                            notice_lines.push(notice.trim().to_string());
                                        }
                                    }
                                    if !notice_lines.is_empty() {
                                        renderer.line(MessageStyle::Info, "")?;
                                        for line in notice_lines {
                                            renderer.line(MessageStyle::Info, &line)?;
                                        }
                                    }

                                    let content = serde_json::to_string(&output)
                                        .unwrap_or_else(|_| "{}".to_string());
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        content,
                                    ));
                                    {
                                        let mut ledger = decision_ledger.write().await;
                                        ledger.record_outcome(
                                            &dec_id,
                                            DecisionOutcome::Success {
                                                result: "tool_ok".to_string(),
                                                metrics: Default::default(),
                                            },
                                        );
                                    }

                                    if has_more {
                                        loop_guard = loop_guard.saturating_sub(1);
                                    }

                                    if command_success
                                        && should_short_circuit_shell(input, name, &args_val)
                                    {
                                        let reply = last_tool_stdout.clone().unwrap_or_else(|| {
                                            "Command completed successfully.".to_string()
                                        });
                                        renderer.line(MessageStyle::Response, &reply)?;
                                        ensure_turn_bottom_gap(
                                            &mut renderer,
                                            &mut bottom_gap_applied,
                                        )?;
                                        working_history.push(uni::Message::assistant(reply));
                                        let _ = last_tool_stdout.take();
                                        break 'outer TurnLoopResult::Completed;
                                    }
                                }
                                ToolExecutionStatus::Failure { error } => {
                                    tool_spinner.finish();

                                    safe_force_redraw(&handle, &mut last_forced_redraw);
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    session_stats.record_tool(name);
                                    renderer.line(
                                        MessageStyle::Tool,
                                        &format!("Tool {} failed.", name),
                                    )?;
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        false,
                                    );

                                    let error_chain: Vec<String> =
                                        error.chain().map(|cause| cause.to_string()).collect();
                                    let error_summary = error_chain
                                        .first()
                                        .cloned()
                                        .unwrap_or_else(|| "unknown tool error".to_string());

                                    let original_details = if error_chain.len() <= 1 {
                                        error_summary.clone()
                                    } else {
                                        error_chain.join(" -> ")
                                    };
                                    let classified = classify_error(&error);
                                    let structured = ToolExecutionError::with_original_error(
                                        name.to_string(),
                                        classified,
                                        error_summary.clone(),
                                        original_details,
                                    );
                                    let error_json = structured.to_json_value();
                                    let error_message = structured.message.clone();

                                    if name.starts_with("mcp_") {
                                        let tool_name = &name[4..];
                                        renderer.line_if_not_empty(MessageStyle::Output)?;
                                        renderer.line(
                                            MessageStyle::Error,
                                            &format!(
                                                "MCP tool {} failed: {}",
                                                tool_name, error_message
                                            ),
                                        )?;
                                        handle.force_redraw();
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        let mut mcp_event = mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            tool_name.to_string(),
                                            Some(args_val.to_string()),
                                        );
                                        mcp_event.failure(Some(error_message.clone()));
                                        mcp_panel_state.add_event(mcp_event);
                                    }

                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool error: {error_message}"),
                                    )?;
                                    render_tool_output(
                                        &mut renderer,
                                        Some(name),
                                        &error_json,
                                        vt_cfg.as_ref(),
                                    )?;
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        serde_json::to_string(&error_json)
                                            .unwrap_or_else(|_| "{}".to_string()),
                                    ));
                                    let _ = last_tool_stdout.take();
                                    {
                                        let mut ledger = decision_ledger.write().await;
                                        ledger.record_outcome(
                                            &dec_id,
                                            DecisionOutcome::Failure {
                                                error: error_message,
                                                recovery_attempts: 0,
                                                context_preserved: true,
                                            },
                                        );
                                    }
                                }
                                ToolExecutionStatus::Timeout { error } => {
                                    tool_spinner.finish();

                                    handle.force_redraw();
                                    tokio::time::sleep(Duration::from_millis(10)).await;

                                    session_stats.record_tool(name);
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool {} timed out after 5 minutes.", name),
                                    )?;
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        false,
                                    );

                                    let err_json = error.to_json_value();
                                    let error_message = error.message.clone();
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        serde_json::to_string(&err_json)
                                            .unwrap_or_else(|_| "{}".to_string()),
                                    ));
                                    let _ = last_tool_stdout.take();
                                    {
                                        let mut ledger = decision_ledger.write().await;
                                        ledger.record_outcome(
                                            &dec_id,
                                            DecisionOutcome::Failure {
                                                error: error_message,
                                                recovery_attempts: 0,
                                                context_preserved: true,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        Ok(ToolPermissionFlow::Denied) => {
                            session_stats.record_tool(name);
                            let denial = ToolExecutionError::new(
                                name.to_string(),
                                ToolErrorType::PolicyViolation,
                                format!("Tool '{}' execution denied by policy", name),
                            )
                            .to_json_value();
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            render_tool_output(
                                &mut renderer,
                                Some(name),
                                &denial,
                                vt_cfg.as_ref(),
                            )?;
                            let content =
                                serde_json::to_string(&denial).unwrap_or("{}".to_string());
                            working_history
                                .push(uni::Message::tool_response(call.id.clone(), content));
                            {
                                let mut ledger = decision_ledger.write().await;
                                ledger.record_outcome(
                                    &dec_id,
                                    DecisionOutcome::Failure {
                                        error: format!(
                                            "Tool '{}' execution denied by policy",
                                            name
                                        ),
                                        recovery_attempts: 0,
                                        context_preserved: true,
                                    },
                                );
                            }
                            continue;
                        }
                        Ok(ToolPermissionFlow::Exit) => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Ok(ToolPermissionFlow::Interrupted) => {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Err(err) => {
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
                            )?;
                            let err_json = serde_json::json!({
                                "error": format!(
                                    "Policy evaluation error for '{}' : {}",
                                    name, err
                                )
                            });
                            working_history.push(uni::Message::tool_response(
                                call.id.clone(),
                                err_json.to_string(),
                            ));
                            let _ = last_tool_stdout.take();
                            {
                                let mut ledger = decision_ledger.write().await;
                                ledger.record_outcome(
                                    &dec_id,
                                    DecisionOutcome::Failure {
                                        error: format!(
                                            "Failed to evaluate policy for tool '{}': {}",
                                            name, err
                                        ),
                                        recovery_attempts: 0,
                                        context_preserved: true,
                                    },
                                );
                            }
                            continue;
                        }
                    }
                }
                continue;
            }

            if let Some(mut text) = final_text.clone() {
                let do_review = vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.agent.enable_self_review)
                    .unwrap_or(false);
                let review_passes = vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.agent.max_review_passes)
                    .unwrap_or(1)
                    .max(1);
                if do_review {
                    let review_system = "You are the agent's critical code reviewer. Improve clarity, correctness, and add missing test or validation guidance. Return only the improved final answer (no meta commentary).".to_string();
                    for _ in 0..review_passes {
                        let review_req = uni::LLMRequest {
                            messages: vec![uni::Message::user(format!(
                                "Please review and refine the following response. Return only the improved response.\n\n{}",
                                text
                            ))],
                            system_prompt: Some(review_system.clone()),
                            tools: None,
                            model: config.model.clone(),
                            max_tokens: Some(2000),
                            temperature: Some(0.5),
                            stream: false,
                            tool_choice: Some(uni::ToolChoice::none()),
                            parallel_tool_calls: None,
                            parallel_tool_config: None,
                            reasoning_effort: vt_cfg.as_ref().and_then(|cfg| {
                                if provider_client.supports_reasoning_effort(&active_model) {
                                    Some(cfg.agent.reasoning_effort)
                                } else {
                                    None
                                }
                            }),
                        };
                        let rr = provider_client.generate(review_req).await.ok();
                        if let Some(r) = rr.and_then(|result| result.content)
                            && !r.trim().is_empty()
                        {
                            text = r;
                        }
                    }
                }
                let trimmed = text.trim();
                let suppress_response = trimmed.is_empty()
                    || last_tool_stdout
                        .as_ref()
                        .map(|stdout| stdout == trimmed)
                        .unwrap_or(false);

                let streamed_matches_output = response_streamed
                    && response
                        .content
                        .as_ref()
                        .map(|original| original == &text)
                        .unwrap_or(false);

                if !suppress_response && !streamed_matches_output {
                    renderer.line(MessageStyle::Response, &text)?;
                }
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(text));
                let _ = last_tool_stdout.take();
            } else {
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
            }
            break TurnLoopResult::Completed;
        };

        match turn_result {
            TurnLoopResult::Cancelled => {
                if ctrl_c_state.is_exit_requested() {
                    break;
                }

                renderer.line_if_not_empty(MessageStyle::Output)?;
                renderer.line(
                    MessageStyle::Info,
                    "Interrupted current task. Press Ctrl+C again to exit.",
                )?;
                handle.clear_input();
                handle.set_placeholder(default_placeholder.clone());
                ctrl_c_state.clear_cancel();
                continue;
            }
            TurnLoopResult::Aborted => {
                let _ = conversation_history.pop();
                continue;
            }
            TurnLoopResult::Completed => {
                conversation_history = working_history;

                let _pruned_after_turn =
                    context_manager.prune_tool_responses(&mut conversation_history);
                // Removed: Tool response pruning message after completion
                let post_trim = context_manager.enforce_context_window(&mut conversation_history);
                if post_trim.is_trimmed() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                            post_trim.removed_messages, trim_config.max_tokens,
                        ),
                    )?;
                }

                if let Some(last) = conversation_history.last()
                    && last.role == uni::MessageRole::Assistant
                {
                    let text = &last.content;
                    let claims_write = text.contains("I've updated")
                        || text.contains("I have updated")
                        || text.contains("updated the `");
                    if claims_write && !any_write_effect {
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                        renderer.line(
                            MessageStyle::Info,
                            "Note: The assistant mentioned edits but no write tool ran.",
                        )?;
                    }
                }

                if let Some(manager) = checkpoint_manager.as_ref() {
                    let conversation_snapshot: Vec<SessionMessage> = conversation_history
                        .iter()
                        .map(SessionMessage::from)
                        .collect();
                    let turn_number = next_checkpoint_turn;
                    let description = refined_user.trim();
                    match manager.create_snapshot(
                        turn_number,
                        description,
                        &conversation_snapshot,
                        &turn_modified_files,
                    ) {
                        Ok(Some(meta)) => {
                            next_checkpoint_turn = meta.turn_number.saturating_add(1);
                        }
                        Ok(None) => {}
                        Err(err) => {
                            warn!(
                                "Failed to create checkpoint for turn {}: {}",
                                turn_number, err
                            );
                        }
                    }
                }
            }
        }
    }

    let transcript_lines = transcript::snapshot();
    if let Some(archive) = session_archive.take() {
        let distinct_tools = session_stats.sorted_tools();
        let total_messages = conversation_history.len();
        let session_messages: Vec<SessionMessage> = conversation_history
            .iter()
            .map(SessionMessage::from)
            .collect();
        match archive.finalize(
            transcript_lines,
            total_messages,
            distinct_tools,
            session_messages,
        ) {
            Ok(path) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Session saved to {}", path.display()),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to save session: {}", err),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
        }
    }

    for linked in linked_directories {
        if let Err(err) = remove_directory_symlink(&linked.link_path) {
            eprintln!(
                "Warning: failed to remove linked directory {}: {}",
                linked.link_path.display(),
                err
            );
        }
    }

    // Shutdown MCP client properly before TUI shutdown
    if let Some(mcp_client) = &mcp_client {
        if let Err(e) = mcp_client.shutdown().await {
            let error_msg = e.to_string();
            if error_msg.contains("EPIPE")
                || error_msg.contains("Broken pipe")
                || error_msg.contains("write EPIPE")
            {
                eprintln!(
                    "Info: MCP client shutdown encountered pipe errors (normal): {}",
                    e
                );
            } else {
                eprintln!("Warning: Failed to shutdown MCP client cleanly: {}", e);
            }
        }
    }

    handle.shutdown();
    Ok(())
}

fn safe_force_redraw(handle: &InlineHandle, last_forced_redraw: &mut Instant) {
    // Rate limit force_redraw calls to prevent TUI corruption
    if last_forced_redraw.elapsed() > std::time::Duration::from_millis(100) {
        handle.force_redraw();
        *last_forced_redraw = Instant::now();
    }
}
