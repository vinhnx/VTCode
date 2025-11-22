use crate::agent::runloop::unified::turn::session::slash_commands::{
    SlashCommandContext, SlashCommandControl,
};
use anyhow::{Context, Result};
use chrono::Local;
use crossterm::terminal::disable_raw_mode;
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;

use crate::agent::runloop::unified::turn::session::slash_commands;
use vtcode_core::llm::provider::{self as uni};

use tracing::warn;
use vtcode_core::config::constants::{defaults, ui};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};
use vtcode_core::tools::ApprovalRecorder;

use crate::agent::runloop::unified::state::CtrlCState;
use vtcode_core::config::types::UiSurfacePreference;
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{InlineEvent, InlineEventCallback, spawn_session, theme_from_styles};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::at_pattern::parse_at_patterns;
use vtcode_core::utils::session_archive::{SessionArchive, SessionArchiveMetadata};
use vtcode_core::utils::transcript;

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::prompt::refine_user_prompt_if_enabled;
use crate::agent::runloop::slash_commands::handle_slash_command;
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};
use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager;

use super::finalization::finalize_session;
use super::utils::render_hook_messages;
use super::workspace::{load_workspace_files, refresh_vt_config};
use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;
use crate::agent::runloop::unified::context_manager::ContextManager;

use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
// loop_detection not used in session loop refactor
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::{ActivePalette, apply_prompt_style};
use crate::agent::runloop::unified::session_setup::{
    SessionState, build_mcp_tool_definitions, initialize_session,
};
use crate::agent::runloop::unified::state::{CtrlCSignal, SessionStats};
use crate::agent::runloop::unified::status_line::{
    InputStatusState, update_context_efficiency, update_input_status_if_changed,
};
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
use crate::ide_context::IdeContextBridge;

#[allow(dead_code)]
enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Blocked { reason: Option<String> },
}

#[allow(dead_code)]
const SELF_REVIEW_MIN_LENGTH: usize = 240;

pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    mut vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    resume: Option<ResumeSession>,
) -> Result<()> {
    // Set up panic handler to ensure terminal cleanup on panic
    // This is critical for handling abnormal termination (e.g., from panics)
    // without leaving ANSI escape sequences in the terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Attempt to restore terminal to clean state
        let _ = disable_raw_mode();
        let mut stdout = std::io::stdout();
        let _ = stdout.flush();

        // Call the original panic hook to maintain normal panic behavior
        original_hook(panic_info);
    }));

    // Create a guard that ensures terminal is restored even on early return
    // This is important because the TUI task may not shutdown cleanly
    let _terminal_cleanup_guard = TerminalCleanupGuard::new();

    // Note: The original hook will not be restored during this session
    // but Rust runtime should handle this appropriately
    let mut config = config.clone();
    let mut resume_state = resume;

    loop {
        let resume_ref = resume_state.as_ref();

        let session_trigger = if resume_ref.is_some() {
            SessionStartTrigger::Resume
        } else {
            SessionStartTrigger::Startup
        };
        let lifecycle_hooks = if let Some(vt) = vt_cfg.as_ref() {
            LifecycleHookEngine::new(config.workspace.clone(), &vt.hooks, session_trigger)?
        } else {
            None
        };

        let SessionState {
            session_bootstrap,
            mut provider_client,
            mut tool_registry,
            tools,
            trim_config,
            mut conversation_history,
            decision_ledger,
            pruning_ledger,
            trajectory: traj,
            base_system_prompt,
            full_auto_allowlist,
            async_mcp_manager,
            mut mcp_panel_state,
            token_budget,
            token_budget_enabled,
            token_counter,
            tool_result_cache,
            tool_permission_cache,
            search_metrics: _,
            custom_prompts,
        } = initialize_session(&config, vt_cfg.as_ref(), full_auto, resume_ref).await?;

        let mut session_end_reason = SessionEndReason::Completed;

        let mut context_manager = ContextManager::new(
            base_system_prompt,
            trim_config,
            token_budget.clone(),
            token_budget_enabled,
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

        // Set environment variable to indicate TUI mode is active
        // This prevents CLI dialoguer prompts from corrupting the TUI display
        // SAFETY: Setting a process-local environment variable is safe; the OS copies the value.
        unsafe {
            std::env::set_var("VTCODE_TUI_MODE", "1");
        }

        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let interrupt_callback: InlineEventCallback = {
            let state = ctrl_c_state.clone();
            let notify = ctrl_c_notify.clone();
            Arc::new(move |event: &InlineEvent| {
                if matches!(event, InlineEvent::Interrupt) {
                    let _ = state.register_signal();
                    notify.notify_waiters();
                }
            })
        };

        let mut session = spawn_session(
            theme_spec.clone(),
            default_placeholder.clone(),
            config.ui_surface,
            inline_rows,
            show_timeline_pane,
            Some(interrupt_callback),
        )
        .context("failed to launch inline session")?;
        let handle = session.clone_inline_handle();
        let highlight_config = vt_cfg
            .as_ref()
            .map(|cfg| cfg.syntax_highlighting.clone())
            .unwrap_or_default();

        // Set the inline handle for the message queue system
        transcript::set_inline_handle(Arc::new(handle.clone()));

        let mut ide_context_bridge = IdeContextBridge::from_env();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);

        let workspace_for_indexer = config.workspace.clone();
        let workspace_for_palette = config.workspace.clone();
        let handle_for_indexer = handle.clone();
        // Spawn background task for file palette loading. See: https://ratatui.rs/faq/
        let _file_palette_task = tokio::spawn(async move {
            match load_workspace_files(workspace_for_indexer).await {
                Ok(files) => {
                    if !files.is_empty() {
                        handle_for_indexer.load_file_palette(files, workspace_for_palette);
                    } else {
                        tracing::debug!("No files found in workspace for file palette");
                    }
                }
                Err(err) => {
                    tracing::warn!("Failed to load workspace files for file palette: {}", err);
                }
            }
        });

        transcript::clear();

        if let Some(session) = resume_state.as_ref() {
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
            provider_client.name().to_string()
        } else {
            config.provider.clone()
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
        let mut session_archive = match SessionArchive::new(archive_metadata).await {
            Ok(archive) => Some(archive),
            Err(err) => {
                session_archive_error = Some(err.to_string());
                None
            }
        };

        if let (Some(hooks), Some(archive)) = (&lifecycle_hooks, session_archive.as_ref()) {
            hooks
                .update_transcript_path(Some(archive.path().to_path_buf()))
                .await;
        }

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

        // Render the session banner, now enriched with Git branch and status information.
        render_session_banner(
            &mut renderer,
            &config,
            &session_bootstrap,
            &config.model,
            &reasoning_label,
        )?;

        if let Some(bridge) = ide_context_bridge.as_mut() {
            match bridge.snapshot() {
                Ok(Some(context)) => {
                    conversation_history.push(uni::Message::system(context));
                }
                Ok(None) => {}
                Err(err) => {
                    warn!("Failed to update IDE context snapshot: {}", err);
                }
            }
        }

        if let Some(hooks) = &lifecycle_hooks {
            match hooks.run_session_start().await {
                Ok(outcome) => {
                    render_hook_messages(&mut renderer, &outcome.messages)?;
                    for context in outcome.additional_context {
                        if !context.trim().is_empty() {
                            conversation_history.push(uni::Message::system(context));
                        }
                    }
                }
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to run session start hooks: {}", err),
                    )?;
                }
            }
        }
        let mode_label = match (config.ui_surface, full_auto) {
            (UiSurfacePreference::Inline, true) => "auto".to_string(),
            (UiSurfacePreference::Inline, false) => "inline".to_string(),
            (UiSurfacePreference::Alternate, _) => "alt".to_string(),
            (UiSurfacePreference::Auto, true) => "auto".to_string(),
            (UiSurfacePreference::Auto, false) => "std".to_string(),
        };
        let header_context = build_inline_header_context(
            &config,
            &session_bootstrap,
            header_provider_label,
            config.model.clone(),
            mode_label,
            reasoning_label.clone(),
        )
        .await?;
        handle.set_header_context(header_context);
        // MCP events are now rendered as message blocks in the conversation history

        if let Some(message) = session_archive_error.take() {
            renderer.line(
                MessageStyle::Info,
                &format!("Session archiving disabled: {}", message),
            )?;
            renderer.line_if_not_empty(MessageStyle::Output)?;
        }

        if full_auto && let Some(allowlist) = full_auto_allowlist.as_ref() {
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

        let async_mcp_manager_for_signal = async_mcp_manager.clone();
        {
            let state = ctrl_c_state.clone();
            let notify = ctrl_c_notify.clone();
            // Spawn Ctrl+C signal handler (background task)
            // See: https://ratatui.rs/faq/#when-should-i-use-tokio-and-async--await-
            let _signal_handler = tokio::spawn(async move {
                loop {
                    if tokio::signal::ctrl_c().await.is_err() {
                        break;
                    }

                    let signal = state.register_signal();
                    notify.notify_waiters();

                    // Shutdown MCP client on interrupt using async manager
                    if let Some(mcp_manager) = &async_mcp_manager_for_signal
                        && let Err(e) = mcp_manager.shutdown().await
                    {
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

                    if matches!(signal, CtrlCSignal::Exit) {
                        break;
                    }
                }
            });
        }

        let mut session_stats = SessionStats::default();
        let cache_dir = std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".vtcode").join("cache"))
            .unwrap_or_else(|| PathBuf::from(".vtcode/cache"));
        let approval_recorder = Arc::new(ApprovalRecorder::new(cache_dir));
        let mut linked_directories: Vec<LinkedDirectory> = Vec::new();
        let mut model_picker_state: Option<ModelPickerState> = None;
        let mut palette_state: Option<ActivePalette> = None;
        let mut last_forced_redraw = Instant::now();
        let mut input_status_state = InputStatusState::default();
        let mut queued_inputs: VecDeque<String> = VecDeque::new();
        let mut ctrl_c_notice_displayed = false;
        let mut mcp_catalog_initialized = tool_registry.mcp_client().is_some();
        let mut last_known_mcp_tools: Vec<String> = Vec::new();
        let mut last_mcp_refresh = std::time::Instant::now();
        const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

        // Report MCP initialization status if available and there's an error
        if let Some(mcp_manager) = &async_mcp_manager {
            let mcp_status = mcp_manager.get_status().await;
            if mcp_status.is_error() {
                if let Some(error_msg) = mcp_status.get_error_message() {
                    renderer.line(MessageStyle::Error, &format!("MCP Error: {}", error_msg))?;
                    renderer.line(
                        MessageStyle::Info,
                        "Use /mcp to check status or update your vtcode.toml configuration.",
                    )?;
                }
            } else if mcp_status.is_initializing() {
                renderer.line(
                    MessageStyle::Info,
                    "MCP is still initializing in the background...",
                )?;
            }
        }

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

            // Update context efficiency metrics in status line
            if let Some(efficiency) = context_manager.last_efficiency() {
                update_context_efficiency(
                    &mut input_status_state,
                    efficiency.context_utilization_percent,
                    efficiency.total_tokens,
                    efficiency.semantic_value_per_token,
                );
            }

            if ctrl_c_state.is_exit_requested() {
                session_end_reason = SessionEndReason::Exit;
                break;
            }

            let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());

            if let Some(mcp_manager) = &async_mcp_manager {
                // Handle initial MCP client setup
                if !mcp_catalog_initialized {
                    match mcp_manager.get_status().await {
                        McpInitStatus::Ready { client } => {
                            tool_registry.set_mcp_client(Arc::clone(&client));
                            match tool_registry.refresh_mcp_tools().await {
                                Ok(()) => {
                                    let mut registered_tools = 0usize;
                                    match tool_registry.list_mcp_tools().await {
                                        Ok(mcp_tools) => {
                                            let new_definitions =
                                                build_mcp_tool_definitions(&mcp_tools);
                                            registered_tools = new_definitions.len();
                                            let _updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool
                                                        .function
                                                        .as_ref()
                                                        .unwrap()
                                                        .name
                                                        .starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };

                                            // Enumerate MCP tools after initial setup (with detailed tool discovery messages)
                                            McpToolManager::enumerate_mcp_tools_after_initial_setup(
                                                &mut tool_registry,
                                                &tools,
                                                mcp_tools,
                                                &mut last_known_mcp_tools,
                                                &mut renderer,
                                            ).await?;
                                        }
                                        Err(err) => {
                                            warn!(
                                                "Failed to enumerate MCP tools after refresh: {err}"
                                            );
                                        }
                                    }

                                    renderer.line(
                                            MessageStyle::Info,
                                            &format!(
                                                "MCP tools ready ({} registered). Use /mcp tools to inspect the catalog.",
                                                registered_tools
                                            ),
                                        )?;
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                }
                                Err(err) => {
                                    warn!(
                                        "Failed to refresh MCP tools after initialization: {err}"
                                    );
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to index MCP tools: {}", err),
                                    )?;
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                }
                            }
                            mcp_catalog_initialized = true;
                        }
                        McpInitStatus::Error { message } => {
                            renderer
                                .line(MessageStyle::Error, &format!("MCP Error: {}", message))?;
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            mcp_catalog_initialized = true;
                        }
                        McpInitStatus::Initializing { .. } | McpInitStatus::Disabled => {}
                    }
                }

                // Dynamic MCP tool refresh - check for new/updated tools after initialization
                if mcp_catalog_initialized && last_mcp_refresh.elapsed() >= MCP_REFRESH_INTERVAL {
                    last_mcp_refresh = std::time::Instant::now();

                    if let Ok(known_tools) = tool_registry.list_mcp_tools().await {
                        let current_tool_keys: Vec<String> = known_tools
                            .iter()
                            .map(|t| format!("{}-{}", t.provider, t.name))
                            .collect();

                        // Check if there are new or changed tools
                        if current_tool_keys != last_known_mcp_tools {
                            match tool_registry.refresh_mcp_tools().await {
                                Ok(()) => {
                                    match tool_registry.list_mcp_tools().await {
                                        Ok(new_mcp_tools) => {
                                            let new_definitions =
                                                build_mcp_tool_definitions(&new_mcp_tools);
                                            let _updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool
                                                        .function
                                                        .as_ref()
                                                        .unwrap()
                                                        .name
                                                        .starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };

                                            // Enumerate MCP tools after refresh (with detailed tool discovery messages)
                                            McpToolManager::enumerate_mcp_tools_after_refresh(
                                                &mut tool_registry,
                                                &tools,
                                                &mut last_known_mcp_tools,
                                                &mut renderer,
                                            )
                                            .await?;
                                        }
                                        Err(err) => {
                                            warn!(
                                                "Failed to enumerate MCP tools after refresh: {err}"
                                            );
                                        }
                                    }
                                }
                                Err(err) => {
                                    warn!(
                                        "Failed to refresh MCP tools during dynamic update: {err}"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            let resources = InlineEventLoopResources {
                renderer: &mut renderer,
                handle: &handle,
                interrupts,
                ctrl_c_notice_displayed: &mut ctrl_c_notice_displayed,
                default_placeholder: &default_placeholder,
                queued_inputs: &mut queued_inputs,
                model_picker_state: &mut model_picker_state,
                palette_state: &mut palette_state,
                config: &mut config,
                vt_cfg: &mut vt_cfg,
                provider_client: &mut provider_client,
                session_bootstrap: &session_bootstrap,
                full_auto,
            };

            let mut input_owned =
                match poll_inline_loop_action(&mut session, &ctrl_c_notify, resources).await? {
                    InlineLoopAction::Continue => continue,
                    InlineLoopAction::Submit(text) => text,
                    InlineLoopAction::Exit(reason) => {
                        session_end_reason = reason;
                        break;
                    }
                };

            if input_owned.is_empty() {
                continue;
            }

            if let Err(err) = refresh_vt_config(&config.workspace, &config, &mut vt_cfg).await {
                warn!(
                    workspace = %config.workspace.display(),
                    error = ?err,
                    "Failed to refresh workspace configuration"
                );
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to reload configuration: {err}"),
                )?;
            }

            // Check for MCP status changes and report errors
            if let Some(mcp_manager) = &async_mcp_manager {
                let mcp_status = mcp_manager.get_status().await;
                if mcp_status.is_error()
                    && let Some(error_msg) = mcp_status.get_error_message()
                {
                    renderer.line(MessageStyle::Error, &format!("MCP Error: {}", error_msg))?;
                    renderer.line(
                        MessageStyle::Info,
                        "Use /mcp to check status or update your vtcode.toml configuration.",
                    )?;
                }
            }

            if let Some(next_placeholder) = follow_up_placeholder.take() {
                handle.set_placeholder(Some(next_placeholder.clone()));
                default_placeholder = Some(next_placeholder);
            }

            match input_owned.as_str() {
                "" => continue,
                "exit" | "quit" => {
                    renderer.line(MessageStyle::Info, "Goodbye!")?;
                    session_end_reason = SessionEndReason::Exit;
                    break;
                }
                "help" => {
                    renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                    continue;
                }
                input if input.starts_with('/') => {
                    // Handle slash commands
                    if let Some(command_input) = input.strip_prefix('/') {
                        let outcome =
                            handle_slash_command(command_input, &mut renderer, &custom_prompts)
                                .await?;
                        let command_result = slash_commands::handle_outcome(
                            outcome,
                            SlashCommandContext {
                                renderer: &mut renderer,
                                handle: &handle,
                                session: &mut session,
                                config: &mut config,
                                vt_cfg: &mut vt_cfg,
                                provider_client: &mut provider_client,
                                session_bootstrap: &session_bootstrap,
                                model_picker_state: &mut model_picker_state,
                                palette_state: &mut palette_state,
                                tool_registry: &mut tool_registry,
                                conversation_history: &mut conversation_history,
                                decision_ledger: &decision_ledger,
                                pruning_ledger: &pruning_ledger,
                                context_manager: &mut context_manager,
                                session_stats: &mut session_stats,
                                tools: &tools,
                                token_budget_enabled,
                                trim_config: &trim_config,
                                async_mcp_manager: async_mcp_manager.as_ref(),
                                mcp_panel_state: &mut mcp_panel_state,
                                linked_directories: &mut linked_directories,
                                ctrl_c_state: &ctrl_c_state,
                                ctrl_c_notify: &ctrl_c_notify,
                                default_placeholder: &default_placeholder,
                                lifecycle_hooks: lifecycle_hooks.as_ref(),
                                full_auto,
                                approval_recorder: Some(&approval_recorder),
                                tool_permission_cache: &tool_permission_cache,
                            },
                        )
                        .await?;
                        match command_result {
                            SlashCommandControl::SubmitPrompt(prompt) => {
                                input_owned = prompt;
                            }
                            SlashCommandControl::Continue => continue,
                            SlashCommandControl::BreakWithReason(reason) => {
                                session_end_reason = reason;
                                break;
                            }
                            SlashCommandControl::BreakWithoutReason => break,
                        }
                    }
                }
                _ => {}
            }

            if let Some(hooks) = &lifecycle_hooks {
                match hooks.run_user_prompt_submit(input_owned.as_str()).await {
                    Ok(outcome) => {
                        render_hook_messages(&mut renderer, &outcome.messages)?;
                        if !outcome.allow_prompt {
                            handle.clear_input();
                            continue;
                        }
                        for context in outcome.additional_context {
                            if !context.trim().is_empty() {
                                conversation_history.push(uni::Message::system(context));
                            }
                        }
                    }
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to run prompt hooks: {}", err),
                        )?;
                    }
                }
            }

            if let Some(picker) = model_picker_state.as_mut() {
                let progress = picker.handle_input(&mut renderer, input_owned.as_str())?;
                match progress {
                    ModelPickerProgress::InProgress => continue,
                    ModelPickerProgress::NeedsRefresh => {
                        picker.refresh_dynamic_models(&mut renderer).await?;
                        continue;
                    }
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
                        )
                        .await
                        {
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

            // Process @ patterns to embed images as base64 content
            let processed_content = match parse_at_patterns(input, &config.workspace).await {
                Ok(content) => content,
                Err(e) => {
                    // Log the error but continue with original input as text
                    tracing::warn!("Failed to parse @ patterns: {}", e);
                    uni::MessageContent::text(input.to_string())
                }
            };

            // Apply prompt refinement if enabled
            let refined_content = match &processed_content {
                uni::MessageContent::Text(text) => {
                    let refined_text =
                        refine_user_prompt_if_enabled(text, &config, vt_cfg.as_ref()).await;
                    uni::MessageContent::text(refined_text)
                }
                uni::MessageContent::Parts(parts) => {
                    let mut refined_parts = Vec::new();
                    for part in parts {
                        match part {
                            uni::ContentPart::Text { text } => {
                                let refined_text =
                                    refine_user_prompt_if_enabled(text, &config, vt_cfg.as_ref())
                                        .await;
                                refined_parts.push(uni::ContentPart::text(refined_text));
                            }
                            _ => refined_parts.push(part.clone()),
                        }
                    }
                    uni::MessageContent::parts(refined_parts)
                }
            };

            // Display the user message with inline border decoration
            match &refined_content {
                uni::MessageContent::Text(text) => display_user_message(&mut renderer, text)?,
                uni::MessageContent::Parts(parts) => {
                    // For multi-part content, display the text parts concatenated
                    let mut display_parts = Vec::new();
                    for part in parts {
                        if let uni::ContentPart::Text { text } = part {
                            display_parts.push(text.as_str());
                        }
                    }
                    let display_text = display_parts.join(" ");
                    display_user_message(&mut renderer, &display_text)?;
                }
            }

            // Spinner is displayed via the input status in the inline handle
            // No need to show a separate message here

            // Create user message with processed content using the appropriate constructor
            let user_message = match refined_content {
                uni::MessageContent::Text(text) => uni::Message::user(text),
                uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
            };

            conversation_history.push(user_message);
            // Removed: Tool response pruning
            // Removed: Context window enforcement to respect token limits

            let working_history = conversation_history.clone();
            let _max_tool_loops = vt_cfg
                .as_ref()
                .map(|cfg| cfg.tools.max_tool_loops)
                .filter(|&value| value > 0)
                .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);

            // Unused turn-level locals removed after refactor
            let _tool_repeat_limit = vt_cfg
                .as_ref()
                .map(|cfg| cfg.tools.max_repeated_tool_calls)
                .filter(|&value| value > 0)
                .unwrap_or(defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS);
            // repeated tool attempts now managed in the turn loop; omitted here

            // Initialize loop detection
            let _loop_detection_enabled = vt_cfg
                .as_ref()
                .map(|cfg| !cfg.model.skip_loop_detection)
                .unwrap_or(true);
            let _loop_detection_threshold = vt_cfg
                .as_ref()
                .map(|cfg| cfg.model.loop_detection_threshold)
                .unwrap_or(3);
            let _loop_detection_interactive = vt_cfg
                .as_ref()
                .map(|cfg| cfg.model.loop_detection_interactive)
                .unwrap_or(true);
            // loop detection instance not used in the session loop path
            let mut _loop_detection_disabled_for_session = false;

            // New unified turn loop: use TurnLoopContext and run_turn_loop
            let turn_loop_ctx = crate::agent::runloop::unified::turn::TurnLoopContext {
                renderer: &mut renderer,
                handle: &handle,
                session: &mut session,
                session_stats: &mut session_stats,
                mcp_panel_state: &mut mcp_panel_state,
                tool_result_cache: &tool_result_cache,
                approval_recorder: &approval_recorder,
                decision_ledger: &decision_ledger,
                pruning_ledger: &pruning_ledger,
                token_budget: &token_budget,
                token_counter: &token_counter,
                tool_registry: &mut tool_registry,
                tools: &tools,
                ctrl_c_state: &ctrl_c_state,
                ctrl_c_notify: &ctrl_c_notify,
                context_manager: &mut context_manager,
                last_forced_redraw: &mut last_forced_redraw,
                input_status_state: &mut input_status_state,
                lifecycle_hooks: lifecycle_hooks.as_ref(),
                default_placeholder: &default_placeholder,
                tool_permission_cache: &tool_permission_cache,
            };
            let outcome = crate::agent::runloop::unified::turn::run_turn_loop(
                input,
                working_history.clone(),
                turn_loop_ctx,
                &config,
                vt_cfg.as_ref(),
                &mut provider_client,
                &traj,
                skip_confirmations,
                &mut session_end_reason,
            )
            .await?;
            // Apply canonical side-effects for the turn outcome (history, checkpoints, session end reason)
            crate::agent::runloop::unified::turn::apply_turn_outcome(
                &outcome,
                &mut conversation_history,
                &mut renderer,
                &handle,
                &ctrl_c_state,
                &default_placeholder,
                checkpoint_manager.as_ref(),
                &mut next_checkpoint_turn,
                &mut session_stats,
                &mut session_end_reason,
                &pruning_ledger,
            )
            .await?;
            let _turn_result = outcome.result;

            // Check for session exit and continue to next iteration otherwise.
            if matches!(session_end_reason, SessionEndReason::Exit) {
                break;
            }
            continue;
        }

        finalize_session(
            &mut renderer,
            lifecycle_hooks.as_ref(),
            session_end_reason,
            &mut session_archive,
            &session_stats,
            &conversation_history,
            linked_directories,
            async_mcp_manager.as_deref(),
            &handle,
            Some(&pruning_ledger),
        )
        .await?;

        // If the session ended with NewSession, restart the loop with fresh config
        if matches!(session_end_reason, SessionEndReason::NewSession) {
            // Reload config to pick up any changes
            vt_cfg =
                vtcode_core::config::loader::ConfigManager::load_from_workspace(&config.workspace)
                    .ok()
                    .map(|manager| manager.config().clone());
            resume_state = None;
            continue;
        }

        break;
    }

    Ok(())
}

/// Guard that ensures terminal is restored to a clean state when dropped
/// This handles cases where the TUI doesn't shutdown cleanly or the session
/// exits early (e.g., due to Ctrl+C or other signals)
struct TerminalCleanupGuard;

impl TerminalCleanupGuard {
    fn new() -> Self {
        Self
    }
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        // Minimal terminal cleanup as last resort
        // The TUI's run_inline_tui should handle full cleanup, this is just a safety net
        // We deliberately avoid sending escape sequences to prevent conflicts with TUI cleanup

        // Attempt to disable raw mode if still enabled
        let _ = disable_raw_mode();

        // Ensure stdout is flushed
        let mut stdout = std::io::stdout();
        let _ = stdout.flush();

        // Wait for terminal to finish processing any pending operations
        // This prevents incomplete writes from corrupting the terminal
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
