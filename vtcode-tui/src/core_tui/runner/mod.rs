use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::crossterm::{
    cursor::{MoveToColumn, RestorePosition, SavePosition},
    execute,
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use crate::config::types::UiSurfacePreference;
use crate::ui::tui::log::{clear_tui_log_sender, register_tui_log_sender, set_log_theme_name};

type EventCallback<E> = std::sync::Arc<dyn Fn(&E) + Send + Sync + 'static>;

pub trait TuiCommand {
    fn is_suspend_event_loop(&self) -> bool;
    fn is_resume_event_loop(&self) -> bool;
    fn is_clear_input_queue(&self) -> bool;
    fn is_force_redraw(&self) -> bool;
}

pub trait TuiSessionDriver {
    type Command: TuiCommand;
    type Event;

    fn handle_command(&mut self, command: Self::Command);
    #[allow(clippy::type_complexity)]
    fn handle_event(
        &mut self,
        event: crossterm::event::Event,
        events: &UnboundedSender<Self::Event>,
        callback: Option<&(dyn Fn(&Self::Event) + Send + Sync + 'static)>,
    );
    fn handle_tick(&mut self);
    fn render(&mut self, frame: &mut ratatui::Frame<'_>);
    fn take_redraw(&mut self) -> bool;
    fn use_steady_cursor(&self) -> bool;
    fn should_exit(&self) -> bool;
    fn request_exit(&mut self);
    fn mark_dirty(&mut self);
    fn update_terminal_title(&mut self);
    fn clear_terminal_title(&mut self);
    fn is_running_activity(&self) -> bool;
    fn has_status_spinner(&self) -> bool;
    fn thinking_spinner_active(&self) -> bool;
    fn has_active_navigation_ui(&self) -> bool;
    fn apply_coalesced_scroll(&mut self, line_delta: i32, page_delta: i32);
    fn set_show_logs(&mut self, show: bool);
    fn set_active_pty_sessions(
        &mut self,
        sessions: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    );
    fn set_workspace_root(&mut self, root: Option<std::path::PathBuf>);
    fn set_log_receiver(&mut self, receiver: UnboundedReceiver<crate::core_tui::log::LogEntry>);
}

impl TuiCommand for crate::core_tui::types::InlineCommand {
    fn is_suspend_event_loop(&self) -> bool {
        matches!(
            self,
            crate::core_tui::types::InlineCommand::SuspendEventLoop
        )
    }

    fn is_resume_event_loop(&self) -> bool {
        matches!(self, crate::core_tui::types::InlineCommand::ResumeEventLoop)
    }

    fn is_clear_input_queue(&self) -> bool {
        matches!(self, crate::core_tui::types::InlineCommand::ClearInputQueue)
    }

    fn is_force_redraw(&self) -> bool {
        matches!(self, crate::core_tui::types::InlineCommand::ForceRedraw)
    }
}

use super::types::FocusChangeCallback;

mod drive;
mod events;
mod signal;
mod surface;
mod terminal_io;
mod terminal_modes;

use drive::{DriveRuntimeOptions, drive_terminal};
use events::{EventListener, spawn_event_loop};
use signal::SignalCleanupGuard;
use surface::TerminalSurface;
use terminal_io::{drain_terminal_events, finalize_terminal, prepare_terminal};
use terminal_modes::{enable_terminal_modes, restore_terminal_modes};

const ALTERNATE_SCREEN_ERROR: &str = "failed to enter alternate inline screen";

pub struct TuiOptions<E> {
    pub surface_preference: UiSurfacePreference,
    pub inline_rows: u16,
    pub show_logs: bool,
    pub log_theme: Option<String>,
    pub event_callback: Option<EventCallback<E>>,
    pub focus_callback: Option<FocusChangeCallback>,
    pub active_pty_sessions: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    pub input_activity_counter: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    pub keyboard_protocol: crate::config::KeyboardProtocolConfig,
    pub workspace_root: Option<std::path::PathBuf>,
}

pub async fn run_tui<S, F>(
    mut commands: UnboundedReceiver<S::Command>,
    events: UnboundedSender<S::Event>,
    options: TuiOptions<S::Event>,
    make_session: F,
) -> Result<()>
where
    S: TuiSessionDriver,
    F: FnOnce(u16) -> S,
{
    // Create a guard to mark TUI as initialized during the session
    // This ensures the panic hook knows to restore terminal state
    let _panic_guard = crate::ui::tui::panic_hook::TuiPanicGuard::new();

    let _signal_guard = SignalCleanupGuard::new()?;

    let surface = TerminalSurface::detect(options.surface_preference, options.inline_rows)?;
    set_log_theme_name(options.log_theme.clone());
    let mut session = make_session(surface.rows());
    session.set_show_logs(options.show_logs);
    session.set_active_pty_sessions(options.active_pty_sessions);
    session.set_workspace_root(options.workspace_root.clone());
    if options.show_logs {
        let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
        session.set_log_receiver(log_rx);
        register_tui_log_sender(log_tx);
    } else {
        clear_tui_log_sender();
    }

    let keyboard_flags = crate::config::keyboard_protocol_to_flags(&options.keyboard_protocol);
    let mut stderr = io::stderr();
    let cursor_position_saved = match execute!(stderr, SavePosition) {
        Ok(_) => true,
        Err(error) => {
            tracing::debug!(%error, "failed to save cursor position for inline session");
            false
        }
    };
    let mode_state = enable_terminal_modes(&mut stderr, keyboard_flags)?;
    if surface.use_alternate() {
        execute!(stderr, EnterAlternateScreen).context(ALTERNATE_SCREEN_ERROR)?;
    }

    session.update_terminal_title();

    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend).context("failed to initialize inline terminal")?;
    prepare_terminal(&mut terminal)?;

    // Create event listener and channels using the new async pattern
    let (mut input_listener, event_channels) = EventListener::new();
    let cancellation_token = CancellationToken::new();
    let event_loop_token = cancellation_token.clone();
    let event_channels_for_loop = event_channels.clone();
    let rx_paused = event_channels.rx_paused.clone();
    let last_input_elapsed_ms = event_channels.last_input_elapsed_ms.clone();
    let session_start = event_channels.session_start;

    // Ensure any capability or resize responses emitted during terminal setup are not treated as
    // the user's first keystrokes.
    drain_terminal_events();

    // Spawn the async event loop after the terminal is fully configured so the first keypress is
    // captured immediately (avoids cooked-mode buffering before raw mode is enabled).
    let event_loop_handle = tokio::spawn(async move {
        spawn_event_loop(
            event_channels_for_loop.tx.clone(),
            event_loop_token,
            rx_paused,
            last_input_elapsed_ms,
            session_start,
        )
        .await;
    });

    let drive_result = drive_terminal(
        &mut terminal,
        &mut session,
        &mut commands,
        &events,
        &mut input_listener,
        event_channels,
        DriveRuntimeOptions {
            event_callback: options.event_callback,
            focus_callback: options.focus_callback,
            use_alternate_screen: surface.use_alternate(),
            input_activity_counter: options.input_activity_counter,
            keyboard_flags,
        },
    )
    .await;

    // Gracefully shutdown the event loop
    cancellation_token.cancel();
    let _ = tokio::time::timeout(Duration::from_millis(100), event_loop_handle).await;

    // Drain any pending events before finalizing terminal and disabling modes
    drain_terminal_events();

    // Clear current line to remove any echoed characters (like ^C)
    let _ = execute!(io::stderr(), MoveToColumn(0), Clear(ClearType::CurrentLine));

    let finalize_result = finalize_terminal(&mut terminal);
    let leave_alternate_result = if surface.use_alternate() {
        Some(execute!(terminal.backend_mut(), LeaveAlternateScreen))
    } else {
        None
    };

    if let Some(result) = leave_alternate_result
        && let Err(error) = result
    {
        tracing::warn!(%error, "failed to leave alternate screen");
    }

    // Restore terminal modes (handles all modes including raw mode)
    let restore_modes_result = restore_terminal_modes(&mode_state);
    if let Err(error) = restore_modes_result {
        tracing::warn!(%error, "failed to restore terminal modes");
    }

    // Clear terminal title on exit.
    session.clear_terminal_title();

    if cursor_position_saved && let Err(error) = execute!(io::stderr(), RestorePosition) {
        tracing::debug!(%error, "failed to restore cursor position for inline session");
    }

    drive_result?;
    finalize_result?;

    clear_tui_log_sender();

    Ok(())
}
