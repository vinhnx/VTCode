mod slash_commands;

use anyhow::{Context, Result};
use chrono::Local;
use slash_commands::{SlashCommandContext, SlashCommandControl};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::task;

#[cfg(debug_assertions)]
use tracing::debug;
use tracing::warn;
use vtcode_core::config::constants::{defaults, ui};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};
use vtcode_core::core::decision_tracker::{Action as DTAction, DecisionOutcome};
use vtcode_core::core::router::{Router, TaskClass};
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, classify_error};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{InlineEvent, InlineEventCallback, spawn_session, theme_from_styles};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::at_pattern::parse_at_patterns;
use vtcode_core::utils::session_archive::{SessionArchive, SessionArchiveMetadata, SessionMessage};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};
use vtcode_core::utils::transcript;

use crate::agent::agents::is_context_overflow_error;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::prompt::refine_user_prompt_if_enabled;
use crate::agent::runloop::slash_commands::handle_slash_command;
use crate::agent::runloop::text_tools::{detect_textual_tool_call, extract_code_fence_blocks};
use crate::agent::runloop::tool_output::render_code_fence_blocks;
use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};
use crate::agent::runloop::unified::ui_interaction::{
    PlaceholderSpinner, stream_and_render_response,
};

use super::finalization::finalize_session;
use super::harmony::strip_harmony_syntax;
use super::utils::{render_hook_messages, safe_force_redraw};
use super::workspace::{load_workspace_files, refresh_vt_config};
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::curator::{
    build_curator_tools, format_provider_label, resolve_mode_label,
};
use crate::agent::runloop::unified::display::{display_user_message, ensure_turn_bottom_gap};
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::{ActivePalette, apply_prompt_style};
use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::session_setup::{
    SessionState, build_mcp_tool_definitions, initialize_session,
};
use crate::agent::runloop::unified::shell::{
    derive_recent_tool_output, should_short_circuit_shell,
};
use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState, SessionStats};
use crate::agent::runloop::unified::status_line::{
    InputStatusState, update_input_status_if_changed,
};
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, execute_tool_with_timeout,
};
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::agent::runloop::unified::tool_summary::{
    describe_tool_action, humanize_tool_name, render_tool_call_summary_with_status,
};
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
use crate::ide_context::IdeContextBridge;

enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Blocked {
        #[allow(dead_code)]
        reason: Option<String>,
    },
}

