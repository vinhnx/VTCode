use std::io::{self, IsTerminal};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
        Event as CrosstermEvent, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        self, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        supports_keyboard_enhancement,
    },
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
use terminal_size::{Height, Width, terminal_size};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::config::{constants::ui, types::UiSurfacePreference};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineTheme},
};

const INLINE_FALLBACK_ROWS: u16 = ui::DEFAULT_INLINE_VIEWPORT_ROWS;
const INPUT_POLL_INTERVAL_MS: u64 = 16;
const ALTERNATE_SCREEN_ERROR: &str = "failed to enter alternate inline screen";
const RAW_MODE_ENABLE_ERROR: &str = "failed to enable raw mode for inline terminal";
const RAW_MODE_DISABLE_ERROR: &str = "failed to disable raw mode after inline session";
const ENABLE_BRACKETED_PASTE_ERROR: &str = "failed to enable bracketed paste for inline terminal";
const DISABLE_BRACKETED_PASTE_ERROR: &str = "failed to disable bracketed paste for inline terminal";
const KEYBOARD_ENHANCEMENT_QUERY_ERROR: &str =
    "failed to determine keyboard enhancement support for inline terminal";

struct TerminalModeState {
    focus_change_enabled: bool,
    keyboard_enhancements_pushed: bool,
}

type TerminalEvent = CrosstermEvent;

struct InputListener {
    receiver: UnboundedReceiver<TerminalEvent>,
    shutdown: Option<mpsc::Sender<()>>,
}

#[derive(Default)]
struct ViewportTracker {
    last: Option<(u16, u16)>,
}

impl ViewportTracker {
    fn new() -> Self {
        let initial = measure_terminal_dimensions();
        Self { last: initial }
    }

    fn update_from_resize(&mut self, columns: u16, rows: u16) {
        if rows > 0 {
            self.last = Some((columns, rows));
        }
    }

    fn poll_changes(&mut self) -> Option<(u16, u16)> {
        let measurement = measure_terminal_dimensions();
        let Some((columns, rows)) = measurement else {
            return None;
        };

        if self.last == Some((columns, rows)) {
            return None;
        }

        self.last = Some((columns, rows));
        Some((columns, rows))
    }
}

impl InputListener {
    fn spawn() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut viewport = ViewportTracker::new();
            loop {
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                match event::poll(Duration::from_millis(INPUT_POLL_INTERVAL_MS)) {
                    Ok(true) => match event::read() {
                        Ok(event) => {
                            if let CrosstermEvent::Resize(columns, rows) = event {
                                viewport.update_from_resize(columns, rows);
                            }
                            if tx.send(event).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            tracing::debug!(%error, "failed to read crossterm event");
                            break;
                        }
                    },
                    Ok(false) => {
                        if let Some((columns, rows)) = viewport.poll_changes() {
                            let resize = CrosstermEvent::Resize(columns, rows);
                            if tx.send(resize).is_err() {
                                break;
                            }
                            continue;
                        }

                        if tx.is_closed() {
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::debug!(%error, "failed to poll crossterm event");
                        break;
                    }
                }
            }
        });

        Self {
            receiver: rx,
            shutdown: Some(shutdown_tx),
        }
    }

    async fn recv(&mut self) -> Option<TerminalEvent> {
        self.receiver.recv().await
    }
}

impl Drop for InputListener {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

struct TerminalSurface {
    rows: u16,
    alternate: bool,
}

impl TerminalSurface {
    fn detect(preference: UiSurfacePreference, inline_rows: u16) -> Result<Self> {
        let fallback_rows = inline_rows.max(1);
        let stdout_is_terminal = io::stdout().is_terminal();
        let resolved_rows = if stdout_is_terminal {
            match measure_terminal_dimensions() {
                Some((_, rows)) if rows > 0 => rows,
                _ => match terminal::size() {
                    Ok((_, 0)) => fallback_rows.max(INLINE_FALLBACK_ROWS),
                    Ok((_, rows)) => rows,
                    Err(error) => {
                        tracing::debug!(%error, "failed to determine terminal size");
                        fallback_rows.max(INLINE_FALLBACK_ROWS)
                    }
                },
            }
        } else {
            fallback_rows.max(INLINE_FALLBACK_ROWS)
        };

        let resolved_rows = resolved_rows.max(1);
        let use_alternate = match preference {
            UiSurfacePreference::Alternate => stdout_is_terminal,
            UiSurfacePreference::Inline => false,
            UiSurfacePreference::Auto => stdout_is_terminal,
        };

        if use_alternate && !stdout_is_terminal {
            tracing::debug!("alternate surface requested but stdout is not a tty");
        }

        Ok(Self {
            rows: resolved_rows,
            alternate: use_alternate && stdout_is_terminal,
        })
    }

    fn rows(&self) -> u16 {
        self.rows
    }

