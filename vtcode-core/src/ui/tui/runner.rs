use std::env;
use std::io::{self, IsTerminal};
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

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
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;

use crate::config::{constants::ui, types::UiSurfacePreference};
use crate::ui::tui::log::{clear_tui_log_sender, register_tui_log_sender, set_log_theme_name};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineEventCallback, InlineTheme},
};

/// Represents accumulated scroll events for coalescing
#[derive(Default)]
struct ScrollAccumulator {
    line_delta: i32,
    page_delta: i32,
}

impl ScrollAccumulator {
    /// Try to accumulate a scroll event. Returns true if the event was a scroll event.
    /// IMPORTANT: Only call this when no modal/palette is active, otherwise navigation breaks.
    fn try_accumulate(&mut self, event: &CrosstermEvent) -> bool {
        if let CrosstermEvent::Key(key) = event
            && matches!(key.kind, ratatui::crossterm::event::KeyEventKind::Press)
        {
            match key.code {
                ratatui::crossterm::event::KeyCode::Up => {
                    self.line_delta -= 1;
                    return true;
                }
                ratatui::crossterm::event::KeyCode::Down => {
                    self.line_delta += 1;
                    return true;
                }
                ratatui::crossterm::event::KeyCode::PageUp => {
                    self.page_delta -= 1;
                    return true;
                }
                ratatui::crossterm::event::KeyCode::PageDown => {
                    self.page_delta += 1;
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Check if there are any accumulated scroll events
    fn has_scroll(&self) -> bool {
        self.line_delta != 0 || self.page_delta != 0
    }

    /// Apply accumulated scroll to the session using the coalesced scroll method
    fn apply(&self, session: &mut Session) {
        if self.has_scroll() {
            session.apply_coalesced_scroll(self.line_delta, self.page_delta);
            session.mark_dirty();
        }
    }
}

/// Check if session has any modal or palette active that uses arrow key navigation
fn has_active_navigation_ui(session: &Session) -> bool {
    session.modal.is_some()
        || session.file_palette_active
        || session.prompt_palette_active
        || session.config_palette_active
        || crate::ui::tui::session::slash::slash_navigation_available(session)
}

const INLINE_FALLBACK_ROWS: u16 = ui::DEFAULT_INLINE_VIEWPORT_ROWS;
const ALTERNATE_SCREEN_ERROR: &str = "failed to enter alternate inline screen";
const RAW_MODE_ENABLE_ERROR: &str = "failed to enable raw mode for inline terminal";
const RAW_MODE_DISABLE_ERROR: &str = "failed to disable raw mode after inline session";
const ENABLE_BRACKETED_PASTE_ERROR: &str = "failed to enable bracketed paste for inline terminal";
const DISABLE_BRACKETED_PASTE_ERROR: &str = "failed to disable bracketed paste for inline terminal";

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
    /// Tracks last input time for adaptive tick rate (milliseconds since session start)
    last_input_elapsed_ms: std::sync::Arc<AtomicU64>,
    /// Session start time for calculating elapsed time
    session_start: Instant,
}

impl EventChannels {
    fn new(tx: UnboundedSender<TerminalEvent>) -> Self {
        Self {
            tx,
            rx_paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_input_elapsed_ms: std::sync::Arc::new(AtomicU64::new(0)),
            session_start: Instant::now(),
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

    /// Record that user input was received (updates last input timestamp)
    /// Uses Instant-based tracking for efficiency (no syscalls)
    fn record_input(&self) {
        let elapsed_ms = self.session_start.elapsed().as_millis() as u64;
        self.last_input_elapsed_ms
            .store(elapsed_ms, std::sync::atomic::Ordering::Release);
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
// Implements adaptive tick rate: 16Hz when active, 4Hz when idle
async fn spawn_event_loop(
    event_tx: UnboundedSender<TerminalEvent>,
    cancellation_token: CancellationToken,
    rx_paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
    last_input_elapsed_ms: std::sync::Arc<AtomicU64>,
    session_start: Instant,
) {
    let mut reader = crossterm::event::EventStream::new();
    let active_tick_duration = Duration::from_secs_f64(1.0 / ui::TUI_ACTIVE_TICK_RATE_HZ);
    let idle_tick_duration = Duration::from_secs_f64(1.0 / ui::TUI_IDLE_TICK_RATE_HZ);
    let active_timeout_ms = ui::TUI_ACTIVE_TIMEOUT_MS;

    let mut last_tick = Instant::now();

    loop {
        // Determine current tick rate based on recent activity (using Instant, no syscalls)
        let last_input = last_input_elapsed_ms.load(std::sync::atomic::Ordering::Acquire);
        let is_active = if last_input == 0 {
            false
        } else {
            let current_elapsed = session_start.elapsed().as_millis() as u64;
            current_elapsed.saturating_sub(last_input) < active_timeout_ms
        };

        let tick_duration = if is_active {
            active_tick_duration
        } else {
            idle_tick_duration
        };

        // Calculate remaining time until next tick
        let elapsed = last_tick.elapsed();
        let sleep_duration = tick_duration.saturating_sub(elapsed);

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
            _ = tokio::time::sleep(sleep_duration) => {
                let _ = event_tx.send(TerminalEvent::Tick);
                last_tick = Instant::now();
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
    pub show_logs: bool,
    pub log_theme: Option<String>,
    pub event_callback: Option<InlineEventCallback>,
    pub custom_prompts: Option<crate::prompts::CustomPromptRegistry>,
    pub keyboard_flags: Option<KeyboardEnhancementFlags>,
    pub active_pty_sessions: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
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
        options.show_logs,
    );
    session.set_log_receiver(log_rx);
    session.active_pty_sessions = options.active_pty_sessions;
    register_tui_log_sender(log_tx);

    // Pre-load custom prompts if provided
    if let Some(prompts) = options.custom_prompts {
        session.set_custom_prompts(prompts);
    }

    let mut stderr = io::stderr();
    let keyboard_support = detect_keyboard_enhancement_support(
        options
            .keyboard_flags
            .unwrap_or(KeyboardEnhancementFlags::empty()),
    )
    .await;
    let keyboard_flags = options.keyboard_flags.unwrap_or_else(|| {
        // Default flags match current hardcoded behavior
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
    });
    let mode_state = enable_terminal_modes(&mut stderr, keyboard_flags, keyboard_support)?;
    if surface.use_alternate() {
        execute!(stderr, EnterAlternateScreen).context(ALTERNATE_SCREEN_ERROR)?;
    }

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
    drain_startup_events();

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

fn enable_terminal_modes(
    stderr: &mut io::Stderr,
    keyboard_flags: KeyboardEnhancementFlags,
    keyboard_enhancement_supported: bool,
) -> Result<TerminalModeState> {
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
        if keyboard_enhancement_supported && !keyboard_flags.is_empty() {
            match execute!(stderr, PushKeyboardEnhancementFlags(keyboard_flags)) {
                Ok(_) => {
                    tracing::debug!(?keyboard_flags, "enabled keyboard enhancement flags");
                    true
                }
                Err(error) => {
                    tracing::debug!(%error, "failed to enable keyboard enhancement flags");
                    false
                }
            }
        } else {
            if keyboard_flags.is_empty() {
                tracing::debug!("keyboard protocol disabled via configuration");
            }
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

async fn detect_keyboard_enhancement_support(flags: KeyboardEnhancementFlags) -> bool {
    if flags.is_empty() {
        return false;
    }

    if keyboard_protocol_env_disabled() {
        tracing::info!("keyboard protocol disabled via VTCODE_KBD_PROTOCOL env override");
        return false;
    }

    // The crossterm capability probe can block while waiting for a terminal response. Bound it so
    // startup is not delayed.
    let probe_start = Instant::now();
    match tokio::time::timeout(
        Duration::from_millis(200),
        spawn_blocking(|| supports_keyboard_enhancement().unwrap_or(false)),
    )
    .await
    {
        Ok(Ok(supported)) => {
            tracing::debug!(
                supported,
                elapsed_ms = probe_start.elapsed().as_millis(),
                "keyboard enhancement support probe completed"
            );
            supported
        }
        Ok(Err(join_error)) => {
            tracing::debug!(%join_error, "keyboard enhancement support probe failed");
            false
        }
        Err(_) => {
            tracing::warn!(
                "keyboard enhancement support probe timed out; disabling protocol to avoid startup lag"
            );
            false
        }
    }
}

fn keyboard_protocol_env_disabled() -> bool {
    match env::var("VTCODE_KBD_PROTOCOL") {
        Ok(val) => {
            let v = val.trim().to_ascii_lowercase();
            matches!(
                v.as_str(),
                "0" | "false" | "off" | "disable" | "disabled" | "no"
            )
        }
        Err(_) => false,
    }
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
                            .map_err(|e| anyhow::anyhow!("failed to clear terminal for redraw: {}", e))?;
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
                .map_err(|e| anyhow::anyhow!("failed to draw inline session: {}", e))?;
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
                                terminal.clear().map_err(|e| anyhow::anyhow!("failed to clear terminal for redraw: {}", e))?;
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
                        // Record input for adaptive tick rate (switches to 16Hz)
                        event_channels.record_input();

                        // Skip event processing if the TUI is suspended (e.g., external editor is running)
                        if !event_channels.rx_paused.load(std::sync::atomic::Ordering::Acquire) {
                            // Only coalesce scroll events when no modal/palette is active
                            // (otherwise Up/Down should navigate the list, not scroll)
                            let can_coalesce_scroll = !has_active_navigation_ui(session);
                            let mut scroll_accum = ScrollAccumulator::default();

                            // Try to accumulate the first event as scroll (only if safe)
                            let first_coalesced = can_coalesce_scroll
                                && scroll_accum.try_accumulate(&event);
                            if !first_coalesced {
                                // Not coalesced, process normally
                                session.handle_event(
                                    event,
                                    events,
                                    event_callback.as_ref().map(|callback| callback.as_ref()),
                                );
                            }

                            // Process all other pending events, coalescing scroll events when safe
                            while let Ok(next_event) = inputs.receiver.try_recv() {
                                match next_event {
                                    TerminalEvent::Crossterm(evt) => {
                                        // Re-check modal state (it may have changed)
                                        let can_coalesce = !has_active_navigation_ui(session);
                                        let coalesced = can_coalesce
                                            && scroll_accum.try_accumulate(&evt);
                                        if !coalesced {
                                            // Not a scroll event or can't coalesce - apply accumulated first
                                            if scroll_accum.has_scroll() {
                                                scroll_accum.apply(session);
                                                scroll_accum = ScrollAccumulator::default();
                                            }
                                            // Then process this event
                                            session.handle_event(
                                                evt,
                                                events,
                                                event_callback.as_ref().map(|callback| callback.as_ref()),
                                            );
                                        }
                                    }
                                    TerminalEvent::Tick => {
                                        // Ticks are handled by the main loop's redraw check
                                    }
                                }
                            }

                            // Apply any remaining accumulated scroll events
                            if scroll_accum.has_scroll() {
                                scroll_accum.apply(session);
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
        .map_err(|e| anyhow::anyhow!("failed to hide inline cursor: {}", e))?;
    terminal
        .clear()
        .map_err(|e| anyhow::anyhow!("failed to clear inline terminal: {}", e))?;
    Ok(())
}

fn finalize_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    terminal
        .show_cursor()
        .map_err(|e| anyhow::anyhow!("failed to show cursor after inline session: {}", e))?;
    terminal
        .clear()
        .map_err(|e| anyhow::anyhow!("failed to clear inline terminal after session: {}", e))?;
    terminal
        .flush()
        .map_err(|e| anyhow::anyhow!("failed to flush inline terminal after session: {}", e))?;
    Ok(())
}

/// Drain any pending crossterm events that may have been emitted during terminal setup (e.g.,
/// resize or focus responses) so that the first user keystroke is processed immediately.
fn drain_startup_events() {
    use ratatui::crossterm::event;

    while event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let _ = event::read();
    }
}
