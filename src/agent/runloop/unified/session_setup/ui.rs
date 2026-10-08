//! Agent Legibility:
//! - Entrypoint: `initialize_session_ui` owns session bootstrap, inline header wiring, and TUI session launch.
//! - Common changes:
//!   - Local-agent sidebar refresh and preview logic live in `ui/local_agents.rs`.
//!   - Header assembly, OpenAI notices, and IDE snapshot wiring live in `ui/header_context.rs`.
//!   - Persistent-memory guide and header badges live in `ui/persistent_memory.rs`.
//!   - Resume rendering and transcript projection live in `ui/resume_render.rs`.
//! - Constraints: TD-005 is active for this surface; keep this file as an orchestration root and prefer responsibility-named support modules for new helper clusters.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode inline_events::tests`

mod active_settings;
mod header_context;
mod local_agents;
mod persistent_memory;
mod resume_render;

use super::hook_approval;
use super::shell::SessionShell;
use super::types::{BackgroundTaskGuard, SessionState, SessionUISetup};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::unified::context_manager;
use crate::agent::runloop::unified::reasoning::{model_supports_reasoning, resolve_reasoning_visibility};
use crate::agent::runloop::unified::session_setup::spawn_editor_open_coordinator;
use crate::agent::runloop::unified::turn::utils::{append_additional_context, render_hook_messages};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::warn;
use vtcode_core::config::constants::ui;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
use vtcode_core::notifications::{set_global_notification_hook_engine, set_global_terminal_focused};
use vtcode_core::primary_agent::build_primary_agent_hook_config;
use vtcode_core::prompts::discover_prompt_templates;
use vtcode_core::subagents::SubagentController;
use vtcode_core::tools::exec_session::ExecSessionManager;
use vtcode_core::tools::terminal_app::TerminalAppLauncher;
use vtcode_core::ui::slash::visible_commands;
use vtcode_core::ui::{is_tui_mode, set_tui_mode, to_tui_slash_commands};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_core::utils::transcript;
use vtcode_ui::tui::app::{AgentPaletteItem, InlineHandle, SlashCommandItem};

use self::header_context::{HeaderContextInit, initialize_header_context, maybe_render_system_prompt_budget_warning};
pub(crate) use self::local_agents::refresh_local_agents;
use self::resume_render::render_resume_state_if_present;
pub(crate) use self::resume_render::{build_structured_resume_lines, render_resume_lines};

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
use self::resume_render::infer_legacy_line_style;
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
    pub settings_sender: UnboundedSender<crate::agent::runloop::unified::session_settings::SessionSettingsControl>,
}

/// Whether transcript file links may open while an agent turn is active.
///
/// GUI editors launch detached without touching the TUI event loop, so they
/// are safe to open mid-turn. Terminal editors suspend the event loop and
/// take over the terminal, which would contend with the running turn's
/// rendering — those stay on the deferred idle-loop drain (previous
/// behavior). The coordinator backend re-resolves this per open; this
/// snapshot only gates timing, so a mid-session editor reconfiguration at
/// worst affects immediacy, never correctness.
pub(super) fn immediate_file_open_allowed(vt_cfg: Option<&VTCodeConfig>) -> bool {
    let Some(editor_config) = vt_cfg.map(|cfg| cfg.tools.editor.clone()) else {
        return true;
    };
    let preferred_editor =
        (!editor_config.preferred_editor.trim().is_empty()).then(|| editor_config.preferred_editor.clone());
    !(editor_config.suspend_tui && TerminalAppLauncher::editor_command_requires_terminal(preferred_editor.as_deref()))
}