    fn use_alternate(&self) -> bool {
        self.alternate
    }
}

pub async fn run_tui(
    mut commands: UnboundedReceiver<InlineCommand>,
    events: UnboundedSender<InlineEvent>,
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    show_timeline_pane: bool,
) -> Result<()> {
    let surface = TerminalSurface::detect(surface_preference, inline_rows)?;
    let mut session = Session::new(theme, placeholder, surface.rows(), show_timeline_pane);
    let mut inputs = InputListener::spawn();

    let mut stdout = io::stdout();
    let mode_state = enable_terminal_modes(&mut stdout)?;
    if surface.use_alternate() {
        execute!(stdout, EnterAlternateScreen).context(ALTERNATE_SCREEN_ERROR)?;
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to initialize inline terminal")?;
    prepare_terminal(&mut terminal)?;

    let drive_result = drive_terminal(
        &mut terminal,
        &mut session,
        &mut commands,
        &events,
        &mut inputs,
    )
    .await;
    let finalize_result = finalize_terminal(&mut terminal);
    let leave_alternate_result = if surface.use_alternate() {
        Some(execute!(terminal.backend_mut(), LeaveAlternateScreen))
    } else {
        None
    };

    if let Some(result) = leave_alternate_result {
        result.context("failed to leave alternate inline screen")?;
    }

    let restore_modes_result = restore_terminal_modes(&mode_state);
    let raw_mode_result = disable_raw_mode();

    restore_modes_result?;
    raw_mode_result.context(RAW_MODE_DISABLE_ERROR)?;

    drive_result?;
    finalize_result?;

    Ok(())
}

fn enable_terminal_modes(stdout: &mut io::Stdout) -> Result<TerminalModeState> {
    execute!(stdout, EnableBracketedPaste).context(ENABLE_BRACKETED_PASTE_ERROR)?;
    enable_raw_mode().context(RAW_MODE_ENABLE_ERROR)?;

    let focus_change_enabled = match execute!(stdout, EnableFocusChange) {
        Ok(_) => true,
        Err(error) => {
            tracing::debug!(%error, "failed to enable focus change events for inline terminal");
            false
        }
    };

    let keyboard_enhancements_pushed =
        if supports_keyboard_enhancement().context(KEYBOARD_ENHANCEMENT_QUERY_ERROR)? {
            match execute!(
                stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
                ),
            ) {
                Ok(_) => true,
                Err(error) => {
                    tracing::debug!(
                        %error,
                        "failed to enable keyboard enhancement flags for inline terminal"
                    );
                    false
                }
            }
        } else {
            false
        };

    Ok(TerminalModeState {
        focus_change_enabled,
        keyboard_enhancements_pushed,
    })
}

fn restore_terminal_modes(state: &TerminalModeState) -> Result<()> {
    let mut stdout = io::stdout();
    if state.keyboard_enhancements_pushed {
        if let Err(error) = execute!(stdout, PopKeyboardEnhancementFlags) {
            tracing::debug!(
                %error,
                "failed to disable keyboard enhancement flags for inline terminal"
            );
        }
    }

    if state.focus_change_enabled {
        if let Err(error) = execute!(stdout, DisableFocusChange) {
            tracing::debug!(
                %error,
                "failed to disable focus change events for inline terminal"
            );
        }
    }

    execute!(stdout, DisableBracketedPaste).context(DISABLE_BRACKETED_PASTE_ERROR)?;

    Ok(())
}

async fn drive_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    session: &mut Session,
    commands: &mut UnboundedReceiver<InlineCommand>,
    events: &UnboundedSender<InlineEvent>,
    inputs: &mut InputListener,
) -> Result<()> {
    'main: loop {
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    session.handle_command(command);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    session.request_exit();
                    break;
                }
            }
        }

        if session.take_redraw() {
            terminal
                .draw(|frame| session.render(frame))
                .context("failed to draw inline session")?;
        }

        if session.should_exit() {
            break 'main;
        }

        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(command) => {
                        session.handle_command(command);
                        continue 'main;
                    }
                    None => {
                        session.request_exit();
                    }
                }
            }
            result = inputs.recv() => {
                match result {
                    Some(event) => {
                        session.handle_event(event, events);
                        if session.take_redraw() {
                            terminal
                                .draw(|frame| session.render(frame))
                                .context("failed to draw inline session")?;
                        }
                    }
                    None => {
                        if commands.is_closed() {
                            break 'main;
                        }
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(INPUT_POLL_INTERVAL_MS)) => {}
        }

        if session.should_exit() {
            break 'main;
        }
    }

    Ok(())
}

fn measure_terminal_dimensions() -> Option<(u16, u16)> {
    let (Width(columns), Height(rows)) = terminal_size()?;
    if rows == 0 {
        return None;
    }
    Some((columns, rows))
}

fn prepare_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    terminal
        .hide_cursor()
        .context("failed to hide inline cursor")?;
    terminal
        .clear()
        .context("failed to clear inline terminal")?;
    Ok(())
}

fn finalize_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    terminal
        .show_cursor()
        .context("failed to show cursor after inline session")?;
    terminal
        .clear()
        .context("failed to clear inline terminal after session")?;
    terminal
        .flush()
        .context("failed to flush inline terminal after session")?;
    Ok(())
}
