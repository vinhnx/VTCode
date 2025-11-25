use std::io::{self, IsTerminal};
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
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use crate::config::{constants::ui, types::UiSurfacePreference};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineEventCallback, InlineTheme},
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

#[derive(Clone)]
struct EventChannels {
    tx: UnboundedSender<TerminalEvent>,
    rx_paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EventChannels {
    fn new(tx: UnboundedSender<TerminalEvent>) -> Self {
        Self {
            tx,
            rx_paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn pause(&self) {
        self.rx_paused
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn resume(&self) {
        self.rx_paused
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

struct EventListener {
    receiver: UnboundedReceiver<TerminalEvent>,
}

impl EventListener {
    fn new() -> (Self, EventChannels) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let channels = EventChannels::new(tx);
        (Self { receiver: rx }, channels)
    }

    async fn recv(&mut self) -> Option<TerminalEvent> {
        self.receiver.recv().await
    }

    /// Clear all queued events from the input channel
    fn clear_queue(&mut self) {
        while self.receiver.try_recv().is_ok() {
            // Keep draining until empty
        }
    }
}

// Spawn the async event loop with proper cancellation token support
// Uses blocking reads via tokio::task::block_in_place for crossterm compatibility
async fn spawn_event_loop(
    event_tx: UnboundedSender<TerminalEvent>,
    cancellation_token: CancellationToken,
    rx_paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut tick_interval = interval(Duration::from_secs_f64(1.0 / 4.0)); // 4 ticks per second
    let poll_timeout = Duration::from_millis(16);

    loop {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                break;
            }
            _ = tick_interval.tick() => {
                // Tick for app logic updates - continue loop for next event
            }
            _ = async {
                // Use block_in_place to allow crossterm's blocking poll
                tokio::task::block_in_place(|| {
                    // Only poll if not paused. When paused (e.g., during external editor launch),
                    // skip polling to prevent reading from stdin while the editor is active.
                    if !rx_paused.load(std::sync::atomic::Ordering::Acquire)
                        && event::poll(poll_timeout).unwrap_or(false)
                        && let Ok(event) = event::read()
                    {
                        let _ = event_tx.send(event);
                    }
                })
            } => {
                // Event has been sent
            }
        }

        if event_tx.is_closed() {
            break;
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
    event_callback: Option<InlineEventCallback>,
    custom_prompts: Option<crate::prompts::CustomPromptRegistry>,
) -> Result<()> {
    let surface = TerminalSurface::detect(surface_preference, inline_rows)?;
    let mut session = Session::new(theme, placeholder, surface.rows(), show_timeline_pane);

    // Pre-load custom prompts if provided
    if let Some(prompts) = custom_prompts {
        session.set_custom_prompts(prompts);
    }

    // Create event listener and channels using the new async pattern
    let (mut input_listener, event_channels) = EventListener::new();
    let cancellation_token = CancellationToken::new();
    let event_loop_token = cancellation_token.clone();
    let event_channels_for_loop = event_channels.clone();
    let rx_paused = event_channels.rx_paused.clone();

    // Spawn the async event loop
    let event_loop_handle = tokio::spawn(async move {
        spawn_event_loop(
            event_channels_for_loop.tx.clone(),
            event_loop_token,
            rx_paused,
        )
        .await;
    });

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
        &mut input_listener,
        event_channels,
        event_callback,
    )
    .await;

    // Gracefully shutdown the event loop
    cancellation_token.cancel();
    let _ = tokio::time::timeout(Duration::from_millis(100), event_loop_handle).await;

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
    if state.keyboard_enhancements_pushed
        && let Err(error) = execute!(stdout, PopKeyboardEnhancementFlags)
    {
        tracing::debug!(
            %error,
            "failed to disable keyboard enhancement flags for inline terminal"
        );
    }

    if state.focus_change_enabled
        && let Err(error) = execute!(stdout, DisableFocusChange)
    {
        tracing::debug!(
            %error,
            "failed to disable focus change events for inline terminal"
        );
    }

    execute!(stdout, DisableBracketedPaste).context(DISABLE_BRACKETED_PASTE_ERROR)?;

    Ok(())
}

async fn drive_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    session: &mut Session,
    commands: &mut UnboundedReceiver<InlineCommand>,
    events: &UnboundedSender<InlineEvent>,
    inputs: &mut EventListener,
    event_channels: EventChannels,
    event_callback: Option<InlineEventCallback>,
) -> Result<()> {
    'main: loop {
        // Process all pending commands without blocking
        loop {
            match commands.try_recv() {
                Ok(command) => match command {
                    InlineCommand::SuspendEventLoop => {
                        event_channels.pause();
                    }
                    InlineCommand::ResumeEventLoop => {
                        event_channels.resume();
                    }
                    InlineCommand::ClearInputQueue => {
                        inputs.clear_queue();
                    }
                    InlineCommand::ForceRedraw => {
                        terminal
                            .clear()
                            .context("failed to clear terminal for redraw")?;
                        session.handle_command(command);
                    }
                    _ => {
                        session.handle_command(command);
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    session.request_exit();
                    break;
                }
            }
        }

        // Only redraw if not suspended
        if !event_channels
            .rx_paused
            .load(std::sync::atomic::Ordering::Acquire)
            && session.take_redraw()
        {
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
                        match command {
                            InlineCommand::SuspendEventLoop => {
                                event_channels.pause();
                            }
                            InlineCommand::ResumeEventLoop => {
                                event_channels.resume();
                            }
                            InlineCommand::ClearInputQueue => {
                                inputs.clear_queue();
                            }
                            InlineCommand::ForceRedraw => {
                                terminal.clear().context("failed to clear terminal for redraw")?;
                                session.handle_command(command);
                            }
                            _ => {
                                session.handle_command(command);
                            }
                        }
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
                        // Skip event processing if the TUI is suspended (e.g., external editor is running)
                        if !event_channels.rx_paused.load(std::sync::atomic::Ordering::Acquire) {
                            session.handle_event(
                                event,
                                events,
                                event_callback.as_ref().map(|callback| callback.as_ref()),
                            );
                            if session.take_redraw() {
                                terminal
                                    .draw(|frame| session.render(frame))
                                    .context("failed to draw inline session")?;
                            }
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