pub(crate) async fn initialize_session_ui(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    session_id: &str,
    session_state: &mut SessionState,
    session_trigger: SessionStartTrigger,
    resume_state: Option<&ResumeSession>,
    shell: SessionShell,
    exit_context: crate::agent::runloop::unified::stop_requests::InitializationExitContext<'_>,
    options: SessionUiLaunchOptions,
) -> Result<Option<SessionUISetup>> {
    let SessionUiLaunchOptions {
        session_archive,
        full_auto,
        skip_confirmations: _,
        steering_sender: _,
        settings_sender: _,
    } = options;
    let SessionShell {
        mut session,
        handle,
        ctrl_c_state,
        ctrl_c_notify,
        input_activity_counter,
        pty_counter,
        settings_event_receiver,
        settings_sender,
        editor_open_dispatcher,
        exec_sessions,
        default_placeholder,
        skip_confirmations,
        legacy_key_bindings,
    } = shell;
    macro_rules! ui_wait {
        ($future:expr) => {{
            match crate::agent::runloop::unified::stop_requests::await_initialization(
                &ctrl_c_state,
                &ctrl_c_notify,
                $future,
            )
            .await
            {
                Some(value) => value,
                None => {
                    crate::agent::runloop::unified::stop_requests::finish_initialization_exit(
                        &handle,
                        &mut session,
                        &ctrl_c_state,
                        exit_context,
                    )
                    .await;
                    return Ok(None);
                }
            }
        }};
    }
    let initialization_progress = handle.begin_progress(vtcode_commons::ui_protocol::ProgressPhase::Initializing);
    session_state.session_bootstrap.legacy_key_bindings = legacy_key_bindings;
    // Embedded callers without the CLI bootstrap snapshot: load dot-config
    // after the shell is painted so first paint never waits on disk I/O.
    if session_state.session_bootstrap.legacy_key_bindings.is_empty()
        && let Some(dot) = ui_wait!(vtcode_core::utils::dot_config::load_user_config()).ok()
    {
        session_state.session_bootstrap.legacy_key_bindings = dot
            .preferences
            .keybindings
            .into_iter()
            .map(|(action, key)| (action, vec![key]))
            .collect();
    }
    // Registry is late-filled (`complete_session_registry` runs after this
    // function). Bind exec sessions / pty counter when it already exists;
    // otherwise `apply_post_hydration_ui` re-drives after the completer.
    if let Some(tool_registry) = session_state.tool_registry.as_ref() {
        let _ = exec_sessions.set(tool_registry.exec_session_manager());
        tool_registry.set_active_pty_sessions(pty_counter.clone());
    }

    let lifecycle_hooks = if let Some(vt) = vt_cfg {
        let hooks = build_primary_agent_hook_config(&vt.hooks, session_state.active_primary_agent.active());
        let workspace_gated = vt.workspace_lifecycle_hooks.as_ref().is_some_and(|hooks| !hooks.is_empty())
            || session_state
                .active_primary_agent
                .active()
                .contributes_workspace_controlled_hooks();
        LifecycleHookEngine::new_with_session_gated(
            config.workspace.clone(),
            &hooks,
            session_trigger,
            session_id,
            workspace_gated,
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

    let default_placeholder = session_state.session_bootstrap.placeholder.clone().or(default_placeholder);
    let follow_up_placeholder = if session_state.session_bootstrap.placeholder.is_none() {
        Some(ui::CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.to_string())
    } else {
        None
    };
    if let Some(vt_cfg) = vt_cfg {
        crate::startup::defer_system_prompt_size_check(vt_cfg, &config.workspace);
    }

    if !is_tui_mode() {
        set_tui_mode(true);
    }

    // Typeable shell is already painted (static-first). Bind the registry
    // exec manager and continue wiring session state into the live session.
    // Drain the palette probe after spawn so a silent terminal cannot delay
    // the first frame. Theme/palette settle here before the first model turn.
    ui_wait!(crate::agent::probe::await_terminal_palette_probe());

    set_global_terminal_focused(true);
    if skip_confirmations {
        session.set_skip_confirmations(true);
    }

    let settings_task_guard = BackgroundTaskGuard::new(tokio::spawn(active_settings::run(
        settings_event_receiver,
        settings_sender,
        handle.clone(),
        config.clone(),
        vt_cfg.cloned(),
        ctrl_c_state.clone(),
        ctrl_c_notify.clone(),
    )));
    // Merge prompt-template slash commands without blocking first paint.
    // The session spawns with built-ins; this one-shot task appends workspace
    // templates via `SetSlashCommands` before the user can open the palette.
    {
        let handle_for_templates = handle.clone();
        let workspace_for_templates = config.workspace.clone();
        let visible: Vec<_> = visible_commands().into_iter().copied().collect();
        let builtin_items = to_tui_slash_commands(visible.as_slice());
        tokio::spawn(async move {
            let discovered = discover_prompt_templates(&workspace_for_templates).await;
            if discovered.is_empty() {
                return;
            }
            let mut merged = builtin_items;
            let visible: Vec<_> = visible_commands().into_iter().copied().collect();
            merged.extend(
                discovered
                    .into_iter()
                    .filter(|template| !visible.iter().any(|cmd| cmd.name == template.name))
                    .map(|template| SlashCommandItem::new(template.name, template.description)),
            );
            handle_for_templates.set_slash_commands(merged);
        });
    }
    // Follow live terminal light/dark reports only when the user opted into
    // automatic color-scheme detection; forced Light/Dark modes must win.
    let color_scheme_auto = vt_cfg
        .map(|cfg| {
            matches!(
                cfg.ui.color_scheme_mode,
                vtcode_config::root::ColorSchemeMode::Auto | vtcode_config::root::ColorSchemeMode::Unknown
            )
        })
        .unwrap_or(true);
    session.set_color_scheme_auto(color_scheme_auto);
    handle.program_status(vtcode_commons::program_status::ProgramStatusUpdate::Configure {
        enabled: vt_cfg.is_some_and(|cfg| cfg.ui.program_status.enabled),
    });
    let (editor_open_sender, editor_open_coordinator_task_guard) =
        spawn_editor_open_coordinator(config.workspace.clone(), &handle);
    editor_open_dispatcher.set_sender(editor_open_sender.clone());
    let highlight_config = vt_cfg.as_ref().map(|cfg| cfg.syntax_highlighting.clone()).unwrap_or_default();

    transcript::set_inline_handle(Arc::new(handle.clone()));
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);
    let supports_reasoning = model_supports_reasoning(&*session_state.provider_client, &config.model);
    renderer.set_reasoning_visible(resolve_reasoning_visibility(vt_cfg, supports_reasoning));
    if let Some(cfg) = vt_cfg {
        renderer.set_screen_reader_mode(cfg.ui.screen_reader_mode);
        renderer.set_show_diagnostics_in_transcript(cfg.ui.show_diagnostics_in_transcript);
        renderer.set_tool_display_mode(cfg.ui.tool_display_mode);
        renderer.set_diff_preview_mode(cfg.ui.diff_preview_mode);
    }
    let workspace_for_palette = config.workspace.clone();
    let handle_for_palette = handle.clone();
    // Configure immediately with a shallow directory lister so the root is
    // usable at once; the full recursive walk for Search mode is deferred to a
    // background task below.
    handle_for_palette.configure_file_palette(
        workspace_for_palette.clone(),
        vtcode_ui::tui::core_tui::app::session::file_palette::DirLister::new({
            // Build the indexer once: it is stateless for `discover_dir_entries`,
            // and constructing it per directory would rebuild the excluded-dir
            // config on every palette navigation.
            let indexer = vtcode_core::SimpleIndexer::new(workspace_for_palette.clone());
            move |dir| {
                // Symlink target and kind metadata are captured once per child
                // here, so the render path never touches the filesystem.
                indexer
                    .discover_dir_entries(dir)
                    .into_iter()
                    .map(|(path, is_dir)| {
                        vtcode_ui::tui::core_tui::app::session::file_palette::DirEntryInfo::from_path(path, is_dir)
                    })
                    .collect()
            }
        }),
    );
    let workspace_for_search = config.workspace.clone();
    let handle_for_search = handle.clone();
    let file_palette_task_guard = BackgroundTaskGuard::new(tokio::spawn(async move {
        match tokio::task::spawn_blocking(move || {
            vtcode_core::SimpleIndexer::new(workspace_for_search.clone()).discover_files(&workspace_for_search)
        })
        .await
        {
            Ok(files) => {
                if files.is_empty() {
                    tracing::debug!("No files found in workspace for file palette");
                }
                // Deliver even an empty result so the picker can distinguish a
                // finished (empty/fully-ignored) workspace from still indexing.
                handle_for_search.set_file_palette_search_index(files);
            }
            Err(err) => {
                tracing::warn!("Failed to load workspace files for file palette: {}", err);
            }
        }
    }));
    let tool_registry = session_state.tool_registry.as_ref();
    let controller = tool_registry.and_then(|r| r.subagent_controller());
    let exec_manager = tool_registry.map(|r| r.exec_session_manager());
    let background_subprocess_task_guard = exec_manager
        .map(|exec_manager| spawn_agent_palette_and_background_refresh(&handle, controller, exec_manager, vt_cfg));

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

    let mut checkpoint_config = vtcode_core::core::agent::snapshots::SnapshotConfig::new(config.workspace.clone());
    checkpoint_config.enabled = config.checkpointing_enabled;
    checkpoint_config.storage_dir = config.checkpointing_storage_dir.clone();
    checkpoint_config.max_snapshots = config.checkpointing_max_snapshots;
    checkpoint_config.max_age_days = config.checkpointing_max_age_days;
    let checkpoint_manager = match ui_wait!(tokio::task::spawn_blocking(move || {
        vtcode_core::core::agent::snapshots::SnapshotManager::new(checkpoint_config)
    })) {
        Ok(Ok(manager)) => Some(manager),
        Ok(Err(err)) => {
            warn!("Failed to initialize checkpoint manager: {}", err);
            None
        }
        Err(error) => {
            warn!(%error, "Checkpoint initialization worker failed");
            None
        }
    };

    if let (Some(hooks), Some(archive)) = (&lifecycle_hooks, session_archive.as_ref()) {
        ui_wait!(hooks.update_transcript_path(Some(archive.path().to_path_buf())));
    }

    // Workspace-controlled lifecycle hooks never execute without explicit
    // approval of the exact command set for this workspace (GHSA-wqgw-crr5-cr2p).
    // A persisted approval matching the current command-set digest is restored
    // silently; otherwise interactive sessions prompt, and auto/non-interactive
    // sessions fail closed by skipping the lifecycle hooks.
    if let Some(hooks) = &lifecycle_hooks {
        // Approval / skip notices stay on the first-frame path. Executing
        // session-start hooks waits until after hydration so they observe the
        // fully initialized tool registry (`run_session_start_hooks`).
        if ui_wait!(hooks.workspace_hooks_need_approval()) {
            let digest = hooks.command_digest().to_string();
            let pre_approved = matches!(
                ui_wait!(vtcode_core::load_lifecycle_hook_approval(&config.workspace)),
                Ok(Some(record)) if record.config_digest == digest
            );
            if pre_approved {
                ui_wait!(hooks.approve_workspace_hooks());
            } else if full_auto || skip_confirmations {
                renderer.line(
                    MessageStyle::Warning,
                    "Workspace lifecycle hooks require approval and were skipped (auto/non-interactive mode). \
                     Run an interactive session to review and approve them.",
                )?;
            } else {
                match hook_approval::prompt_workspace_hook_approval(
                    &handle,
                    &mut session,
                    &ctrl_c_state,
                    &ctrl_c_notify,
                    &config.workspace,
                    &hooks.command_previews(),
                )
                .await
                {
                    Ok(hook_approval::HookApprovalDecision::Approved) => {
                        if let Err(err) =
                            ui_wait!(vtcode_core::update_lifecycle_hook_approval(&config.workspace, digest))
                        {
                            tracing::warn!(
                                error = %err,
                                "Failed to persist workspace lifecycle hook approval; approval applies to this session only"
                            );
                        }
                        ui_wait!(hooks.approve_workspace_hooks());
                    }
                    Ok(hook_approval::HookApprovalDecision::Denied) => {
                        renderer.line(
                            MessageStyle::Warning,
                            "Workspace lifecycle hooks were not approved and will be skipped.",
                        )?;
                    }
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Warning,
                            &format!("Could not prompt for workspace lifecycle hook approval; skipping them: {err}"),
                        )?;
                    }
                }
            }
        }
    }

    render_full_auto_allowlist_banner(&mut renderer, full_auto, session_state.full_auto_allowlist.as_ref())?;

    handle.set_placeholder(default_placeholder.clone());

    let mut header_context = ui_wait!(initialize_header_context(
        &mut renderer,
        &handle,
        HeaderContextInit {
            config,
            vt_cfg,
            session_bootstrap: &session_state.session_bootstrap,
            provider_client: &*session_state.provider_client,
            header_provider_label,
        },
    ))?;
    let primary_agent_name = session_state.active_primary_agent.active().display_name.clone();
    let primary_agent_color = session_state
        .active_primary_agent
        .active()
        .color
        .clone()
        .filter(|c| !c.trim().is_empty());
    header_context.primary_agent = Some(primary_agent_name.clone());
    header_context.primary_agent_color = primary_agent_color.clone();
    handle.set_primary_agent(Some(primary_agent_name), primary_agent_color);

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
    } else if session_state.startup_update_check.cached_notice.is_none() {
        // The preflight check may have completed after load_startup_update_check()
        // read the cache.  Re-check the static notice and, if an update arrived
        // in the meantime, deliver it through a channel so the TUI can display it.
        if let Some(notice) = crate::updater::get_preflight_notice() {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let _ = tx.send(notice);
            startup_update_notice_rx = Some(rx);
        }
    }

    let next_checkpoint_turn = if let Some(manager) = checkpoint_manager.as_ref() {
        let manager = manager.clone();
        match ui_wait!(tokio::task::spawn_blocking(move || manager.next_turn_number())) {
            Ok(Ok(turn)) => turn,
            Ok(Err(error)) => {
                warn!(%error, "Failed to inspect checkpoint turn numbers");
                1
            }
            Err(error) => {
                warn!(%error, "Checkpoint enumeration worker failed");
                1
            }
        }
    } else {
        1
    };

    ui_wait!(async {});
    initialization_progress.transfer();
    Ok(Some(SessionUISetup {
        settings_task_guard,
        renderer,
        session,
        handle,
        header_context,
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
        editor_open_sender,
        editor_open_dispatcher,
        editor_open_coordinator_task_guard,
        exec_sessions: Some(exec_sessions),
        pty_counter: Some(pty_counter),
    }))
}

