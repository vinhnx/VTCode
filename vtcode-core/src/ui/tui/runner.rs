use std::io::{self, IsTerminal};
use std::time::Duration;

use anyhow::{Context, Result};
use futures::{FutureExt, StreamExt};
use ratatui::crossterm::{
    event::{
        DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
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
use crate::ui::tui::log::{clear_tui_log_sender, register_tui_log_sender, set_log_theme_name};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineEventCallback, InlineTheme},
};

const INLINE_FALLBACK_ROWS: u16 = ui::DEFAULT_INLINE_VIEWPORT_ROWS;
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

#[derive(Debug, Clone)]
enum TerminalEvent {
    Tick,
    Crossterm(CrosstermEvent),
}

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
// Uses crossterm::event::EventStream for async-native event handling
async fn spawn_event_loop(
    event_tx: UnboundedSender<TerminalEvent>,
    cancellation_token: CancellationToken,
    rx_paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut reader = crossterm::event::EventStream::new();
    let mut tick_interval = interval(Duration::from_secs_f64(1.0 / 4.0)); // 4 ticks per second

    loop {
        let tick_delay = tick_interval.tick();
        let crossterm_event = reader.next().fuse();

        tokio::select! {
            _ = cancellation_token.cancelled() => {
                break;
            }
            maybe_event = crossterm_event => {
                match maybe_event {
                    Some(Ok(evt)) => {
                        // Only send if not paused. When paused (e.g., during external editor launch),
                        // skip sending to prevent processing input while the editor is active.
                        if !rx_paused.load(std::sync::atomic::Ordering::Acquire) {
                            let _ = event_tx.send(TerminalEvent::Crossterm(evt));
                        }
                    }
                    Some(Err(error)) => {
                        tracing::error!(%error, "terminal event stream error");
                    }
                    None => {}
                }
            }
            _ = tick_delay => {
                let _ = event_tx.send(TerminalEvent::Tick);
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
        let stderr_is_terminal = io::stderr().is_terminal();
        let resolved_rows = if stderr_is_terminal {
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
            UiSurfacePreference::Alternate => stderr_is_terminal,
            UiSurfacePreference::Inline => false,
            UiSurfacePreference::Auto => stderr_is_terminal,
        };

        if use_alternate && !stderr_is_terminal {
            tracing::debug!("alternate surface requested but stderr is not a tty");
        }

        Ok(Self {
            rows: resolved_rows,
            alternate: use_alternate && stderr_is_terminal,
        })
    }

    fn rows(&self) -> u16 {
        self.rows
    }

    fn use_alternate(&self) -> bool {
        self.alternate
    }
}

pub struct TuiOptions {
    pub theme: InlineTheme,
    pub placeholder: Option<String>,
    pub surface_preference: UiSurfacePreference,
    pub inline_rows: u16,
    pub show_timeline_pane: bool,
    pub show_logs: bool,
    pub log_theme: Option<String>,
    pub event_callback: Option<InlineEventCallback>,
    pub custom_prompts: Option<crate::prompts::CustomPromptRegistry>,
}

pub async fn run_tui(
    mut commands: UnboundedReceiver<InlineCommand>,
    events: UnboundedSender<InlineEvent>,
    options: TuiOptions,
) -> Result<()> {
    // Create a guard to mark TUI as initialized during the session
    // This ensures the panic hook knows to restore terminal state
    let _panic_guard = crate::ui::tui::panic_hook::TuiPanicGuard::new();

    let surface = TerminalSurface::detect(options.surface_preference, options.inline_rows)?;
    let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
    set_log_theme_name(options.log_theme.clone());
    let mut session = Session::new_with_logs(
        options.theme,
        options.placeholder,
        surface.rows(),
        options.show_timeline_pane,
        options.show_logs,
    );
    session.set_log_receiver(log_rx);
    register_tui_log_sender(log_tx);

    // Pre-load custom prompts if provided
    if let Some(prompts) = options.custom_prompts {
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

    let mut stderr = io::stderr();
    let mode_state = enable_terminal_modes(&mut stderr)?;
    if surface.use_alternate() {
        execute!(stderr, EnterAlternateScreen).context(ALTERNATE_SCREEN_ERROR)?;
    }

    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend).context("failed to initialize inline terminal")?;
    prepare_terminal(&mut terminal)?;

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

    clear_tui_log_sender();

    Ok(())
}

fn enable_terminal_modes(stderr: &mut io::Stderr) -> Result<TerminalModeState> {
    execute!(stderr, EnableBracketedPaste).context(ENABLE_BRACKETED_PASTE_ERROR)?;
    enable_raw_mode().context(RAW_MODE_ENABLE_ERROR)?;

    let focus_change_enabled = match execute!(stderr, EnableFocusChange) {
        Ok(_) => true,
        Err(error) => {
            tracing::debug!(%error, "failed to enable focus change events for inline terminal");
            false
        }
    };

    let keyboard_enhancements_pushed =
        if supports_keyboard_enhancement().context(KEYBOARD_ENHANCEMENT_QUERY_ERROR)? {
            match execute!(
                stderr,
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
    let mut stderr = io::stderr();
    if state.keyboard_enhancements_pushed
        && let Err(error) = execute!(stderr, PopKeyboardEnhancementFlags)
    {
        tracing::debug!(
            %error,
            "failed to disable keyboard enhancement flags for inline terminal"
        );
    }

    if state.focus_change_enabled
        && let Err(error) = execute!(stderr, DisableFocusChange)
    {
        tracing::debug!(
            %error,
            "failed to disable focus change events for inline terminal"
        );
    }

    execute!(stderr, DisableBracketedPaste).context(DISABLE_BRACKETED_PASTE_ERROR)?;

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
                        session.handle_command(InlineCommand::ForceRedraw);
                    }
                    cmd => {
                        session.handle_command(cmd);
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
                                session.handle_command(InlineCommand::ForceRedraw);
                            }
                            cmd => {
                                session.handle_command(cmd);
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
                    Some(TerminalEvent::Crossterm(event)) => {
                        // Skip event processing if the TUI is suspended (e.g., external editor is running)
                        if !event_channels.rx_paused.load(std::sync::atomic::Ordering::Acquire) {
                            session.handle_event(
                                event,
                                events,
                                event_callback.as_ref().map(|callback| callback.as_ref()),
                            );

                            // Process all other pending events to avoid redundant draws
                            while let Ok(next_event) = inputs.receiver.try_recv() {
                                match next_event {
                                    TerminalEvent::Crossterm(evt) => {
                                        session.handle_event(
                                            evt,
                                            events,
                                            event_callback.as_ref().map(|callback| callback.as_ref()),
                                        );
                                    }
                                    TerminalEvent::Tick => {
                                        // Ticks are handled by the main loop's redraw check
                                    }
                                }
                            }
                        }
                    }
                    Some(TerminalEvent::Tick) => {
                        // Periodic tick for animations or state updates
                    }
                    None => {
                        if commands.is_closed() {
                            break 'main;
                        }
                    }
                }
            }
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
