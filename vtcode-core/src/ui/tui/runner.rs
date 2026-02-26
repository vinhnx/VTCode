use std::io;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures::{FutureExt, StreamExt};
use ratatui::crossterm::{
    cursor::SetCursorStyle,
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event as CrosstermEvent, MouseEventKind,
    },
    execute,
    terminal::{
        self, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
#[cfg(unix)]
use signal_hook::consts::signal::{SIGINT, SIGTERM};
#[cfg(unix)]
use signal_hook::iterator::Signals;
use terminal_size::{Height, Width, terminal_size};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio_util::sync::CancellationToken;

use crate::config::{constants::ui, types::UiSurfacePreference};
use crate::ui::tui::log::{clear_tui_log_sender, register_tui_log_sender, set_log_theme_name};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineEventCallback, InlineTheme},
};

/// Terminal title displayed when VT Code TUI is active
const TERMINAL_TITLE: &str = "> VT Code";

/// Represents the state of terminal modes before TUI initialization.
///
/// This struct tracks which terminal features were enabled before we
/// modified them, allowing proper restoration on exit.
#[derive(Debug, Clone)]
struct TerminalModeState {
    /// Whether bracketed paste was enabled (we enable it)
    bracketed_paste_enabled: bool,
    /// Whether raw mode was enabled (we enable it)
    raw_mode_enabled: bool,
    /// Whether mouse capture was enabled (we enable it)
    mouse_capture_enabled: bool,
    /// Whether focus change events were enabled (we enable them)
    focus_change_enabled: bool,
    /// Whether keyboard enhancement flags were pushed (we push them)
    keyboard_enhancements_pushed: bool,
}

impl TerminalModeState {
    /// Create a new TerminalModeState with all modes disabled (clean state)
    fn new() -> Self {
        Self {
            bracketed_paste_enabled: false,
            raw_mode_enabled: false,
            mouse_capture_enabled: false,
            focus_change_enabled: false,
            keyboard_enhancements_pushed: false,
        }
    }
}

impl Default for TerminalModeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents accumulated scroll events for coalescing
#[derive(Default)]
struct ScrollAccumulator {
    line_delta: i32,
    page_delta: i32,
}