/// Shared agent-palette + background-refresh wiring used both when a
/// controller already exists at UI spawn and when it appears during hydration.
fn spawn_agent_palette_and_background_refresh(
    handle: &InlineHandle,
    controller: Option<Arc<SubagentController>>,
    exec_sessions: ExecSessionManager,
    vt_cfg: Option<&VTCodeConfig>,
) -> BackgroundTaskGuard {
    let handle_for_agents = handle.clone();
    let controller_for_agents = controller.clone();
    if let Some(controller_for_agents) = controller_for_agents {
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
    }

    let handle_for_subprocesses = handle.clone();
    let controller_for_subprocesses = controller;
    let exec_sessions_for_subprocesses = exec_sessions;
    let refresh_interval_ms = vt_cfg
        .map(|cfg| cfg.subagents.background.refresh_interval_ms)
        .unwrap_or(2_000)
        .max(250);
    BackgroundTaskGuard::new(tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(refresh_interval_ms));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            if let Err(err) = refresh_local_agents(
                &handle_for_subprocesses,
                controller_for_subprocesses.as_ref(),
                exec_sessions_for_subprocesses.clone(),
            )
            .await
            {
                tracing::warn!("Failed to refresh background subprocesses: {}", err);
            }
        }
    }))
}

fn render_full_auto_allowlist_banner(
    renderer: &mut AnsiRenderer,
    full_auto: bool,
    allowlist: Option<&Vec<String>>,
) -> Result<()> {
    if !full_auto {
        return Ok(());
    }
    let Some(allowlist) = allowlist else {
        return Ok(());
    };
    if allowlist.is_empty() {
        renderer.line(
            MessageStyle::Info,
            "Full-auto permission review enabled with no execution tool permissions; only workflow-coordination tools (task_tracker, start_planning, request_user_input) stay available.",
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Full-auto permission review enabled. Permitted tools: {} (plus workflow-coordination tools: task_tracker, start_planning, request_user_input).",
                allowlist.join(", ")
            ),
        )?;
    }
    Ok(())
}

