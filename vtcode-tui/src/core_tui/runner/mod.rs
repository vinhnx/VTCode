use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::crossterm::{execute, terminal::EnterAlternateScreen, terminal::LeaveAlternateScreen};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use crate::config::types::UiSurfacePreference;
use crate::ui::tui::log::{clear_tui_log_sender, register_tui_log_sender, set_log_theme_name};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineEventCallback, InlineTheme},
};

mod drive;
mod events;
mod signal;
mod surface;
mod terminal_io;
mod terminal_modes;

use drive::drive_terminal;
use events::{EventListener, spawn_event_loop};
use signal::SignalCleanupGuard;
use surface::TerminalSurface;
use terminal_io::{drain_terminal_events, finalize_terminal, prepare_terminal};
use terminal_modes::{enable_terminal_modes, restore_terminal_modes};

/// Terminal title displayed when VT Code TUI is active
const TERMINAL_TITLE: &str = "> VT Code";
const ALTERNATE_SCREEN_ERROR: &str = "failed to enter alternate inline screen";

pub struct TuiOptions {
    pub theme: InlineTheme,
    pub placeholder: Option<String>,
    pub surface_preference: UiSurfacePreference,
    pub inline_rows: u16,
    pub show_logs: bool,
    pub log_theme: Option<String>,
    pub event_callback: Option<InlineEventCallback>,
    pub active_pty_sessions: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    pub keyboard_protocol: crate::config::KeyboardProtocolConfig,
    pub workspace_root: Option<std::path::PathBuf>,
}

pub async fn run_tui(
    mut commands: UnboundedReceiver<InlineCommand>,
    events: UnboundedSender<InlineEvent>,
    options: TuiOptions,
) -> Result<()> {
    // Create a guard to mark TUI as initialized during the session
    // This ensures the panic hook knows to restore terminal state
    let _panic_guard = crate::ui::tui::panic_hook::TuiPanicGuard::new();

    let _signal_guard = SignalCleanupGuard::new()?;

    let surface = TerminalSurface::detect(options.surface_preference, options.inline_rows)?;
    let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
    set_log_theme_name(options.log_theme.clone());
    let mut session = Session::new_with_config(options.theme, options.placeholder, surface.rows())?;
    session.show_logs = options.show_logs;
    session.set_log_receiver(log_rx);
    session.active_pty_sessions = options.active_pty_sessions;
    session.set_workspace_root(options.workspace_root.clone());
    register_tui_log_sender(log_tx);

    let keyboard_flags = crate::config::keyboard_protocol_to_flags(&options.keyboard_protocol);
    let mut stderr = io::stderr();
    let mode_state = enable_terminal_modes(&mut stderr, keyboard_flags)?;
    if surface.use_alternate() {
        execute!(stderr, EnterAlternateScreen).context(ALTERNATE_SCREEN_ERROR)?;
    }

    // Set initial terminal title with project name using OSC 2 sequence
    let initial_title = options
        .workspace_root
        .as_ref()
        .and_then(|path| {
            path.file_name()
                .or_else(|| path.parent()?.file_name())
                .map(|name| format!("> VT Code ({})", name.to_string_lossy()))
        })
        .unwrap_or_else(|| TERMINAL_TITLE.to_string());

    // Use OSC 2 sequence directly for cross-terminal compatibility
    let osc_sequence = format!("\x1b]2;{}\x07", initial_title);
    let _ = stderr.write_all(osc_sequence.as_bytes());
    let _ = stderr.flush();

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
        options.event_callback,
    )
    .await;

    // Gracefully shutdown the event loop
    cancellation_token.cancel();
    let _ = tokio::time::timeout(Duration::from_millis(100), event_loop_handle).await;

    // Drain any pending events before finalizing terminal and disabling modes
    drain_terminal_events();

    // Clear current line to remove any echoed characters (like ^C)
    let _ = io::stderr().write_all(b"\r\x1b[K");

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

    // Clear terminal title on exit using OSC 2 sequence
    session.clear_terminal_title();

    drive_result?;
    finalize_result?;

    clear_tui_log_sender();

    Ok(())
}
