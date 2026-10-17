//! Static-first typeable shell spawn (blog static-composer pattern).
//!
//! `initialize_session_shell` paints a typeable TUI **before** ToolRegistry,
//! subagent discovery, provider construction, or resume-history load. Those
//! heavy builders run afterward in `initialize_session_critical`; the ready
//! UI is wired in `initialize_session_ui` without respawning the session so
//! keystrokes typed into the shell survive the handoff.
//!
//! Ratchet: `tests` in this module assert this file never reintroduces
//! registry/discovery/provider construction.

use hashbrown::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize};

use anyhow::{Context, Result};
use tokio::sync::Notify;
use vtcode_config::root::ColorSchemeMode;
use vtcode_core::config::constants::app as app_constants;
use vtcode_core::config::constants::ui as ui_constants;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::notifications::set_global_terminal_focused;
use vtcode_core::tools::exec_session::ExecSessionManager;
use vtcode_core::ui::slash::visible_commands;
use vtcode_core::ui::theme;
use vtcode_core::ui::{
    inline_theme_from_core_styles, is_tui_mode, set_tui_mode, to_tui_appearance, to_tui_fullscreen,
    to_tui_keyboard_protocol, to_tui_slash_commands, to_tui_surface,
};
use vtcode_core::utils::dot_config::take_startup_user_config;
use vtcode_ui::tui::app::{
    FocusChangeCallback, InlineEvent, InlineEventCallback, InlineHandle, InlineListSelection, InlineSession,
    PreviewCallback, SessionOptions, spawn_session_with_options,
};

use super::EditorOpenDispatcher;
use super::ui::SessionUiLaunchOptions;

/// Shared handle to the live `ExecSessionManager`.
///
/// Filled after `initialize_session_critical` so the shell paint path never
/// constructs a tool registry. Until then background-shortcut events no-op.
pub(crate) type SharedExecSessions = Arc<std::sync::OnceLock<ExecSessionManager>>;

/// Typeable TUI shell painted before heavy session init.
pub(crate) struct SessionShell {
    pub session: InlineSession,
    pub handle: InlineHandle,
    pub ctrl_c_state: Arc<crate::agent::runloop::unified::state::CtrlCState>,
    pub ctrl_c_notify: Arc<Notify>,
    pub input_activity_counter: Arc<AtomicU64>,
    pub pty_counter: Arc<AtomicUsize>,
    pub settings_event_receiver: tokio::sync::mpsc::UnboundedReceiver<InlineEvent>,
    pub settings_sender:
        tokio::sync::mpsc::UnboundedSender<crate::agent::runloop::unified::session_settings::SessionSettingsControl>,
    pub editor_open_dispatcher: Arc<EditorOpenDispatcher>,
    pub exec_sessions: SharedExecSessions,
    pub default_placeholder: Option<String>,
    pub skip_confirmations: bool,
    pub legacy_key_bindings: HashMap<String, Vec<String>>,
}

