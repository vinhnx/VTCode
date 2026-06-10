//! Agent Legibility:
//! - Entrypoint: `initialize_session_ui` owns session bootstrap, inline header wiring, and TUI session launch.
//! - Common changes:
//!   - Local-agent sidebar refresh and preview logic live in `ui/local_agents.rs`.
//!   - Header assembly, OpenAI notices, and IDE snapshot wiring live in `ui/header_context.rs`.
//!   - Persistent-memory guide and header badges live in `ui/persistent_memory.rs`.
//!   - Resume rendering and transcript projection live in `ui/resume_render.rs`.
//! - Constraints: TD-005 is active for this surface; keep this file as an orchestration root and prefer responsibility-named support modules for new helper clusters.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode inline_events::tests`

mod header_context;
mod local_agents;
mod persistent_memory;
mod resume_render;

use super::types::{BackgroundTaskGuard, SessionState, SessionUISetup};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::unified::reasoning::{
    model_supports_reasoning, resolve_reasoning_visibility,
};
use crate::agent::runloop::unified::session_setup::ide_context::IdeContextBridge;
use crate::agent::runloop::unified::stop_requests::request_local_stop;
use crate::agent::runloop::unified::turn::utils::{
    append_additional_context, render_hook_messages,
};
use crate::agent::runloop::unified::turn::workspace::load_workspace_files;
use crate::agent::runloop::unified::{context_manager, state};
use anyhow::{Context, Result};
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
use vtcode_core::notifications::{
    set_global_notification_hook_engine, set_global_terminal_focused,
};
use vtcode_core::primary_agent::build_primary_agent_hook_config;
use vtcode_core::prompts::discover_prompt_templates;
use vtcode_core::ui::slash::visible_commands;
use vtcode_core::ui::theme;
use vtcode_core::ui::{
    inline_theme_from_core_styles, is_tui_mode, set_tui_mode, to_tui_appearance, to_tui_fullscreen,
    to_tui_keyboard_protocol, to_tui_slash_commands, to_tui_surface,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::load_user_config;
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_core::utils::transcript;
use vtcode_ui::tui::app::{
    AgentPaletteItem, FocusChangeCallback, InlineEvent, InlineEventCallback, SessionOptions,
    SlashCommandItem, spawn_session_with_options,
};

use self::header_context::{HeaderContextInit, initialize_header_context};
pub(crate) use self::header_context::{
    apply_ide_context_snapshot, ide_context_status_label_from_bridge,
};
pub(crate) use self::local_agents::refresh_local_agents;
use self::resume_render::render_resume_state_if_present;

#[cfg(test)]
use self::header_context::ide_context_status_label;
#[cfg(test)]
use self::local_agents::{
    background_local_agent_preview_placeholder, delegated_local_agent_preview_placeholder,
    visible_background_local_agents, visible_delegated_local_agents,
};
#[cfg(test)]
use self::persistent_memory::apply_persistent_memory_header_guide;
#[cfg(test)]
use self::persistent_memory::{persistent_memory_guide_lines, persistent_memory_header_badge};
#[cfg(test)]
use self::resume_render::{build_structured_resume_lines, infer_legacy_line_style};
#[cfg(test)]
use vtcode_core::llm::provider as uni;
#[cfg(test)]
use vtcode_core::persistent_memory::PersistentMemoryStatus;
#[cfg(test)]
use vtcode_core::subagents::SubagentStatusEntry;
#[cfg(test)]
use vtcode_ui::tui::app::InlineHeaderContext;
#[cfg(test)]
use vtcode_ui::tui::app::InlineHeaderStatusTone;

pub(crate) struct SessionUiLaunchOptions {
    pub session_archive: Option<SessionArchive>,
    pub full_auto: bool,
    pub skip_confirmations: bool,
    pub steering_sender: Option<UnboundedSender<SteeringMessage>>,
}

pub(crate) async fn initialize_session_ui(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    session_id: &str,
    session_state: &mut SessionState,
    session_trigger: SessionStartTrigger,
    resume_state: Option<&ResumeSession>,
    options: SessionUiLaunchOptions,
) -> Result<SessionUISetup> {
    let SessionUiLaunchOptions {
        session_archive,
        full_auto,
        skip_confirmations,
        steering_sender,
    } = options;

    let lifecycle_hooks = if let Some(vt) = vt_cfg {
        let hooks =
            build_primary_agent_hook_config(&vt.hooks, session_state.active_primary_agent.active());
        LifecycleHookEngine::new_with_session(
            config.workspace.clone(),
            &hooks,
            session_trigger,
            session_id,
        )?
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

    // Load user keybindings from dot config (best-effort)
    let user_key_bindings: HashMap<String, Vec<String>> = match load_user_config().await {
        Ok(dot) => dot
            .preferences
            .keybindings
            .into_iter()
            .map(|(action, key)| (action, vec![key]))
            .collect(),
        Err(_) => HashMap::new(),
    };

    let mut session = spawn_session_with_options(
        theme_spec.clone(),
        SessionOptions {
            placeholder: default_placeholder.clone(),
            surface_preference: vt_cfg
                .and_then(|cfg| cfg.tui.alternate_screen)
                .map(|mode| match mode {
                    vtcode_core::config::TuiAlternateScreen::Always => {
                        vtcode_ui::tui::app::SessionSurface::Alternate
                    }
                    vtcode_core::config::TuiAlternateScreen::Never => {
                        vtcode_ui::tui::app::SessionSurface::Inline
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
            fullscreen: vt_cfg.map(to_tui_fullscreen).unwrap_or_default(),
            workspace_root: Some(config.workspace.clone()),
            slash_commands: slash_command_items,
            appearance: vt_cfg.map(to_tui_appearance),
            app_name: "VT Code".to_string(),
            non_interactive_hint: Some(
                "Use `vtcode ask \"your prompt\"` for non-interactive input.".to_string(),
            ),
            key_bindings: user_key_bindings,
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
    if let Ok((width, _)) = crossterm::terminal::size() {
        renderer.set_table_max_width(Some(width as usize));
    }
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
                    .filter(|spec| spec.is_subagent())
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
                append_additional_context(
                    &mut session_state.conversation_history,
                    outcome.additional_context,
                );
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
                "Full-auto permission review enabled with no tool permissions; tool calls will be skipped.",
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Full-auto permission review enabled. Permitted tools: {}",
                    allowlist.join(", ")
                ),
            )?;
        }
    }

    handle.set_placeholder(default_placeholder.clone());

    let mut header_context = initialize_header_context(
        &mut renderer,
        &handle,
        &mut context_manager,
        &mut ide_context_bridge,
        HeaderContextInit {
            config,
            vt_cfg,
            session_bootstrap: &session_state.session_bootstrap,
            provider_client: &*session_state.provider_client,
            header_provider_label,
        },
    )
    .await?;
    let primary_agent_name = session_state
        .active_primary_agent
        .active()
        .display_name
        .clone();
    header_context.primary_agent = Some(primary_agent_name.clone());
    handle.set_primary_agent(Some(primary_agent_name));

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

#[cfg(test)]
mod tests;