impl ScrollAccumulator {
    /// Try to accumulate a scroll event. Returns true if the event was a scroll event.
    /// Handles mouse scroll wheel events and PageUp/PageDown keyboard events.
    fn try_accumulate(&mut self, event: &CrosstermEvent) -> bool {
        match event {
            CrosstermEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => {
                    self.line_delta += 1;
                    true
                }
                MouseEventKind::ScrollUp => {
                    self.line_delta -= 1;
                    true
                }
                _ => false,
            },
            CrosstermEvent::Key(key)
                if matches!(key.kind, ratatui::crossterm::event::KeyEventKind::Press) =>
            {
                match key.code {
                    ratatui::crossterm::event::KeyCode::PageUp => {
                        self.page_delta -= 1;
                        true
                    }
                    ratatui::crossterm::event::KeyCode::PageDown => {
                        self.page_delta += 1;
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
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

/// Check if session has any modal or palette active that uses keyboard navigation
fn has_active_navigation_ui(session: &Session) -> bool {
    session.modal.is_some()
        || session.file_palette_active
        || session.config_palette_active
        || crate::ui::tui::session::slash::slash_navigation_available(session)
}

const INLINE_FALLBACK_ROWS: u16 = ui::DEFAULT_INLINE_VIEWPORT_ROWS;
const ALTERNATE_SCREEN_ERROR: &str = "failed to enter alternate inline screen";

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
        use crate::utils::tty::TtyExt;

        let fallback_rows = inline_rows.max(1);
        let stderr_is_terminal = io::stderr().is_tty_ext();

        // Detect terminal capabilities before proceeding
        let capabilities = if stderr_is_terminal {
            crate::utils::tty::TtyCapabilities::detect()
        } else {
            None
        };

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

        // Check if terminal supports the features we need
        if stderr_is_terminal
            && let Some(caps) = capabilities
            && !caps.is_basic_tui()
        {
            tracing::warn!("Terminal has limited capabilities, some features may be disabled");
        }

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
    use std::io::Write;
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

struct SignalCleanupGuard {
    #[cfg(unix)]
    handle: signal_hook::iterator::Handle,
    #[cfg(unix)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SignalCleanupGuard {
    #[cfg(unix)]
    fn new() -> Result<Self> {
        let mut signals =
            Signals::new([SIGINT, SIGTERM]).context("failed to register signal handlers")?;
        let handle = signals.handle();
        let thread = std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = crate::ui::tui::panic_hook::restore_tui();
                std::process::exit(130);
            }
        });

        Ok(Self {
            handle,
            thread: Some(thread),
        })
    }

    #[cfg(not(unix))]
    fn new() -> Result<Self> {
        Ok(Self {})
    }
}

impl Drop for SignalCleanupGuard {
    #[cfg(unix)]
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    #[cfg(not(unix))]
    fn drop(&mut self) {}
}

fn enable_terminal_modes(
    stderr: &mut io::Stderr,
    keyboard_flags: ratatui::crossterm::event::KeyboardEnhancementFlags,
) -> Result<TerminalModeState> {
    use ratatui::crossterm::event::PushKeyboardEnhancementFlags;

    let mut state = TerminalModeState::new();

    // Enable bracketed paste
    match execute!(stderr, EnableBracketedPaste) {
        Ok(_) => state.bracketed_paste_enabled = true,
        Err(error) => {
            tracing::warn!(%error, "failed to enable bracketed paste");
        }
    }

    // Enable raw mode
    match enable_raw_mode() {
        Ok(_) => state.raw_mode_enabled = true,
        Err(error) => {
            return Err(anyhow::anyhow!("failed to enable raw mode: {}", error));
        }
    }

    // Enable mouse capture
    match execute!(stderr, EnableMouseCapture) {
        Ok(_) => state.mouse_capture_enabled = true,
        Err(error) => {
            tracing::warn!(%error, "failed to enable mouse capture");
        }
    }

    // Enable focus change events
    match execute!(stderr, EnableFocusChange) {
        Ok(_) => state.focus_change_enabled = true,
        Err(error) => {
            tracing::debug!(%error, "failed to enable focus change events");
        }
    }

    // Push keyboard enhancement flags
    if !keyboard_flags.is_empty() {
        match execute!(stderr, PushKeyboardEnhancementFlags(keyboard_flags)) {
            Ok(_) => state.keyboard_enhancements_pushed = true,
            Err(error) => {
                tracing::debug!(%error, "failed to push keyboard enhancement flags");
            }
        }
    }

    Ok(state)
}

fn restore_terminal_modes(state: &TerminalModeState) -> Result<()> {
    use ratatui::crossterm::event::PopKeyboardEnhancementFlags;
    let mut stderr = io::stderr();

    let mut errors = Vec::new();

    // Restore in reverse order of enabling

    // 1. Pop keyboard enhancement flags (if they were pushed)
    if state.keyboard_enhancements_pushed
        && let Err(error) = execute!(stderr, PopKeyboardEnhancementFlags)
    {
        tracing::debug!(%error, "failed to pop keyboard enhancement flags");
        errors.push(format!("keyboard enhancements: {}", error));
    }

    // 2. Disable focus change events (if they were enabled)
    if state.focus_change_enabled
        && let Err(error) = execute!(stderr, DisableFocusChange)
    {
        tracing::debug!(%error, "failed to disable focus change events");
        errors.push(format!("focus change: {}", error));
    }

    // 3. Disable mouse capture (if it was enabled)
    if state.mouse_capture_enabled
        && let Err(error) = execute!(stderr, DisableMouseCapture)
    {
        tracing::debug!(%error, "failed to disable mouse capture");
        errors.push(format!("mouse capture: {}", error));
    }

    // 4. Disable bracketed paste (if it was enabled)
    if state.bracketed_paste_enabled
        && let Err(error) = execute!(stderr, DisableBracketedPaste)
    {
        tracing::debug!(%error, "failed to disable bracketed paste");
        errors.push(format!("bracketed paste: {}", error));
    }

    // 5. Disable raw mode LAST (if it was enabled)
    if state.raw_mode_enabled
        && let Err(error) = disable_raw_mode()
    {
        tracing::debug!(%error, "failed to disable raw mode");
        errors.push(format!("raw mode: {}", error));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        tracing::warn!(
            errors = ?errors,
            "some terminal modes failed to restore"
        );
        Ok(()) // Don't fail the operation, just warn
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
    let mut cursor_steady = false;
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
                        terminal.clear().map_err(|e| {
                            anyhow::anyhow!("failed to clear terminal for redraw: {}", e)
                        })?;
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

        // Update terminal title based on current activity state
        session.update_terminal_title();

        if session.thinking_spinner.is_active
            || session.is_running_activity()
            || session.has_status_spinner()
        {
            event_channels.record_input();
        }

        // Only redraw if not suspended
        if !event_channels
            .rx_paused
            .load(std::sync::atomic::Ordering::Acquire)
            && session.take_redraw()
        {
            let desired_steady = session.use_steady_cursor();
            if desired_steady != cursor_steady {
                let style = if desired_steady {
                    SetCursorStyle::SteadyBlock
                } else {
                    SetCursorStyle::DefaultUserShape
                };
                execute!(io::stderr(), style)
                    .context("failed to update cursor style for inline session")?;
                cursor_steady = desired_steady;
            }
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
                        session.handle_tick();
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
    execute!(io::stderr(), SetCursorStyle::DefaultUserShape)
        .context("failed to restore cursor style after inline session")?;
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

/// Drain any pending crossterm events (e.g., resize, focus responses, or buffered keystrokes)
/// so they don't leak to the shell or interfere with next startup.
fn drain_terminal_events() {
    use ratatui::crossterm::event;

    while event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let _ = event::read();
    }
}
