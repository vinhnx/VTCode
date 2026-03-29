use super::types::{BackgroundTaskGuard, SessionState, SessionUISetup};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::reasoning::{
    model_supports_reasoning, resolve_reasoning_visibility,
};
use crate::agent::runloop::unified::session_setup::ide_context::{
    IdeContextBridge, status_line_editor_label, tui_header_summary,
};
use crate::agent::runloop::unified::stop_requests::request_local_stop;
use crate::agent::runloop::unified::turn::utils::render_hook_messages;
use crate::agent::runloop::unified::turn::workspace::load_workspace_files;
use crate::agent::runloop::unified::{context_manager, palettes, state};
use anyhow::{Context, Result};
use chrono::Local;
use hashbrown::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::sync::{Notify, mpsc::UnboundedSender};
use tracing::warn;
use vtcode_core::config::constants::ui;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
use vtcode_core::llm::provider as uni;
use vtcode_core::notifications::{
    set_global_notification_hook_engine, set_global_terminal_focused,
};
use vtcode_core::persistent_memory::{PersistentMemoryStatus, persistent_memory_status};
use vtcode_core::prompts::discover_prompt_templates;
use vtcode_core::subagents::{SubagentStatusEntry, SubagentThreadSnapshot};
use vtcode_core::ui::slash::visible_commands;
use vtcode_core::ui::theme;
use vtcode_core::ui::{
    inline_theme_from_core_styles, is_tui_mode, set_tui_mode, to_tui_appearance,
    to_tui_keyboard_protocol, to_tui_slash_commands, to_tui_surface,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_core::utils::transcript;
use vtcode_core::{CommandExecutionStatus, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use vtcode_tui::app::{
    AgentPaletteItem, FocusChangeCallback, InlineEvent, InlineEventCallback, InlineHandle,
    InlineHeaderContext, InlineHeaderHighlight, InlineHeaderStatusBadge, InlineHeaderStatusTone,
    LocalAgentEntry, LocalAgentKind, SessionOptions, SlashCommandItem, spawn_session_with_options,
};

pub(crate) async fn initialize_session_ui(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    session_state: &mut SessionState,
    resume_state: Option<&ResumeSession>,
    session_archive: Option<SessionArchive>,
    full_auto: bool,
    skip_confirmations: bool,
    steering_sender: Option<UnboundedSender<SteeringMessage>>,
) -> Result<SessionUISetup> {
    let session_trigger = if resume_state.is_some() {
        SessionStartTrigger::Resume
    } else {
        SessionStartTrigger::Startup
    };
    let lifecycle_hooks = if let Some(vt) = vt_cfg {
        LifecycleHookEngine::new(config.workspace.clone(), &vt.hooks, session_trigger)?
    } else {
        None
    };
    set_global_notification_hook_engine(lifecycle_hooks.clone());

    let mut context_manager = context_manager::ContextManager::new(
        session_state.base_system_prompt.clone(),
        (),
        session_state.loaded_skills.clone(),
        vt_cfg.map(|cfg| cfg.agent.clone()),
    );
    context_manager.set_workspace_root(config.workspace.as_path());

    let active_styles = theme::active_styles();
    let theme_spec = inline_theme_from_core_styles(&active_styles);
    let default_placeholder = session_state
        .session_bootstrap
        .placeholder
        .clone()
        .or_else(|| Some(ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.to_string()));
    let follow_up_placeholder = if session_state.session_bootstrap.placeholder.is_none() {
        Some(ui::CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.to_string())
    } else {
        None
    };
    let inline_rows = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.inline_viewport_rows)
        .unwrap_or(ui::DEFAULT_INLINE_VIEWPORT_ROWS);

    if !is_tui_mode() {
        set_tui_mode(true);
    }

    let ctrl_c_state = Arc::new(state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let input_activity_counter = Arc::new(AtomicU64::new(0));
    let interrupt_callback: InlineEventCallback = {
        let state = ctrl_c_state.clone();
        let notify = ctrl_c_notify.clone();
        let steering_sender = steering_sender.clone();
        Arc::new(move |event: &InlineEvent| match event {
            InlineEvent::Interrupt => {
                let _ = request_local_stop(&state, &notify);
            }
            InlineEvent::Pause => {
                if let Some(sender) = steering_sender.as_ref() {
                    let _ = sender.send(SteeringMessage::Pause);
                }
            }
            InlineEvent::Resume => {
                if let Some(sender) = steering_sender.as_ref() {
                    let _ = sender.send(SteeringMessage::Resume);
                }
            }
            InlineEvent::Steer(text) => {
                if let Some(sender) = steering_sender.as_ref() {
                    let _ = sender.send(SteeringMessage::FollowUpInput(text.clone()));
                }
            }
            _ => {}
        })
    };
    let focus_callback: FocusChangeCallback = Arc::new(set_global_terminal_focused);

    let pty_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    session_state
        .tool_registry
        .set_active_pty_sessions(pty_counter.clone());

    let visible_slash_commands: Vec<_> = visible_commands().into_iter().copied().collect();
    let mut slash_command_items = to_tui_slash_commands(visible_slash_commands.as_slice());
    let template_slash_commands = discover_prompt_templates(&config.workspace)
        .await
        .into_iter()
        .filter(|template| {
            !visible_slash_commands
                .iter()
                .any(|cmd| cmd.name == template.name)
        })
        .map(|template| SlashCommandItem::new(template.name, template.description))
        .collect::<Vec<_>>();
    slash_command_items.extend(template_slash_commands);

    let mut session = spawn_session_with_options(
        theme_spec.clone(),
        SessionOptions {
            placeholder: default_placeholder.clone(),
            surface_preference: vt_cfg
                .and_then(|cfg| cfg.tui.alternate_screen)
                .map(|mode| match mode {
                    vtcode_core::config::TuiAlternateScreen::Always => {
                        vtcode_tui::app::SessionSurface::Alternate
                    }
                    vtcode_core::config::TuiAlternateScreen::Never => {
                        vtcode_tui::app::SessionSurface::Inline
                    }
                })
                .unwrap_or_else(|| to_tui_surface(config.ui_surface)),
            inline_rows,
            event_callback: Some(interrupt_callback),
            focus_callback: Some(focus_callback),
            active_pty_sessions: Some(pty_counter.clone()),
            input_activity_counter: Some(input_activity_counter.clone()),
            keyboard_protocol: vt_cfg
                .map(|cfg| to_tui_keyboard_protocol(cfg.ui.keyboard_protocol.clone()))
                .unwrap_or_default(),
            workspace_root: Some(config.workspace.clone()),
            slash_commands: slash_command_items,
            appearance: vt_cfg.map(to_tui_appearance),
            app_name: "VT Code".to_string(),
            non_interactive_hint: Some(
                "Use `vtcode ask \"your prompt\"` for non-interactive input.".to_string(),
            ),
        },
    )
    .context("failed to launch inline session")?;
    set_global_terminal_focused(true);
    if skip_confirmations {
        session.set_skip_confirmations(true);
    }

    let handle = session.clone_inline_handle();
    let highlight_config = vt_cfg
        .as_ref()
        .map(|cfg| cfg.syntax_highlighting.clone())
        .unwrap_or_default();

    transcript::set_inline_handle(Arc::new(handle.clone()));
    let mut ide_context_bridge = Some(IdeContextBridge::new(config.workspace.clone()));
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);
    let supports_reasoning =
        model_supports_reasoning(&*session_state.provider_client, &config.model);
    renderer.set_reasoning_visible(resolve_reasoning_visibility(vt_cfg, supports_reasoning));
    if let Some(cfg) = vt_cfg {
        renderer.set_screen_reader_mode(cfg.ui.screen_reader_mode);
        renderer.set_show_diagnostics_in_transcript(cfg.ui.show_diagnostics_in_transcript);
    }
    let workspace_for_indexer = config.workspace.clone();
    let workspace_for_palette = config.workspace.clone();
    let handle_for_indexer = handle.clone();
    let file_palette_task_guard = BackgroundTaskGuard::new(tokio::spawn(async move {
        match load_workspace_files(workspace_for_indexer).await {
            Ok(files) => {
                if !files.is_empty() {
                    handle_for_indexer.configure_file_palette(files, workspace_for_palette);
                } else {
                    tracing::debug!("No files found in workspace for file palette");
                }
            }
            Err(err) => {
                tracing::warn!("Failed to load workspace files for file palette: {}", err);
            }
        }
    }));
    let mut background_subprocess_task_guard = None;
    if let Some(controller) = session_state.tool_registry.subagent_controller() {
        let handle_for_agents = handle.clone();
        let controller_for_agents = controller.clone();
        tokio::spawn(async move {
            let specs = controller_for_agents.effective_specs().await;
            if specs.is_empty() {
                return;
            }

            handle_for_agents.configure_agent_palette(
                specs
                    .into_iter()
                    .map(|spec| AgentPaletteItem {
                        name: spec.name,
                        description: Some(spec.description),
                    })
                    .collect(),
            );
        });

        let handle_for_subprocesses = handle.clone();
        let controller_for_subprocesses = controller.clone();
        let refresh_interval_ms = vt_cfg
            .map(|cfg| cfg.subagents.background.refresh_interval_ms)
            .unwrap_or(2_000)
            .max(250);
        background_subprocess_task_guard =
            Some(BackgroundTaskGuard::new(tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_millis(refresh_interval_ms));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    interval.tick().await;
                    if let Err(err) =
                        refresh_local_agents(&handle_for_subprocesses, &controller_for_subprocesses)
                            .await
                    {
                        tracing::warn!("Failed to refresh background subprocesses: {}", err);
                    }
                }
            })));
    }

    transcript::clear();
    render_resume_state_if_present(&mut renderer, resume_state, supports_reasoning)?;

    let provider_label = {
        let label = super::init::resolve_provider_label(config, vt_cfg);
        if label.is_empty() {
            session_state.provider_client.name().to_string()
        } else {
            label
        }
    };
    let header_provider_label = provider_label.clone();

    let mut checkpoint_config =
        vtcode_core::core::agent::snapshots::SnapshotConfig::new(config.workspace.clone());
    checkpoint_config.enabled = config.checkpointing_enabled;
    checkpoint_config.storage_dir = config.checkpointing_storage_dir.clone();
    checkpoint_config.max_snapshots = config.checkpointing_max_snapshots;
    checkpoint_config.max_age_days = config.checkpointing_max_age_days;
    let checkpoint_manager =
        match vtcode_core::core::agent::snapshots::SnapshotManager::new(checkpoint_config) {
            Ok(manager) => Some(manager),
            Err(err) => {
                warn!("Failed to initialize checkpoint manager: {}", err);
                None
            }
        };

    if let (Some(hooks), Some(archive)) = (&lifecycle_hooks, session_archive.as_ref()) {
        hooks
            .update_transcript_path(Some(archive.path().to_path_buf()))
            .await;
    }

    if let Some(hooks) = &lifecycle_hooks {
        match hooks.run_session_start().await {
            Ok(outcome) => {
                render_hook_messages(&mut renderer, &outcome.messages)?;
                for context in outcome.additional_context {
                    if !context.trim().is_empty() {
                        session_state
                            .conversation_history
                            .push(uni::Message::system(context));
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

    if full_auto && let Some(allowlist) = session_state.full_auto_allowlist.as_ref() {
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

    let persistent_memory_status = load_persistent_memory_status(config, vt_cfg);
    if let Err(err) = persistent_memory_status.as_ref() {
        warn!(
            workspace = %config.workspace.display(),
            error = ?err,
            "Failed to load persistent memory status for TUI guide"
        );
        renderer.line(
            MessageStyle::Warning,
            "Persistent memory is enabled, but VT Code couldn't load the TUI memory guide.",
        )?;
    }
    let persistent_memory_status = persistent_memory_status.ok().flatten();

    if let Some(notice) = session_state.session_bootstrap.search_tools_notice.as_ref() {
        notice.render(&mut renderer)?;
    }
    maybe_render_openai_priority_notice(&mut renderer, config, vt_cfg)?;

    handle.set_theme(theme_spec.clone());
    palettes::apply_prompt_style(&handle);
    handle.set_placeholder(default_placeholder.clone());

    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());

    let mode_label = match (config.ui_surface, full_auto) {
        (vtcode_core::config::types::UiSurfacePreference::Inline, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Inline, false) => "inline".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => "alt".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, false) => "std".to_string(),
    };
    let mut header_context = build_inline_header_context(
        config,
        &session_state.session_bootstrap,
        header_provider_label,
        config.model.clone(),
        session_state
            .provider_client
            .effective_context_size(&config.model),
        mode_label,
        reasoning_label.clone(),
    )
    .await?;
    if let Some(memory_status) = persistent_memory_status.as_ref() {
        apply_persistent_memory_header_guide(&mut header_context, memory_status);
    }

    let initial_editor_snapshot = if let Some(bridge) = ide_context_bridge.as_mut() {
        match bridge.refresh() {
            Ok((snapshot, _)) => snapshot,
            Err(err) => {
                warn!("Failed to refresh IDE context snapshot: {}", err);
                None
            }
        }
    } else {
        None
    };
    apply_ide_context_snapshot(
        &mut context_manager,
        &mut header_context,
        &handle,
        config.workspace.as_path(),
        vt_cfg,
        initial_editor_snapshot,
    );

    let mut startup_update_notice_rx = None;
    let mut startup_update_task_guard = None;
    if session_state.startup_update_check.should_refresh {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let announced_version = session_state
            .startup_update_check
            .cached_notice
            .as_ref()
            .map(|notice| notice.latest_version.clone());
        startup_update_notice_rx = Some(rx);
        startup_update_task_guard = Some(BackgroundTaskGuard::new(tokio::spawn(async move {
            let updater = match crate::updater::Updater::new(env!("CARGO_PKG_VERSION")) {
                Ok(updater) => updater,
                Err(err) => {
                    tracing::debug!("Failed to initialize updater in background task: {}", err);
                    return;
                }
            };

            match updater.refresh_startup_update_cache().await {
                Ok(Some(notice)) if Some(notice.latest_version.clone()) != announced_version => {
                    let _ = tx.send(notice);
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::debug!("Background startup update refresh failed: {}", err);
                }
            }
        })));
    }

    let next_checkpoint_turn = checkpoint_manager
        .as_ref()
        .and_then(|manager| manager.next_turn_number().ok())
        .unwrap_or(1);

    Ok(SessionUISetup {
        renderer,
        session,
        handle,
        header_context,
        ide_context_bridge,
        ctrl_c_state,
        ctrl_c_notify,
        input_activity_counter,
        checkpoint_manager,
        session_archive,
        lifecycle_hooks,
        session_end_reason: SessionEndReason::Completed,
        context_manager,
        default_placeholder,
        follow_up_placeholder,
        next_checkpoint_turn,
        file_palette_task_guard,
        background_subprocess_task_guard,
        startup_update_cached_notice: session_state.startup_update_check.cached_notice.clone(),
        startup_update_notice_rx,
        startup_update_task_guard,
    })
}

pub(crate) async fn refresh_local_agents(
    handle: &InlineHandle,
    controller: &Arc<vtcode_core::subagents::SubagentController>,
) -> Result<()> {
    let background_entries = controller.refresh_background_processes().await?;
    let delegated_entries = controller.status_entries().await;
    let local_agents =
        build_local_agent_entries(controller, delegated_entries, background_entries).await;
    handle.set_local_agents(local_agents);
    Ok(())
}

async fn build_local_agent_entries(
    controller: &Arc<vtcode_core::subagents::SubagentController>,
    delegated_entries: Vec<SubagentStatusEntry>,
    background_entries: Vec<vtcode_core::subagents::BackgroundSubprocessEntry>,
) -> Vec<LocalAgentEntry> {
    let mut entries = Vec::new();

    for entry in visible_delegated_local_agents(delegated_entries) {
        let snapshot = match controller.snapshot_for_thread(&entry.id).await {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                tracing::debug!(
                    subagent_id = entry.id.as_str(),
                    "Failed to snapshot delegated agent for local-agents UI: {}",
                    err
                );
                None
            }
        };
        let preview = snapshot
            .as_ref()
            .map(|snapshot| delegated_local_agent_preview(&entry, snapshot))
            .unwrap_or_else(|| delegated_local_agent_preview_placeholder(&entry));
        let summary = snapshot
            .as_ref()
            .map(|snapshot| delegated_local_agent_summary(&entry, snapshot));
        entries.push((
            entry.updated_at,
            LocalAgentEntry {
                id: entry.id.clone(),
                display_label: entry.display_label.clone(),
                agent_name: entry.agent_name.clone(),
                color: entry.color.clone(),
                kind: LocalAgentKind::Delegated,
                status: entry.status.as_str().to_string(),
                summary,
                preview,
                transcript_path: entry.transcript_path.clone(),
            },
        ));
    }

    for entry in visible_background_local_agents(background_entries) {
        let snapshot = match controller.background_snapshot(&entry.id).await {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                tracing::debug!(
                    subprocess_id = entry.id.as_str(),
                    "Failed to snapshot background subprocess for local-agents UI: {}",
                    err
                );
                None
            }
        };
        let preview = snapshot
            .as_ref()
            .map(background_local_agent_preview)
            .unwrap_or_else(|| background_local_agent_preview_placeholder(&entry));
        entries.push((
            entry.updated_at,
            LocalAgentEntry {
                id: entry.id.clone(),
                display_label: entry.display_label.clone(),
                agent_name: entry.agent_name.clone(),
                color: entry.color.clone(),
                kind: LocalAgentKind::Background,
                status: entry.status.as_str().to_string(),
                summary: Some(background_local_agent_summary(&entry)),
                preview,
                transcript_path: entry.transcript_path.clone().or(entry.archive_path.clone()),
            },
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    entries.into_iter().map(|(_, entry)| entry).collect()
}

fn visible_delegated_local_agents(entries: Vec<SubagentStatusEntry>) -> Vec<SubagentStatusEntry> {
    let mut entries = entries
        .into_iter()
        .filter(|entry| {
            !matches!(
                entry.status,
                vtcode_core::subagents::SubagentStatus::Completed
                    | vtcode_core::subagents::SubagentStatus::Closed
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    entries
}

fn visible_background_local_agents(
    entries: Vec<vtcode_core::subagents::BackgroundSubprocessEntry>,
) -> Vec<vtcode_core::subagents::BackgroundSubprocessEntry> {
    let mut entries = entries
        .into_iter()
        .filter(|entry| {
            matches!(
                entry.status,
                vtcode_core::subagents::BackgroundSubprocessStatus::Starting
                    | vtcode_core::subagents::BackgroundSubprocessStatus::Running
            ) || (entry.desired_enabled
                && matches!(
                    entry.status,
                    vtcode_core::subagents::BackgroundSubprocessStatus::Error
                ))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    entries
}

fn delegated_local_agent_summary(
    entry: &SubagentStatusEntry,
    snapshot: &SubagentThreadSnapshot,
) -> String {
    entry
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if snapshot.snapshot.turn_in_flight {
                "Turn in flight; streaming live updates.".to_string()
            } else if matches!(entry.status, vtcode_core::subagents::SubagentStatus::Failed) {
                entry
                    .error
                    .as_deref()
                    .map(str::trim)
                    .filter(|error| !error.is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| {
                        "Delegated agent failed before producing a summary.".to_string()
                    })
            } else if matches!(entry.status, vtcode_core::subagents::SubagentStatus::Queued) {
                "Queued and waiting to start.".to_string()
            } else {
                "Running without a final summary yet.".to_string()
            }
        })
}

fn delegated_local_agent_preview(
    entry: &SubagentStatusEntry,
    snapshot: &SubagentThreadSnapshot,
) -> String {
    let preview = summarize_subagent_sidebar_preview(snapshot);
    if preview.trim().is_empty() {
        delegated_local_agent_preview_placeholder(entry)
    } else {
        preview
    }
}

fn delegated_local_agent_preview_placeholder(entry: &SubagentStatusEntry) -> String {
    if matches!(entry.status, vtcode_core::subagents::SubagentStatus::Queued) {
        "Agent is queued and has not emitted transcript output yet.".to_string()
    } else if matches!(entry.status, vtcode_core::subagents::SubagentStatus::Failed) {
        entry
            .error
            .as_deref()
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "Agent failed before emitting more transcript output.".to_string())
    } else {
        "Waiting for the next delegated transcript update.".to_string()
    }
}

fn background_local_agent_summary(
    entry: &vtcode_core::subagents::BackgroundSubprocessEntry,
) -> String {
    entry
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| match entry.status {
            vtcode_core::subagents::BackgroundSubprocessStatus::Starting => {
                "Starting; waiting for subprocess output.".to_string()
            }
            vtcode_core::subagents::BackgroundSubprocessStatus::Running => {
                "Running; waiting for transcript output.".to_string()
            }
            vtcode_core::subagents::BackgroundSubprocessStatus::Stopped => "Stopped.".to_string(),
            vtcode_core::subagents::BackgroundSubprocessStatus::Error => {
                "Exited with an error.".to_string()
            }
        })
}

fn background_local_agent_preview(
    snapshot: &vtcode_core::subagents::BackgroundSubprocessSnapshot,
) -> String {
    if snapshot.preview.trim().is_empty() {
        background_local_agent_preview_placeholder(&snapshot.entry)
    } else {
        snapshot.preview.clone()
    }
}

fn background_local_agent_preview_placeholder(
    entry: &vtcode_core::subagents::BackgroundSubprocessEntry,
) -> String {
    match entry.status {
        vtcode_core::subagents::BackgroundSubprocessStatus::Starting => {
            "Waiting for the subprocess to emit output...".to_string()
        }
        vtcode_core::subagents::BackgroundSubprocessStatus::Running => {
            "Subprocess is running; waiting for the next transcript update.".to_string()
        }
        vtcode_core::subagents::BackgroundSubprocessStatus::Stopped => {
            "Subprocess stopped.".to_string()
        }
        vtcode_core::subagents::BackgroundSubprocessStatus::Error => {
            "Subprocess ended with an error.".to_string()
        }
    }
}

fn summarize_subagent_sidebar_preview(snapshot: &SubagentThreadSnapshot) -> String {
    let live_preview = summarize_thread_event_preview(&snapshot.recent_events);
    if !live_preview.is_empty() {
        return live_preview;
    }

    snapshot
        .snapshot
        .messages
        .iter()
        .rev()
        .filter_map(|message| {
            let text = message.content.as_text();
            let preview = summarize_preview_text(text.as_ref())?;
            Some(format!("{:?}: {}", message.role, preview))
        })
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn summarize_thread_event_preview(events: &[ThreadEvent]) -> String {
    let mut items = Vec::<(String, String)>::new();
    for event in events {
        let Some((item_id, line)) = thread_event_preview_line(event) else {
            continue;
        };
        if let Some((_, current)) = items.iter_mut().find(|(id, _)| id == &item_id) {
            *current = line;
        } else {
            items.push((item_id, line));
        }
    }

    items
        .into_iter()
        .map(|(_, line)| line)
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn thread_event_preview_line(event: &ThreadEvent) -> Option<(String, String)> {
    let item = match event {
        ThreadEvent::ItemStarted(event) => &event.item,
        ThreadEvent::ItemUpdated(event) => &event.item,
        ThreadEvent::ItemCompleted(event) => &event.item,
        _ => return None,
    };

    let line = match &item.details {
        ThreadItemDetails::AgentMessage(message) => {
            format!("assistant: {}", summarize_preview_text(&message.text)?)
        }
        ThreadItemDetails::Reasoning(reasoning) => {
            format!("thinking: {}", summarize_preview_text(&reasoning.text)?)
        }
        ThreadItemDetails::ToolInvocation(tool) => {
            format!(
                "tool {}: {}",
                tool.tool_name,
                tool_status_label(tool.status.clone())
            )
        }
        ThreadItemDetails::ToolOutput(output) => summarize_preview_text(&output.output)
            .map(|text| format!("tool output: {}", text))
            .unwrap_or_else(|| {
                format!("tool output: {}", tool_status_label(output.status.clone()))
            }),
        ThreadItemDetails::CommandExecution(command) => {
            summarize_preview_text(&command.aggregated_output)
                .map(|text| format!("command {}: {}", command.command, text))
                .unwrap_or_else(|| {
                    format!(
                        "command {}: {}",
                        command.command,
                        command_status_label(command.status.clone())
                    )
                })
        }
        _ => return None,
    };

    Some((item.id.clone(), line))
}

fn tool_status_label(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::InProgress => "running",
    }
}

fn command_status_label(status: CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::InProgress => "running",
    }
}

fn summarize_preview_text(text: &str) -> Option<String> {
    let preview = text
        .lines()
        .rev()
        .find_map(|line| {
            let collapsed = collapse_preview_whitespace(line);
            (!collapsed.is_empty()).then_some(collapsed)
        })
        .or_else(|| {
            let collapsed = collapse_preview_whitespace(text);
            (!collapsed.is_empty()).then_some(collapsed)
        })?;

    Some(truncate_preview_text(preview, 180))
}

fn collapse_preview_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_preview_text(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn maybe_render_openai_priority_notice(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<()> {
    if !config.provider.eq_ignore_ascii_case("openai") {
        return Ok(());
    }

    let default_auth = vtcode_auth::OpenAIAuthConfig::default();
    let auth_cfg = vt_cfg.map(|cfg| &cfg.auth.openai).unwrap_or(&default_auth);
    let storage_mode = vt_cfg
        .map(|cfg| cfg.agent.credential_storage_mode)
        .unwrap_or_default();
    let api_key = vtcode_core::config::api_keys::get_api_key(
        "openai",
        &vtcode_core::config::api_keys::ApiKeySources::default(),
    )
    .ok();
    let overview =
        vtcode_config::auth::summarize_openai_credentials(auth_cfg, storage_mode, api_key)?;
    let Some(notice) = overview.notice.as_deref() else {
        return Ok(());
    };

    renderer.line(MessageStyle::Info, notice)?;
    if let Some(recommendation) = overview.recommendation.as_deref() {
        renderer.line(MessageStyle::Output, recommendation)?;
    }
    Ok(())
}

fn persistent_memory_guide_lines(memory_status: &PersistentMemoryStatus) -> Vec<String> {
    let mut lines = Vec::with_capacity(3);
    if memory_status.cleanup_status.needed {
        lines.push(
            "Run `/memory` to inspect status and finish one-time cleanup before memory updates."
                .to_string(),
        );
    } else {
        lines.push("Use `/memory` to inspect notes, status, and quick actions.".to_string());
    }
    lines.push("Use `remember ...` to save a note or `forget ...` to remove one.".to_string());
    lines.push(if memory_status.auto_write {
        "Auto-write is on: VT Code may consolidate durable notes after a session.".to_string()
    } else {
        "Auto-write is off: VT Code will only change memory through explicit actions.".to_string()
    });
    lines
}

fn load_persistent_memory_status(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Option<PersistentMemoryStatus>> {
    let Some(vt_cfg) = vt_cfg else {
        return Ok(None);
    };
    let memory_config = &vt_cfg.agent.persistent_memory;
    if !memory_config.enabled {
        return Ok(None);
    }

    persistent_memory_status(memory_config, &config.workspace).map(Some)
}

fn persistent_memory_header_badge(
    memory_status: &PersistentMemoryStatus,
) -> InlineHeaderStatusBadge {
    if memory_status.cleanup_status.needed {
        return InlineHeaderStatusBadge {
            text: "Memory: cleanup".to_string(),
            tone: InlineHeaderStatusTone::Warning,
        };
    }

    let text = if memory_status.pending_rollout_summaries > 0 {
        format!(
            "Memory: {} pending",
            memory_status.pending_rollout_summaries
        )
    } else if memory_status.auto_write {
        "Memory: auto".to_string()
    } else {
        "Memory: manual".to_string()
    };
    InlineHeaderStatusBadge {
        text,
        tone: InlineHeaderStatusTone::Ready,
    }
}

fn persistent_memory_header_highlight(
    memory_status: &PersistentMemoryStatus,
) -> InlineHeaderHighlight {
    InlineHeaderHighlight {
        title: "Memory".to_string(),
        lines: persistent_memory_guide_lines(memory_status),
    }
}

fn apply_persistent_memory_header_guide(
    header_context: &mut InlineHeaderContext,
    memory_status: &PersistentMemoryStatus,
) {
    header_context.persistent_memory = Some(persistent_memory_header_badge(memory_status));
    header_context
        .highlights
        .push(persistent_memory_header_highlight(memory_status));
}

pub(crate) fn apply_ide_context_snapshot(
    context_manager: &mut crate::agent::runloop::unified::context_manager::ContextManager,
    header_context: &mut InlineHeaderContext,
    handle: &InlineHandle,
    workspace: &std::path::Path,
    vt_cfg: Option<&VTCodeConfig>,
    snapshot: Option<vtcode_core::EditorContextSnapshot>,
) {
    let ide_context_config = vt_cfg.map(|cfg| &cfg.ide_context);
    context_manager.set_editor_context_snapshot(snapshot.clone(), ide_context_config);
    let effective_ide_context_config =
        context_manager.effective_ide_context_config_with_base(ide_context_config);
    header_context.editor_context = tui_header_summary(
        workspace,
        Some(&effective_ide_context_config),
        snapshot.as_ref(),
    );
    handle.set_header_context(header_context.clone());
}

pub(crate) fn ide_context_status_label(
    context_manager: &crate::agent::runloop::unified::context_manager::ContextManager,
    workspace: &std::path::Path,
    vt_cfg: Option<&VTCodeConfig>,
    snapshot: Option<&vtcode_core::EditorContextSnapshot>,
    source: Option<&std::path::Path>,
) -> Option<String> {
    let effective_ide_context_config =
        context_manager.effective_ide_context_config_with_base(vt_cfg.map(|cfg| &cfg.ide_context));
    status_line_editor_label(
        workspace,
        Some(&effective_ide_context_config),
        snapshot,
        source,
    )
}

pub(crate) fn ide_context_status_label_from_bridge(
    context_manager: &crate::agent::runloop::unified::context_manager::ContextManager,
    workspace: &std::path::Path,
    vt_cfg: Option<&VTCodeConfig>,
    ide_context_bridge: Option<&IdeContextBridge>,
) -> Option<String> {
    ide_context_bridge.and_then(|bridge| {
        ide_context_status_label(
            context_manager,
            workspace,
            vt_cfg,
            bridge.snapshot(),
            bridge.snapshot_source(),
        )
    })
}

fn render_resume_state_if_present(
    renderer: &mut AnsiRenderer,
    resume_state: Option<&ResumeSession>,
    supports_reasoning: bool,
) -> Result<()> {
    let Some(session) = resume_state else {
        return Ok(());
    };

    let ended_local = session
        .snapshot()
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    let action = if session.is_fork() {
        "Forking"
    } else {
        "Resuming"
    };
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{} session {} · ended {} · {} messages",
            action,
            session.identifier(),
            ended_local,
            session.message_count()
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("Previous archive: {}", session.path().display()),
    )?;
    if session.is_fork() {
        renderer.line(MessageStyle::Info, "Starting independent forked session")?;
    }

    if !session.history().is_empty() {
        renderer.line(MessageStyle::Info, "Conversation history:")?;
        let lines = build_structured_resume_lines(session.history(), supports_reasoning);
        render_resume_lines(renderer, &lines)?;
    } else if !session.snapshot().transcript.is_empty() {
        renderer.line(
            MessageStyle::Info,
            "Conversation history (legacy transcript):",
        )?;
        let lines = build_legacy_resume_lines(&session.snapshot().transcript);
        render_resume_lines(renderer, &lines)?;
    }
    renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResumeRenderLine {
    style: MessageStyle,
    text: String,
}

impl ResumeRenderLine {
    fn new(style: MessageStyle, text: impl Into<String>) -> Self {
        Self {
            style,
            text: text.into(),
        }
    }
}

fn render_resume_lines(renderer: &mut AnsiRenderer, lines: &[ResumeRenderLine]) -> Result<()> {
    for line in lines {
        renderer.line(line.style, &line.text)?;
    }
    Ok(())
}

fn build_structured_resume_lines(
    history: &[uni::Message],
    supports_reasoning: bool,
) -> Vec<ResumeRenderLine> {
    let mut lines = Vec::new();
    let mut tool_name_by_call_id: HashMap<String, String> = HashMap::new();

    for (index, message) in history.iter().enumerate() {
        if index > 0 {
            push_resume_spacing(&mut lines);
        }
        match message.role {
            uni::MessageRole::User => {
                push_content_lines(&mut lines, MessageStyle::User, &message.content);
            }
            uni::MessageRole::Assistant => {
                let mut rendered_any = false;

                if let Some(tool_calls) = &message.tool_calls {
                    for tool_call in tool_calls {
                        rendered_any = true;
                        let tool_name = tool_call
                            .function
                            .as_ref()
                            .map(|function| function.name.clone())
                            .unwrap_or_else(|| "unknown".to_string());
                        if !tool_call.id.trim().is_empty() {
                            tool_name_by_call_id.insert(tool_call.id.clone(), tool_name.clone());
                        }

                        lines.push(ResumeRenderLine::new(
                            MessageStyle::Tool,
                            format_resume_tool_header(&tool_name, Some(tool_call.id.as_str())),
                        ));

                        if let Some(function) = &tool_call.function {
                            let args_block = format_tool_arguments_for_resume(&function.arguments);
                            if !args_block.is_empty() {
                                lines.push(ResumeRenderLine::new(
                                    MessageStyle::ToolDetail,
                                    args_block,
                                ));
                            }
                        } else if let Some(text) = tool_call.text.as_deref()
                            && !text.trim().is_empty()
                        {
                            lines.push(ResumeRenderLine::new(
                                MessageStyle::ToolDetail,
                                text.trim().to_string(),
                            ));
                        }
                    }
                }

                let reasoning_text = if supports_reasoning {
                    message
                        .reasoning
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                        .or_else(|| {
                            message
                                .reasoning_details
                                .as_deref()
                                .and_then(
                                    vtcode_core::llm::providers::common::extract_reasoning_text_from_detail_values,
                                )
                        })
                } else {
                    None
                };

                if let Some(reasoning) = reasoning_text {
                    rendered_any = true;
                    lines.push(ResumeRenderLine::new(MessageStyle::Reasoning, reasoning));
                }

                if let Some(content) = project_content_text(&message.content) {
                    rendered_any = true;
                    lines.push(ResumeRenderLine::new(MessageStyle::Response, content));
                }

                if !rendered_any {
                    lines.push(ResumeRenderLine::new(
                        MessageStyle::Response,
                        "Assistant: [no content]",
                    ));
                }
            }
            uni::MessageRole::Tool => {
                let call_id = message.tool_call_id.as_deref();
                let tool_name = call_id
                    .and_then(|id| tool_name_by_call_id.get(id))
                    .cloned()
                    .or_else(|| message.origin_tool.clone())
                    .unwrap_or_else(|| "tool".to_string());
                lines.push(ResumeRenderLine::new(
                    MessageStyle::Tool,
                    format_resume_tool_header(&tool_name, call_id),
                ));
                push_content_lines(&mut lines, MessageStyle::ToolOutput, &message.content);
            }
            uni::MessageRole::System => {
                lines.push(ResumeRenderLine::new(MessageStyle::Info, "System:"));
                push_content_lines(&mut lines, MessageStyle::Info, &message.content);
            }
        }
    }

    lines
}

fn format_resume_tool_header(tool_name: &str, tool_call_id: Option<&str>) -> String {
    let tool_name = vtcode_core::tools::tool_intent::canonical_unified_exec_tool_name(tool_name)
        .unwrap_or(tool_name);
    match tool_call_id {
        Some(id) if !id.trim().is_empty() && tool_name.trim().eq_ignore_ascii_case("tool") => {
            format!("Tool [tool_call_id: {}]:", id)
        }
        Some(id) if !id.trim().is_empty() => {
            format!("Tool {} [tool_call_id: {}]:", tool_name, id)
        }
        _ if tool_name.trim().eq_ignore_ascii_case("tool") => "Tool:".to_string(),
        _ => format!("Tool {}:", tool_name),
    }
}

fn format_tool_arguments_for_resume(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => serde_json::to_string_pretty(&value)
            .map(|pretty| format!("```json\n{}\n```", pretty))
            .unwrap_or_else(|_| format!("```json\n{}\n```", trimmed)),
        Err(_) => format!("```text\n{}\n```", trimmed),
    }
}

fn push_resume_spacing(lines: &mut Vec<ResumeRenderLine>) {
    if lines.last().is_none_or(|line| !line.text.is_empty()) {
        lines.push(ResumeRenderLine::new(MessageStyle::Info, ""));
    }
}

fn push_content_lines(
    lines: &mut Vec<ResumeRenderLine>,
    style: MessageStyle,
    content: &uni::MessageContent,
) {
    if let Some(projected) = project_content_text(content) {
        lines.push(ResumeRenderLine::new(style, projected));
    } else {
        lines.push(ResumeRenderLine::new(style, "[no textual content]"));
    }
}

fn project_content_text(content: &uni::MessageContent) -> Option<String> {
    match content {
        uni::MessageContent::Text(text) => (!text.trim().is_empty()).then(|| text.clone()),
        uni::MessageContent::Parts(parts) => {
            let mut fragments = Vec::new();
            for part in parts {
                match part {
                    uni::ContentPart::Text { text } => {
                        if !text.trim().is_empty() {
                            fragments.push(text.clone());
                        }
                    }
                    uni::ContentPart::Image { mime_type, .. } => {
                        fragments.push(format!("[image content: {}]", mime_type));
                    }
                    uni::ContentPart::File {
                        filename,
                        file_id,
                        file_url,
                        ..
                    } => {
                        if let Some(name) = filename {
                            fragments.push(format!("[file attachment: {}]", name));
                        } else if let Some(id) = file_id {
                            fragments.push(format!("[file attachment id: {}]", id));
                        } else if let Some(url) = file_url {
                            fragments.push(format!("[file attachment url: {}]", url));
                        } else {
                            fragments.push("[file attachment]".to_string());
                        }
                    }
                }
            }

            (!fragments.is_empty()).then(|| fragments.join("\n"))
        }
    }
}

fn build_legacy_resume_lines(transcript: &[String]) -> Vec<ResumeRenderLine> {
    transcript
        .iter()
        .map(|line| ResumeRenderLine::new(infer_legacy_line_style(line), line.clone()))
        .collect()
}

fn infer_legacy_line_style(line: &str) -> MessageStyle {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return MessageStyle::Info;
    }

    if trimmed.contains("You:") {
        return MessageStyle::User;
    }
    if trimmed.contains("Assistant:") {
        return MessageStyle::Response;
    }
    if trimmed.contains("System:") {
        return MessageStyle::Info;
    }
    if trimmed.contains("Tool ")
        || trimmed.contains("[tool_call_id:")
        || trimmed.contains("\"tool_call_id\"")
    {
        return MessageStyle::ToolOutput;
    }
    MessageStyle::Info
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashbrown::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use vtcode_core::persistent_memory::MemoryCleanupStatus;
    use vtcode_core::{EditorContextSnapshot, EditorFileContext};

    fn sample_memory_status() -> PersistentMemoryStatus {
        PersistentMemoryStatus {
            enabled: true,
            auto_write: true,
            directory: PathBuf::from("/tmp/memory"),
            summary_file: PathBuf::from("/tmp/memory/memory_summary.md"),
            memory_file: PathBuf::from("/tmp/memory/MEMORY.md"),
            preferences_file: PathBuf::from("/tmp/memory/preferences.md"),
            repository_facts_file: PathBuf::from("/tmp/memory/repository-facts.md"),
            rollout_summaries_dir: PathBuf::from("/tmp/memory/rollout_summaries"),
            summary_exists: true,
            registry_exists: true,
            pending_rollout_summaries: 0,
            cleanup_status: MemoryCleanupStatus {
                needed: false,
                suspicious_facts: 0,
                suspicious_summary_lines: 0,
            },
        }
    }

    #[test]
    fn structured_resume_lines_preserve_tool_context() {
        let mut assistant =
            uni::Message::assistant("cargo fmt completed successfully.".to_string());
        assistant.reasoning = Some("Need to run formatter before checks.".to_string());
        assistant.tool_calls = Some(vec![uni::ToolCall::function(
            "call_123".to_string(),
            "unified_exec".to_string(),
            "{\"cmd\":\"cargo fmt\"}".to_string(),
        )]);

        let mut tool_response =
            uni::Message::tool_response("call_123".to_string(), "{\"exit_code\":0}".to_string());
        tool_response.origin_tool = Some("unified_exec".to_string());

        let history = vec![
            uni::Message::user("run cargo fmt".to_string()),
            assistant,
            tool_response,
        ];

        let lines = build_structured_resume_lines(&history, true);

        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::User && line.text.contains("run cargo fmt")
        }));
        assert!(!lines.iter().any(|line| line.text == "You:"));
        assert!(!lines.iter().any(|line| line.text == "Assistant:"));
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::Tool
                && line
                    .text
                    .contains("Tool unified_exec [tool_call_id: call_123]:")
        }));
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::ToolDetail && line.text.starts_with("```json")
        }));
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::ToolOutput && line.text.contains("\"exit_code\":0")
        }));
    }

    #[test]
    fn legacy_style_inference_maps_common_prefixes() {
        assert_eq!(infer_legacy_line_style("  [1] You:"), MessageStyle::User);
        assert_eq!(
            infer_legacy_line_style("  [5] Assistant:"),
            MessageStyle::Response
        );
        assert_eq!(
            infer_legacy_line_style("System: startup"),
            MessageStyle::Info
        );
        assert_eq!(
            infer_legacy_line_style("Tool [tool_call_id: call_1]:"),
            MessageStyle::ToolOutput
        );
    }

    #[test]
    fn structured_resume_lines_fallback_to_reasoning_details() {
        let assistant =
            uni::Message::assistant("done".to_string()).with_reasoning_details(Some(vec![
                serde_json::json!(r#"{"type":"reasoning.text","text":"detail trace"}"#),
            ]));
        let lines = build_structured_resume_lines(&[assistant], true);
        assert!(lines.iter().any(|line| {
            line.style == MessageStyle::Reasoning && line.text.contains("detail trace")
        }));
    }

    #[test]
    fn structured_resume_lines_hide_reasoning_when_unsupported() {
        let mut assistant = uni::Message::assistant("done".to_string());
        assistant.reasoning = Some("trace".to_string());
        let lines = build_structured_resume_lines(&[assistant], false);
        assert!(
            !lines
                .iter()
                .any(|line| line.style == MessageStyle::Reasoning)
        );
    }

    #[test]
    fn persistent_memory_guide_lines_show_standard_actions() {
        let lines = persistent_memory_guide_lines(&sample_memory_status());
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("/memory"));
        assert!(lines[1].contains("remember"));
        assert!(lines[2].contains("Auto-write is on"));
    }

    #[test]
    fn persistent_memory_guide_lines_call_out_cleanup_when_needed() {
        let mut status = sample_memory_status();
        status.auto_write = false;
        status.cleanup_status.needed = true;

        let lines = persistent_memory_guide_lines(&status);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("one-time cleanup"));
        assert!(lines[2].contains("Auto-write is off"));
    }

    #[test]
    fn persistent_memory_header_badge_reflects_memory_mode() {
        let badge = persistent_memory_header_badge(&sample_memory_status());
        assert_eq!(badge.text, "Memory: auto");
        assert_eq!(badge.tone, InlineHeaderStatusTone::Ready);
    }

    #[test]
    fn persistent_memory_header_badge_warns_on_cleanup() {
        let mut status = sample_memory_status();
        status.cleanup_status.needed = true;

        let badge = persistent_memory_header_badge(&status);
        assert_eq!(badge.text, "Memory: cleanup");
        assert_eq!(badge.tone, InlineHeaderStatusTone::Warning);
    }

    #[test]
    fn apply_persistent_memory_header_guide_sets_badge_and_highlight() {
        let mut header_context = InlineHeaderContext::default();

        apply_persistent_memory_header_guide(&mut header_context, &sample_memory_status());

        assert_eq!(
            header_context
                .persistent_memory
                .as_ref()
                .map(|badge| badge.text.as_str()),
            Some("Memory: auto")
        );
        assert!(
            header_context
                .highlights
                .iter()
                .any(|highlight| highlight.title == "Memory")
        );
    }

    #[test]
    fn background_local_agent_visibility_hides_stopped_entries() {
        let entry = vtcode_core::subagents::BackgroundSubprocessEntry {
            id: "background-default".to_string(),
            session_id: "session-456".to_string(),
            exec_session_id: String::new(),
            agent_name: "default".to_string(),
            display_label: "default".to_string(),
            description: "Default agent".to_string(),
            source: "builtin".to_string(),
            color: None,
            status: vtcode_core::subagents::BackgroundSubprocessStatus::Stopped,
            desired_enabled: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            started_at: None,
            ended_at: None,
            pid: None,
            summary: None,
            error: None,
            archive_path: None,
            transcript_path: None,
        };

        assert!(visible_background_local_agents(vec![entry]).is_empty());
    }

    #[test]
    fn delegated_local_agent_preview_uses_queue_placeholder() {
        let entry = SubagentStatusEntry {
            id: "thread-1".to_string(),
            session_id: "session-123".to_string(),
            parent_thread_id: "main".to_string(),
            agent_name: "rust-engineer".to_string(),
            display_label: "rust-engineer".to_string(),
            description: "Review Rust changes".to_string(),
            source: "project".to_string(),
            color: None,
            status: vtcode_core::subagents::SubagentStatus::Queued,
            background: false,
            depth: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
            summary: None,
            error: None,
            transcript_path: None,
            nickname: None,
        };

        assert_eq!(
            delegated_local_agent_preview_placeholder(&entry),
            "Agent is queued and has not emitted transcript output yet."
        );
    }

    #[test]
    fn delegated_local_agent_visibility_keeps_failed_entries() {
        let entry = SubagentStatusEntry {
            id: "thread-1".to_string(),
            session_id: "session-123".to_string(),
            parent_thread_id: "main".to_string(),
            agent_name: "rust-engineer".to_string(),
            display_label: "rust-engineer".to_string(),
            description: "Review Rust changes".to_string(),
            source: "project".to_string(),
            color: None,
            status: vtcode_core::subagents::SubagentStatus::Failed,
            background: false,
            depth: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: Some(chrono::Utc::now()),
            summary: None,
            error: Some("subagent failed".to_string()),
            transcript_path: None,
            nickname: None,
        };

        let visible = visible_delegated_local_agents(vec![entry]);
        assert_eq!(visible.len(), 1);
    }

    #[test]
    fn delegated_local_agent_preview_uses_failure_message() {
        let entry = SubagentStatusEntry {
            id: "thread-1".to_string(),
            session_id: "session-123".to_string(),
            parent_thread_id: "main".to_string(),
            agent_name: "rust-engineer".to_string(),
            display_label: "rust-engineer".to_string(),
            description: "Review Rust changes".to_string(),
            source: "project".to_string(),
            color: None,
            status: vtcode_core::subagents::SubagentStatus::Failed,
            background: false,
            depth: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: Some(chrono::Utc::now()),
            summary: None,
            error: Some("subagent failed".to_string()),
            transcript_path: None,
            nickname: None,
        };

        assert_eq!(
            delegated_local_agent_preview_placeholder(&entry),
            "subagent failed"
        );
    }

    #[test]
    fn background_local_agent_preview_uses_status_placeholder() {
        let entry = vtcode_core::subagents::BackgroundSubprocessEntry {
            id: "background-default".to_string(),
            session_id: "session-456".to_string(),
            exec_session_id: String::new(),
            agent_name: "default".to_string(),
            display_label: "default".to_string(),
            description: "Default agent".to_string(),
            source: "builtin".to_string(),
            color: None,
            status: vtcode_core::subagents::BackgroundSubprocessStatus::Starting,
            desired_enabled: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            started_at: None,
            ended_at: None,
            pid: None,
            summary: None,
            error: None,
            archive_path: None,
            transcript_path: None,
        };

        assert_eq!(
            background_local_agent_preview_placeholder(&entry),
            "Waiting for the subprocess to emit output..."
        );
    }

    #[test]
    fn ide_context_status_label_respects_session_override() {
        let workspace = assert_fs::TempDir::new().expect("workspace");
        let mut context_manager =
            crate::agent::runloop::unified::context_manager::ContextManager::new(
                "sys".into(),
                (),
                Arc::new(RwLock::new(HashMap::new())),
                None,
            );
        context_manager.set_workspace_root(workspace.path());

        let snapshot = EditorContextSnapshot {
            workspace_root: Some(PathBuf::from(workspace.path())),
            active_file: Some(EditorFileContext {
                path: workspace.path().join("src/main.rs").display().to_string(),
                language_id: Some("rust".to_string()),
                line_range: None,
                dirty: false,
                truncated: false,
                selection: None,
            }),
            ..EditorContextSnapshot::default()
        };
        context_manager.set_editor_context_snapshot(
            Some(snapshot.clone()),
            Some(&vtcode_config::IdeContextConfig::default()),
        );

        assert_eq!(
            ide_context_status_label(
                &context_manager,
                workspace.path(),
                None,
                Some(&snapshot),
                None
            )
            .as_deref(),
            Some("IDE Context (IDE): src/main.rs")
        );

        assert!(!context_manager.toggle_session_ide_context());
        assert_eq!(
            ide_context_status_label(
                &context_manager,
                workspace.path(),
                None,
                Some(&snapshot),
                None
            ),
            None
        );
    }
}