/// Paint a typeable shell with bootstrap chrome only.
///
/// Performs no `ToolRegistry` construction, subagent discovery, provider
/// construction, or resume-history load. Built-in slash commands only;
/// workspace templates merge after spawn from `initialize_session_ui`.
pub(crate) async fn initialize_session_shell(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    options: SessionUiLaunchOptions,
) -> Result<SessionShell> {
    let shell_phase = vtcode_commons::startup_trace::phase_started();
    let SessionUiLaunchOptions {
        session_archive: _,
        full_auto: _,
        skip_confirmations,
        steering_sender,
        settings_sender,
    } = options;

    let active_styles = theme::active_styles();
    let theme_spec = inline_theme_from_core_styles(&active_styles);
    // Paint the final placeholder on the shell frame: the configured
    // onboarding placeholder (in-memory derivation, no I/O) or the shared
    // bootstrap hint. `initialize_session_ui` re-derives the same value, so
    // the ready re-drive never visibly swaps the placeholder.
    let default_placeholder = crate::agent::runloop::welcome::configured_chat_placeholder(vt_cfg)
        .or_else(|| Some(ui_constants::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.to_string()));
    let inline_rows = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.inline_viewport_rows)
        .unwrap_or(ui_constants::DEFAULT_INLINE_VIEWPORT_ROWS);

    if !is_tui_mode() {
        set_tui_mode(true);
    }

    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let input_activity_counter = Arc::new(AtomicU64::new(0));
    let pty_counter = Arc::new(AtomicUsize::new(0));
    let editor_open_dispatcher = Arc::new(EditorOpenDispatcher::new(super::ui::immediate_file_open_allowed(vt_cfg)));
    let (settings_event_sender, settings_event_receiver) = tokio::sync::mpsc::unbounded_channel();

    let exec_sessions: SharedExecSessions = Arc::new(std::sync::OnceLock::new());
    let interrupt_callback = build_session_event_callback(
        ctrl_c_state.clone(),
        ctrl_c_notify.clone(),
        steering_sender.clone(),
        settings_event_sender.clone(),
        editor_open_dispatcher.clone(),
        config.workspace.clone(),
        exec_sessions.clone(),
    );
    let focus_callback: FocusChangeCallback = Arc::new(set_global_terminal_focused);

    let visible_slash_commands: Vec<_> = visible_commands().into_iter().copied().collect();
    let slash_command_items = to_tui_slash_commands(visible_slash_commands.as_slice());

    // In-memory snapshot only: never block first paint on dot-config disk I/O.
    // Embedded callers without a CLI bootstrap get empty legacy bindings here;
    // `initialize_session_ui` may load the fallback after the shell is painted.
    let user_dot_config = take_startup_user_config();
    let legacy_key_bindings: HashMap<String, Vec<String>> = match user_dot_config {
        Some(dot) => dot
            .preferences
            .keybindings
            .into_iter()
            .map(|(action, key)| (action, vec![key]))
            .collect(),
        None => HashMap::new(),
    };
    let mut user_key_bindings = legacy_key_bindings.clone();
    if let Some(vt_config) = vt_cfg {
        user_key_bindings.extend(vt_config.ui.keybindings.clone());
    }

    let preview_callback: PreviewCallback = Arc::new(move |selection| match selection {
        Some(InlineListSelection::Theme(theme_id)) => theme::set_preview_theme(theme_id),
        None => {
            theme::clear_preview_theme();
            Ok(())
        }
        Some(_) => Ok(()),
    });

    // Await the palette probe before the TUI owns the TTY. The probe is
    // bounded (~50 ms read + 40 ms settle + 100 ms drain on failure), and it
    // already overlapped with startup-context resolution, so this usually
    // returns immediately. Spawning the event loop concurrently lets late
    // `OSC 10/11/4` replies win the `/dev/tty` read race and leak as
    // `10;rgb:...` keystrokes; the vendored `parse_osc` backstop swallows
    // stragglers from slow terminals. `note_crossterm_raw_mode` stays as a
    // guard so a late `RawModeGuard` restore cannot undo crossterm raw mode.
    crate::agent::probe::await_terminal_palette_probe().await;
    vtcode_core::utils::terminal_color_probe::note_crossterm_raw_mode();

    let mut session = spawn_session_with_options(
        theme_spec.clone(),
        SessionOptions {
            placeholder: default_placeholder.clone(),
            surface_preference: vt_cfg
                .and_then(|cfg| cfg.tui.alternate_screen)
                .map(|mode| match mode {
                    vtcode_core::config::TuiAlternateScreen::Always => vtcode_ui::tui::app::SessionSurface::Alternate,
                    vtcode_core::config::TuiAlternateScreen::Never
                    | vtcode_core::config::TuiAlternateScreen::Unknown => vtcode_ui::tui::app::SessionSurface::Inline,
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
            app_name: app_constants::DISPLAY_NAME.to_string(),
            non_interactive_hint: Some("Use `vtcode ask \"your prompt\"` for non-interactive input.".to_string()),
            key_bindings: user_key_bindings,
            preview_callback: Some(preview_callback),
        },
    )
    .context("failed to launch typeable shell session")?;

    set_global_terminal_focused(true);
    if skip_confirmations {
        session.set_skip_confirmations(true);
    }

    let handle = session.clone_inline_handle();
    let color_scheme_auto = vt_cfg
        .map(|cfg| matches!(cfg.ui.color_scheme_mode, ColorSchemeMode::Auto | ColorSchemeMode::Unknown))
        .unwrap_or(true);
    session.set_color_scheme_auto(color_scheme_auto);
    handle.program_status(vtcode_commons::program_status::ProgramStatusUpdate::Configure {
        enabled: vt_cfg.is_some_and(|cfg| cfg.ui.program_status.enabled),
    });

    vtcode_commons::startup_trace::record_phase("session_setup_shell", shell_phase);

    Ok(SessionShell {
        session,
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
    })
}

pub(crate) fn build_session_event_callback(
    state: Arc<crate::agent::runloop::unified::state::CtrlCState>,
    notify: Arc<Notify>,
    steering_sender: Option<tokio::sync::mpsc::UnboundedSender<SteeringMessage>>,
    settings_events: tokio::sync::mpsc::UnboundedSender<InlineEvent>,
    editor_open: Arc<EditorOpenDispatcher>,
    editor_workspace: std::path::PathBuf,
    exec_sessions: SharedExecSessions,
) -> InlineEventCallback {
    Arc::new(move |event: &InlineEvent| match event {
        InlineEvent::OpenFileInEditor(path) => {
            editor_open.try_forward_immediate(path, &editor_workspace);
        }
        InlineEvent::Interrupt => {
            crate::agent::runloop::unified::stop_requests::request_local_cancel(&state, &notify);
        }
        InlineEvent::Exit => {
            crate::agent::runloop::unified::stop_requests::request_local_exit(&state, &notify);
        }
        InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) if state.is_cancel_handled() => {
            state.reset();
            notify.notify_one();
        }
        InlineEvent::BackgroundOperation => {
            // Registry-backed manager is attached after critical init; until
            // then the shell is typeable but tools are not live yet.
            if let Some(exec_sessions) = exec_sessions.get() {
                let _ = exec_sessions.request_foreground_background();
            }
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
        InlineEvent::Steer(input) => {
            if matches!(input.text.split_whitespace().next(), Some("/model")) {
                let _ = settings_events.send(event.clone());
                return;
            }
            // Hand text-only steers to the live steering channel for mid-turn
            // injection. When the channel is unavailable (no sender yet, or
            // closed) leave the delivery latch clear so the runloop Steer
            // handler queues the message for the next ready boundary instead
            // of dropping it.
            if !input.has_attachments()
                && let Some(sender) = steering_sender.as_ref()
                && sender.send(SteeringMessage::FollowUpInput(input.text.clone())).is_ok()
            {
                state.mark_steer_delivered();
            }
        }
        InlineEvent::Transient(
            vtcode_ui::tui::app::TransientEvent::Submitted(_) | vtcode_ui::tui::app::TransientEvent::Cancelled,
        ) => {
            let _ = settings_events.send(event.clone());
        }
        _ => {}
    })
}

#[cfg(test)]
mod tests {

    /// Structural ratchet (blog "anything you can count"): the shell paint
    /// path must never reintroduce heavy init before first paint.
    #[test]
    fn shell_module_avoids_heavy_init_before_paint() {
        // Scan production code only: the ratchet list itself mentions the
        // forbidden symbols, so strip the tests module before matching.
        let src = include_str!("shell.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        for forbidden in [
            "ToolRegistry::new",
            "ToolRegistry::new_for_first_paint",
            "discover_controller_subagents",
            "create_provider_client",
            "build_conversation_history_from_resume",
            "hydrate_session_runtime",
            "load_user_config",
        ] {
            assert!(
                !production.contains(forbidden),
                "initialize_session_shell must not call {before} before first paint",
                before = forbidden
            );
        }
        assert!(production.contains("spawn_session_with_options"), "shell module must own the typeable spawn");
    }
}