/// Execute session-start lifecycle hooks after hydration so they observe a
/// fully initialized tool registry.
pub(crate) async fn run_session_start_hooks(
    lifecycle_hooks: &Option<LifecycleHookEngine>,
    renderer: &mut AnsiRenderer,
    session_state: &mut SessionState,
) -> Result<()> {
    let Some(hooks) = lifecycle_hooks.as_ref() else {
        return Ok(());
    };
    match hooks.run_session_start().await {
        Ok(outcome) => {
            render_hook_messages(renderer, &outcome.messages)?;
            append_additional_context(&mut session_state.conversation_history, outcome.additional_context);
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("Failed to run session start hooks: {err}"))?;
        }
    }
    Ok(())
}

/// Re-drive UI surfaces that depend on fields completed in deferred session
/// hydration (agent palette, background refresh, primary-agent header,
/// full-auto banner, system-prompt budget warning).
///
/// Returns a background-refresh task guard when a subagent controller is
/// present after hydration.
pub(crate) fn apply_post_hydration_ui(
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    session_state: &SessionState,
    ui_setup: &mut SessionUISetup,
) -> Result<Option<BackgroundTaskGuard>> {
    let handle = ui_setup.handle.clone();

    let primary_agent_name = session_state.active_primary_agent.active().display_name.clone();
    let primary_agent_color = session_state
        .active_primary_agent
        .active()
        .color
        .clone()
        .filter(|c| !c.trim().is_empty());
    ui_setup.header_context.primary_agent = Some(primary_agent_name.clone());
    ui_setup.header_context.primary_agent_color = primary_agent_color.clone();
    handle.set_primary_agent(Some(primary_agent_name), primary_agent_color);

    render_full_auto_allowlist_banner(&mut ui_setup.renderer, full_auto, session_state.full_auto_allowlist.as_ref())?;
    maybe_render_system_prompt_budget_warning(&mut ui_setup.renderer, vt_cfg, &session_state.session_bootstrap)?;

    // Re-drive shell bindings that were skipped when the registry was still
    // late-filled at `initialize_session_ui` time.
    if let Some(tool_registry) = session_state.tool_registry.as_ref() {
        if let Some(exec_sessions) = ui_setup.exec_sessions.as_ref() {
            let _ = exec_sessions.set(tool_registry.exec_session_manager());
        }
        if let Some(pty_counter) = ui_setup.pty_counter.as_ref() {
            tool_registry.set_active_pty_sessions(pty_counter.clone());
        }
    }
    let background_subprocess_task_guard = session_state.tool_registry.as_ref().map(|tool_registry| {
        spawn_agent_palette_and_background_refresh(
            &handle,
            tool_registry.subagent_controller(),
            tool_registry.exec_session_manager(),
            vt_cfg,
        )
    });

    Ok(background_subprocess_task_guard)
}

#[cfg(test)]
mod tests;