const SELF_REVIEW_MIN_LENGTH: usize = 240;

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
            trajectory: traj,
            base_system_prompt,
            full_auto_allowlist,
            #[allow(unused_variables)]
            async_mcp_manager,
            mut mcp_panel_state,
            token_budget,
            token_budget_enabled,
            curator,
            custom_prompts,
            mut sandbox,
        } = initialize_session(&config, vt_cfg.as_ref(), full_auto, resume_ref).await?;

        let mut session_end_reason = SessionEndReason::Completed;

        let initial_tool_snapshot = tools.read().await.clone();
        let curator_tool_catalog = build_curator_tools(&initial_tool_snapshot);
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

        // Set environment variable to indicate TUI mode is active
        // This prevents CLI dialoguer prompts from corrupting the TUI display
        // SAFETY: We're setting this at the start of the TUI session and it's only read
        // by the tool policy manager to detect TUI mode. No other threads are modifying
        // this variable concurrently.
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
        tokio::spawn(async move {
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
        let mode_label = resolve_mode_label(config.ui_surface, full_auto);
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
            tokio::spawn(async move {
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
                                            let updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool.function.name.starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };
                                            context_manager.update_tool_catalog(
                                                build_curator_tools(&updated_snapshot),
                                            );

                                            // Store the initial tool names to track changes later
                                            last_known_mcp_tools = mcp_tools
                                                .iter()
                                                .map(|t| format!("{}-{}", t.provider, t.name))
                                                .collect();
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
                            renderer.line(
                                MessageStyle::Error,
                                &format!("⚠️  MCP Error: {}", message),
                            )?;
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
                                            let updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool.function.name.starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };
                                            context_manager.update_tool_catalog(
                                                build_curator_tools(&updated_snapshot),
                                            );

                                            let added_count = new_mcp_tools
                                                .len()
                                                .saturating_sub(last_known_mcp_tools.len());
                                            let message = if added_count > 0 {
                                                format!(
                                                    "Discovered {} new MCP tool{}",
                                                    added_count,
                                                    if added_count == 1 { "" } else { "s" }
                                                )
                                            } else {
                                                "MCP tools updated".to_string()
                                            };

                                            renderer.line(
                                                MessageStyle::Info,
                                                &format!("{}", message),
                                            )?;
                                            renderer.line_if_not_empty(MessageStyle::Output)?;

                                            // Update the last known tools
                                            last_known_mcp_tools = new_mcp_tools
                                                .iter()
                                                .map(|t| format!("{}-{}", t.provider, t.name))
                                                .collect();
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
                                sandbox: &mut sandbox,
                                tool_registry: &mut tool_registry,
                                conversation_history: &mut conversation_history,
                                decision_ledger: &decision_ledger,
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

            // Create user message with processed content using the appropriate constructor
            let user_message = match refined_content {
                uni::MessageContent::Text(text) => uni::Message::user(text),
                uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
            };

            conversation_history.push(user_message);
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

            let mut step_count = 0usize;
            let mut allow_follow_up = true;
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
                    renderer.line_if_not_empty(MessageStyle::Output)?;
                    renderer.line(MessageStyle::Info, "Cancelling current operation...")?;
                    break TurnLoopResult::Cancelled;
                }
                if step_count > 0 && !allow_follow_up {
                    ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                    break TurnLoopResult::Completed;
                }
                if step_count == 0 {
                    renderer.line_if_not_empty(MessageStyle::Output)?;
                }
                step_count += 1;
                allow_follow_up = false;
                if step_count > max_tool_loops {
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
                            .map(|message| message.content.as_text()),
                    );
                    let tool_names: Vec<String> = {
                        let snapshot = tools.read().await;
                        snapshot
                            .iter()
                            .map(|tool| tool.function.name.clone())
                            .collect()
                    };
                    ledger.update_available_tools(tool_names);
                }

                // Automatic summarization: prevent context overflow and blocking
                // Check if the feature is enabled in config (disabled by default)
                let optimization_enabled = vt_cfg
                    .as_ref()
                    .map(|cfg| {
                        cfg.agent
                            .smart_summarization
                            .show_context_optimization_message
                    })
                    .unwrap_or(false);

                let conversation_len = working_history.len();
                let should_compress = optimization_enabled
                    && (if token_budget_enabled {
                        let budget = context_manager.token_budget();
                        let usage_percent = budget.usage_percentage().await;
                        conversation_len >= 20 || usage_percent >= 85.0
                    } else {
                        conversation_len >= 20
                    });

                if should_compress && working_history.len() > 15 {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Optimizing context ({} messages -> 15 recent)",
                            conversation_len
                        ),
                    )?;

                    // Keep system message + recent 15 messages
                    let mut compressed = Vec::new();
                    if let Some(first) = working_history.first()
                        && matches!(first.role, uni::MessageRole::System)
                    {
                        compressed.push(first.clone());
                    }
                    compressed.extend(working_history.iter().rev().take(15).rev().cloned());
                    working_history = compressed;
                }

                let mut request_history = working_history.clone();
                let _ = context_manager.enforce_context_window(&mut request_history);
                context_manager.reset_token_budget().await;
                let system_prompt = context_manager
                    .build_system_prompt(&request_history, step_count)
                    .await?;

                let use_streaming = provider_client.supports_streaming();
                let reasoning_effort = vt_cfg.as_ref().and_then(|cfg| {
                    if provider_client.supports_reasoning_effort(&active_model) {
                        Some(cfg.agent.reasoning_effort)
                    } else {
                        None
                    }
                });
                let current_tools = tools.read().await.clone();
                let request = uni::LLMRequest {
                    messages: request_history.clone(),
                    system_prompt: Some(system_prompt),
                    tools: Some(current_tools),
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

                let thinking_spinner = PlaceholderSpinner::new(
                    &handle,
                    input_status_state.left.clone(),
                    input_status_state.right.clone(),
                    "Thinking...",
                );
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
                        step = step_count,
                        messages = request.messages.len(),
                        tools = tool_count,
                        "Dispatching provider request"
                    );
                }
                let llm_result = if use_streaming {
                    stream_and_render_response(
                        provider_client.as_ref(),
                        request,
                        &thinking_spinner,
                        &mut renderer,
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    )
                    .await
                } else {
                    let provider_name = provider_client.name().to_string();

                    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
                        thinking_spinner.finish();
                        Err(uni::LLMError::Provider(error_display::format_llm_error(
                            &provider_name,
                            "Interrupted by user",
                        )))
                    } else {
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
                    }
                };

                #[cfg(debug_assertions)]
                {
                    debug!(
                        target = "vtcode::agent::llm",
                        model = %active_model,
                        streaming = use_streaming,
                        step = step_count,
                        elapsed_ms = request_timer.elapsed().as_millis(),
                        succeeded = llm_result.is_ok(),
                        "Provider request finished"
                    );
                }

                let (response, response_streamed) = match llm_result {
                    Ok(payload) => {
                        if ctrl_c_state.is_cancel_requested() {
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            renderer.line(MessageStyle::Info, "Operation cancelled by user.")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        working_history = request_history;
                        payload
                    }
                    Err(error) => {
                        if ctrl_c_state.is_cancel_requested() {
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            renderer.line(MessageStyle::Info, "Operation cancelled by user.")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }

                        let error_text = error.to_string();
                        if is_context_overflow_error(&error_text) {
                            let removed_tool_messages =
                                context_manager.prune_tool_responses(&mut working_history);
                            let removed_turns =
                                context_manager.aggressive_trim(&mut working_history);
                            if removed_tool_messages + removed_turns > 0 {
                                renderer.line(
                                    MessageStyle::Info,
                                    "Context overflow detected; trimmed earlier messages and will retry.",
                                )?;
                                allow_follow_up = true;
                                continue 'outer;
                            }
                        }

                        let has_recent_tool = working_history
                            .iter()
                            .rev()
                            .take_while(|msg| msg.role != uni::MessageRole::Assistant)
                            .any(|msg| msg.role == uni::MessageRole::Tool);

                        if has_recent_tool {
                            let reply = derive_recent_tool_output(&working_history)
                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                            renderer.line(MessageStyle::Response, &reply)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            working_history.push(uni::Message::assistant(reply));
                            let _ = last_tool_stdout.take();
                            break 'outer TurnLoopResult::Completed;
                        }

                        renderer.line(
                            MessageStyle::Error,
                            &format!("Provider error: {error_text}"),
                        )?;
                        ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                        break 'outer TurnLoopResult::Aborted;
                    }
                };

                let mut final_text = response.content.clone();
                let mut tool_calls = response.tool_calls.unwrap_or_default();
                let mut interpreted_textual_call = false;
                let assistant_reasoning = response.reasoning.clone();

                // Strip harmony syntax from displayed content if present
                if let Some(ref text) = final_text
                    && (text.contains("<|start|>")
                        || text.contains("<|channel|>")
                        || text.contains("<|call|>"))
                {
                    // Remove harmony tool call syntax from the displayed text
                    let cleaned = strip_harmony_syntax(text);
                    if !cleaned.trim().is_empty() {
                        final_text = Some(cleaned);
                    } else {
                        final_text = None;
                    }
                }

                if tool_calls.is_empty() {
                    if let Some(text) = final_text.clone() {
                        if !text.trim().is_empty()
                            && let Some((name, args)) = detect_textual_tool_call(&text)
                        {
                            let args_json =
                                serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
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
                    }
                }

                if !tool_calls.is_empty() {
                    let assistant_text = if interpreted_textual_call {
                        String::new()
                    } else {
                        final_text.clone().unwrap_or_default()
                    };
                    let message =
                        uni::Message::assistant_with_tools(assistant_text, tool_calls.clone())
                            .with_reasoning(assistant_reasoning.clone());
                    working_history.push(message);
                    // Clear final_text since it was used for assistant_text
                    // This prevents the loop from breaking after tool execution
                    let _ = final_text.take();
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
                        if let Some(tool_name) = name.strip_prefix("mcp_") {
                            // Remove "mcp_" prefix
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
                        }
                        // Note: tool summary will be rendered after execution with status
                        let dec_id = {
                            let mut ledger = decision_ledger.write().await;
                            ledger.record_decision(
                                format!("Execute tool '{}' to progress task", name),
                                DTAction::ToolCall {
                                    name: name.to_string(),
                                    args: args_val.clone(),
                                    expected_outcome: "Use tool output to decide next step"
                                        .to_string(),
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
                            &mut session,
                            default_placeholder.clone(),
                            &ctrl_c_state,
                            &ctrl_c_notify,
                            lifecycle_hooks.as_ref(),
                        )
                        .await
                        {
                            Ok(ToolPermissionFlow::Approved) => {
                                // Force redraw immediately after modal closes to clear artifacts
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                // Longer delay to ensure modal is fully cleared before spinner starts
                                tokio::time::sleep(Duration::from_millis(100)).await;

                                if ctrl_c_state.is_cancel_requested() {
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                    renderer.line(
                                        MessageStyle::Info,
                                        "Tool execution cancelled by user.",
                                    )?;
                                    break 'outer TurnLoopResult::Cancelled;
                                }

                                // Create a progress reporter for the tool execution
                                let progress_reporter = ProgressReporter::new();

                                // Set initial progress and total
                                progress_reporter.set_total(100).await;
                                progress_reporter.set_progress(0).await;
                                progress_reporter
                                    .set_message(format!("Starting {}...", name))
                                    .await;

                                let tool_spinner = PlaceholderSpinner::with_progress(
                                    &handle,
                                    input_status_state.left.clone(),
                                    input_status_state.right.clone(),
                                    format!("Running tool: {}", name),
                                    Some(&progress_reporter),
                                );

                                // Force TUI refresh to ensure display stability
                                safe_force_redraw(&handle, &mut last_forced_redraw);

                                let pipeline_outcome = execute_tool_with_timeout(
                                    &mut tool_registry,
                                    name,
                                    args_val.clone(),
                                    &ctrl_c_state,
                                    &ctrl_c_notify,
                                    Some(&progress_reporter),
                                )
                                .await;

                                match pipeline_outcome {
                                    ToolExecutionStatus::Progress(progress) => {
                                        // Handle progress updates
                                        progress_reporter.set_message(progress.message).await;
                                        // Progress is already a u8 between 0-100
                                        if progress.progress <= 100 {
                                            progress_reporter
                                                .set_progress(progress.progress as u64)
                                                .await;
                                        }
                                        continue;
                                    }
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

                                        // Handle MCP events
                                        if let Some(tool_name) = name.strip_prefix("mcp_") {
                                            let mut mcp_event = mcp_events::McpEvent::new(
                                                "mcp".to_string(),
                                                tool_name.to_string(),
                                                Some(args_val.to_string()),
                                            );
                                            mcp_event.success(None);
                                            mcp_panel_state.add_event(mcp_event);
                                        } else {
                                            // Render tool summary with status
                                            let exit_code =
                                                output.get("exit_code").and_then(|v| v.as_i64());
                                            let status_icon =
                                                if command_success { "✓" } else { "✗" };
                                            render_tool_call_summary_with_status(
                                                &mut renderer,
                                                name,
                                                &args_val,
                                                status_icon,
                                                exit_code,
                                            )?;
                                        }

                                        // Render unified tool output (handles all formatting)
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
                                            renderer
                                                .line(MessageStyle::Info, "Changes discarded.")?;
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
                                            && !notice.trim().is_empty()
                                        {
                                            notice_lines.push(notice.trim().to_string());
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

                                        let mut hook_block_reason: Option<String> = None;

                                        if let Some(hooks) = &lifecycle_hooks {
                                            match hooks
                                                .run_post_tool_use(name, Some(&args_val), &output)
                                                .await
                                            {
                                                Ok(outcome) => {
                                                    render_hook_messages(
                                                        &mut renderer,
                                                        &outcome.messages,
                                                    )?;
                                                    for context in outcome.additional_context {
                                                        if !context.trim().is_empty() {
                                                            working_history.push(
                                                                uni::Message::system(context),
                                                            );
                                                        }
                                                    }
                                                    if let Some(reason) = outcome.block_reason {
                                                        let trimmed = reason.trim();
                                                        if !trimmed.is_empty() {
                                                            renderer.line(
                                                                MessageStyle::Info,
                                                                trimmed,
                                                            )?;
                                                            hook_block_reason =
                                                                Some(trimmed.to_string());
                                                        }
                                                    }
                                                }
                                                Err(err) => {
                                                    renderer.line(
                                                        MessageStyle::Error,
                                                        &format!(
                                                            "Failed to run post-tool hooks: {}",
                                                            err
                                                        ),
                                                    )?;
                                                }
                                            }
                                        }

                                        if let Some(reason) = hook_block_reason {
                                            let blocked_message = format!(
                                                "Tool execution blocked by lifecycle hooks: {}",
                                                reason
                                            );
                                            working_history
                                                .push(uni::Message::system(blocked_message));

                                            {
                                                let mut ledger = decision_ledger.write().await;
                                                ledger.record_outcome(
                                                    &dec_id,
                                                    DecisionOutcome::Failure {
                                                        error: reason.clone(),
                                                        recovery_attempts: 0,
                                                        context_preserved: true,
                                                    },
                                                );
                                            }

                                            session_end_reason = SessionEndReason::Cancelled;
                                            break 'outer TurnLoopResult::Blocked {
                                                reason: Some(reason),
                                            };
                                        }

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

                                        let allow_short_circuit = !has_more
                                            && command_success
                                            && should_short_circuit_shell(input, name, &args_val);

                                        if allow_short_circuit {
                                            let reply = derive_recent_tool_output(&working_history)
                                                .unwrap_or_else(|| {
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

                                        // Display failure indicator with clear messaging
                                        let palette = ColorPalette::default();
                                        let failure_msg = format!(
                                            "{} Tool '{}' failed",
                                            render_styled("✗", palette.error, None),
                                            name
                                        );
                                        renderer.line(MessageStyle::Error, &failure_msg)?;

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
                                        let classified_clone = classified.clone();
                                        let structured = ToolExecutionError::with_original_error(
                                            name.to_string(),
                                            classified,
                                            error_summary.clone(),
                                            original_details,
                                        );
                                        let error_json = structured.to_json_value();
                                        let error_message = structured.message.clone();

                                        if let Some(tool_name) = name.strip_prefix("mcp_") {
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

                                        // Display error details
                                        renderer.line(
                                            MessageStyle::Error,
                                            &format!("Error: {}", error_message),
                                        )?;

                                        // Display error type for better understanding
                                        let error_type_msg = match classified_clone {
                                            ToolErrorType::InvalidParameters => {
                                                "Invalid parameters provided"
                                            }
                                            ToolErrorType::ToolNotFound => "Tool not found",
                                            ToolErrorType::ResourceNotFound => "Resource not found",
                                            ToolErrorType::PermissionDenied => "Permission denied",
                                            ToolErrorType::ExecutionError => "Execution error",
                                            ToolErrorType::PolicyViolation => "Policy violation",
                                            ToolErrorType::Timeout => "Operation timed out",
                                            ToolErrorType::NetworkError => "Network error",
                                        };
                                        renderer.line(
                                            MessageStyle::Info,
                                            &format!("Type: {}", error_type_msg),
                                        )?;

                                        // Encourage retry with helpful message
                                        renderer.line(
                                        MessageStyle::Info,
                                        "💡 Tip: Review the error and try again with corrected parameters",
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
                                    ToolExecutionStatus::Cancelled => {
                                        tool_spinner.finish();

                                        safe_force_redraw(&handle, &mut last_forced_redraw);
                                        tokio::time::sleep(Duration::from_millis(50)).await;

                                        renderer.line_if_not_empty(MessageStyle::Output)?;
                                        renderer.line(
                                            MessageStyle::Info,
                                            "Operation cancelled by user. Stopping current turn.",
                                        )?;

                                        let cancel_error = ToolExecutionError::new(
                                            name.to_string(),
                                            ToolErrorType::ExecutionError,
                                            "Tool execution cancelled by user".to_string(),
                                        );
                                        let err_json = cancel_error.to_json_value();

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
                                                    error: "Cancelled by user".to_string(),
                                                    recovery_attempts: 0,
                                                    context_preserved: true,
                                                },
                                            );
                                        }

                                        break 'outer TurnLoopResult::Cancelled;
                                    }
                                }
                            }
                            Ok(ToolPermissionFlow::Denied) => {
                                // Force redraw after modal closes
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                tokio::time::sleep(Duration::from_millis(50)).await;

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
                                // Force redraw after modal closes
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                tokio::time::sleep(Duration::from_millis(50)).await;

                                renderer.line(MessageStyle::Info, "Goodbye!")?;
                                session_end_reason = SessionEndReason::Exit;
                                break 'outer TurnLoopResult::Cancelled;
                            }
                            Ok(ToolPermissionFlow::Interrupted) => {
                                // Force redraw after modal closes
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                tokio::time::sleep(Duration::from_millis(50)).await;

                                break 'outer TurnLoopResult::Cancelled;
                            }
                            Err(err) => {
                                // Force redraw after modal closes
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                tokio::time::sleep(Duration::from_millis(50)).await;

                                traj.log_tool_call(working_history.len(), name, &args_val, false);
                                renderer.line(
                                    MessageStyle::Error,
                                    &format!(
                                        "Failed to evaluate policy for tool '{}': {}",
                                        name, err
                                    ),
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
                    allow_follow_up = true;
                    continue 'outer;
                }

                if let Some(mut text) = final_text {
                    let do_review = vt_cfg
                        .as_ref()
                        .map(|cfg| cfg.agent.enable_self_review)
                        .unwrap_or(false);
                    let review_passes = vt_cfg
                        .as_ref()
                        .map(|cfg| cfg.agent.max_review_passes)
                        .unwrap_or(1)
                        .max(1);
                    let should_run_review =
                        do_review && (text.len() >= SELF_REVIEW_MIN_LENGTH || text.contains("```"));
                    if should_run_review {
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

                    // Empty responses mean we're done; avoid spinning another iteration.
                    if trimmed.is_empty() {
                        break TurnLoopResult::Completed;
                    }

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
                    working_history.push(
                        uni::Message::assistant(text.clone())
                            .with_reasoning(assistant_reasoning.clone()),
                    );
                    let _ = last_tool_stdout.take();
                    break TurnLoopResult::Completed;
                }
                continue;
            };

            match turn_result {
                TurnLoopResult::Cancelled => {
                    if ctrl_c_state.is_exit_requested() {
                        session_end_reason = SessionEndReason::Exit;
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
                    session_end_reason = SessionEndReason::Cancelled;
                    continue;
                }
                TurnLoopResult::Aborted => {
                    let _ = conversation_history.pop();
                    continue;
                }
                TurnLoopResult::Blocked { reason: _ } => {
                    conversation_history = working_history;
                    handle.clear_input();
                    handle.set_placeholder(default_placeholder.clone());
                    continue;
                }
                TurnLoopResult::Completed => {
                    conversation_history = working_history;

                    let _pruned_after_turn =
                        context_manager.prune_tool_responses(&mut conversation_history);
                    // Removed: Tool response pruning message after completion
                    let post_trim =
                        context_manager.enforce_context_window(&mut conversation_history);
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
                        let text = last.content.as_text();
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
                        let description = conversation_history
                            .last()
                            .map(|msg| msg.content.as_text())
                            .unwrap_or_default();
                        let description = description.trim().to_string();
                        match manager
                            .create_snapshot(
                                turn_number,
                                description.as_str(),
                                &conversation_snapshot,
                                &turn_modified_files,
                            )
                            .await
                        {
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
