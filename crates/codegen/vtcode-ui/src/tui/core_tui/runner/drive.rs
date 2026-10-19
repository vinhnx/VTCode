use std::io;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::Backend,
    crossterm::{cursor::SetCursorStyle, execute},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::tui::core_tui::types::FocusChangeCallback;
use crate::tui::options::FullscreenInteractionSettings;

use super::events::{EventChannels, EventListener, ScrollAccumulator, TerminalEvent};
use super::terminal_io::{MousePointerShape, set_mouse_pointer_shape};
use super::{TuiCommand, TuiSessionDriver};

/// Check if session has any modal or palette active that uses keyboard navigation
fn has_active_navigation_ui<S: TuiSessionDriver>(session: &S) -> bool {
    session.has_active_navigation_ui()
}

fn handle_focus_change_event(event: &crossterm::event::Event, focus_callback: Option<&FocusChangeCallback>) {
    let Some(callback) = focus_callback else {
        return;
    };

    match event {
        crossterm::event::Event::FocusGained => callback(true),
        crossterm::event::Event::FocusLost => callback(false),
        _ => {}
    }
}

#[cfg(unix)]
fn is_suspend_shortcut(event: &crossterm::event::Event) -> bool {
    use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEventKind, KeyModifiers};

    matches!(
        event,
        CrosstermEvent::Key(key)
            if matches!(key.kind, KeyEventKind::Press)
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z') | KeyCode::Char('\u{1A}'))
    )
}

#[cfg(unix)]
fn suspend_to_shell<B: Backend, S: TuiSessionDriver>(
    terminal: &mut Terminal<B>,
    session: &mut S,
    inputs: &mut EventListener,
    event_channels: &EventChannels,
    use_alternate_screen: bool,
    keyboard_flags: crossterm::event::KeyboardEnhancementFlags,
    fullscreen: &FullscreenInteractionSettings,
) -> Result<()> {
    use ratatui::crossterm::{
        event::{
            DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
            EnableMouseCapture, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
        },
        terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };
    use signal_hook::{consts::signal::SIGTSTP, low_level::raise};

    event_channels.pause();
    inputs.clear_queue();

    let suspend_result = (|| -> Result<()> {
        let mut stderr = io::stderr();
        if !keyboard_flags.is_empty()
            && let Err(error) = execute!(stderr, PopKeyboardEnhancementFlags)
        {
            tracing::debug!(%error, "failed to suspend keyboard enhancement flags");
        }
        if use_alternate_screen {
            // Purge the alternate viewport before leaving so suspending to the
            // shell never reveals the last TUI frame in the main scrollback
            // (mirrors the canonical `panic_hook::restore_tui` ordering).
            let _ = execute!(stderr, Clear(ClearType::All));
            execute!(stderr, LeaveAlternateScreen).context("failed to leave alternate screen before suspend")?;
        }

        if let Err(error) = execute!(stderr, DisableFocusChange, DisableBracketedPaste) {
            tracing::debug!(%error, "failed to disable terminal enhancements before suspend");
        }
        if fullscreen.mouse_capture
            && let Err(error) = execute!(stderr, DisableMouseCapture)
        {
            tracing::debug!(%error, "failed to disable mouse capture before suspend");
        }
        // Drain any terminal responses from the disable sequences above
        // while raw mode is still active so individual bytes remain readable.
        super::terminal_io::drain_terminal_events();

        if let Err(error) = disable_raw_mode() {
            tracing::debug!(%error, "failed to disable raw mode before suspend");
        }

        raise(SIGTSTP).context("failed to suspend process with SIGTSTP")?;

        enable_raw_mode().context("failed to re-enable raw mode after resume")?;
        if let Err(error) = execute!(stderr, EnableBracketedPaste, EnableFocusChange) {
            tracing::debug!(%error, "failed to re-enable terminal enhancements after resume");
        }
        if fullscreen.mouse_capture
            && let Err(error) = execute!(stderr, EnableMouseCapture)
        {
            tracing::debug!(%error, "failed to restore mouse capture after resume");
        }
        if use_alternate_screen {
            execute!(stderr, EnterAlternateScreen).context("failed to re-enter alternate screen after resume")?;
            terminal
                .clear()
                .map_err(|error| anyhow::anyhow!("failed to clear terminal after resume: {error}"))?;
        }
        if !keyboard_flags.is_empty()
            && let Err(error) = execute!(stderr, PushKeyboardEnhancementFlags(keyboard_flags))
        {
            tracing::debug!(%error, "failed to restore keyboard enhancement flags after resume");
        }

        Ok(())
    })();

    event_channels.resume();
    inputs.clear_queue();
    if suspend_result.is_ok() {
        session.mark_dirty();
    }

    suspend_result
}

#[must_use]
enum EventStreamAction {
    None,
    Stop,
    Start,
}

fn handle_inline_command<S: TuiSessionDriver>(
    _terminal: &mut Terminal<impl Backend>,
    session: &mut S,
    inputs: &mut EventListener,
    event_channels: &EventChannels,
    command: S::Command,
) -> Result<EventStreamAction> {
    if command.is_suspend_event_loop() {
        event_channels.pause();
        return Ok(EventStreamAction::None);
    }
    if command.is_resume_event_loop() {
        event_channels.resume();
        return Ok(EventStreamAction::None);
    }
    if command.is_clear_input_queue() {
        inputs.clear_queue();
        return Ok(EventStreamAction::None);
    }
    if command.is_stop_event_stream() {
        return Ok(EventStreamAction::Stop);
    }
    if command.is_start_event_stream() {
        return Ok(EventStreamAction::Start);
    }
    if command.is_force_redraw() {
        session.handle_command(command);
        return Ok(EventStreamAction::None);
    }

    session.handle_command(command);
    session.flush_program_status();

    Ok(EventStreamAction::None)
}

/// Process an event-stream action (stop/start) that requires async work.
async fn process_event_stream_action(
    action: EventStreamAction,
    event_channels: &EventChannels,
    inputs: &mut EventListener,
    event_stream: &mut super::EventStreamController,
) {
    match action {
        EventStreamAction::Stop => {
            event_channels.pause();
            event_stream.stop().await;
            super::terminal_io::drain_terminal_events();
            inputs.clear_queue();
        }
        EventStreamAction::Start => {
            event_stream.start();
        }
        EventStreamAction::None => {}
    }
}

/// Maximum number of command runs to handle per turn to prevent unbounded
/// latency under load. Ensures input events and redraws are not starved by a
/// flood of background commands (e.g., during tool execution or PTY output
/// bursts). Coalesced transcript appends count as a single run.
const MAX_COMMANDS_PER_TURN: usize = 16;
/// Maximum number of raw commands to receive per turn while coalescing. A
/// streamed-line burst larger than this yields after one frame, so a runaway
/// producer cannot monopolize the loop; the remainder drains on later frames.
const MAX_DRAINED_COMMANDS_PER_TURN: usize = 512;
/// Maximum number of terminal events to process in one wakeup.
///
/// This keeps redraw latency bounded when the terminal produces a backlog of
/// events (mouse movement, scroll bursts, key repeat, stale ticks) instead of
/// draining the entire queue before painting the latest visible state.
const MAX_TERMINAL_EVENTS_PER_TURN: usize = 32;
const INPUT_TO_DRAW_WARN_MS: u128 = 16;
const DRAW_WARN_MS: u128 = 8;

/// Render immediately if the session is dirty and the event loop is not paused.
///
/// Placing this call right after input processing (same wakeup) eliminates an
/// extra loop iteration between keypress and screen update — the single highest-
/// leverage latency improvement per Dan Luu's terminal latency research.
fn render_if_dirty<B: Backend, S: TuiSessionDriver>(
    terminal: &mut Terminal<B>,
    session: &mut S,
    event_channels: &EventChannels,
    cursor_steady: &mut bool,
    mouse_pointer: &mut MousePointerShape,
    input_started_at: Option<Instant>,
) -> Result<()> {
    if event_channels.rx_paused.load(Ordering::Acquire) {
        return Ok(());
    }
    if !session.take_redraw() {
        crate::tui::frame_metrics::record_frame_skipped();
        return Ok(());
    }

    let _terminal_lock = crate::tui::core_tui::panic_hook::lock_terminal_operations();
    if crate::tui::core_tui::panic_hook::is_restore_claimed() {
        return Ok(());
    }

    let desired_steady = session.use_steady_cursor();
    if desired_steady != *cursor_steady {
        let style = if desired_steady {
            SetCursorStyle::SteadyBlock
        } else {
            SetCursorStyle::DefaultUserShape
        };
        execute!(io::stderr(), style).context("failed to update cursor style for inline session")?;
        *cursor_steady = desired_steady;
    }

    let desired_pointer = if session.is_selecting_text() {
        MousePointerShape::Text
    } else if session.is_hovering_link() {
        MousePointerShape::Pointer
    } else {
        MousePointerShape::Default
    };
    if desired_pointer != *mouse_pointer {
        set_mouse_pointer_shape(desired_pointer);
        *mouse_pointer = desired_pointer;
    }

    let draw_started_at = Instant::now();
    terminal
        .draw(|frame| session.render(frame))
        .map_err(|e| anyhow::anyhow!("failed to draw inline session: {e}"))?;
    vtcode_commons::startup_trace::record_first_render();
    let draw_elapsed = draw_started_at.elapsed();
    let input_to_draw_elapsed = input_started_at.map(|started| started.elapsed());
    crate::tui::frame_metrics::record_draw(draw_elapsed, input_to_draw_elapsed);
    if let Some(input_to_draw_elapsed) = input_to_draw_elapsed {
        if input_to_draw_elapsed.as_millis() >= INPUT_TO_DRAW_WARN_MS || draw_elapsed.as_millis() >= DRAW_WARN_MS {
            tracing::debug!(
                target: "vtcode.tui.latency",
                input_to_draw_ms = input_to_draw_elapsed.as_millis(),
                draw_ms = draw_elapsed.as_millis(),
                "slow input-to-draw path observed"
            );
        }
    } else if draw_elapsed.as_millis() >= DRAW_WARN_MS {
        tracing::debug!(
            target: "vtcode.tui.latency",
            draw_ms = draw_elapsed.as_millis(),
            "slow draw observed"
        );
    }
    Ok(())
}

pub(super) struct DriveRuntimeOptions<E> {
    pub(super) event_callback: Option<super::EventCallback<E>>,
    pub(super) focus_callback: Option<FocusChangeCallback>,
    pub(super) input_activity_counter: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    pub(super) use_alternate_screen: bool,
    pub(super) keyboard_flags: crossterm::event::KeyboardEnhancementFlags,
    pub(super) fullscreen: FullscreenInteractionSettings,
    pub(super) preview_callback: Option<super::PreviewCallback>,
}

fn should_count_as_user_activity(event: &crossterm::event::Event) -> bool {
    matches!(
        event,
        crossterm::event::Event::Key(_) | crossterm::event::Event::Mouse(_) | crossterm::event::Event::Paste(_)
    )
}

/// Folds adjacent transcript appends from a drain slice into one batched
/// command so a burst of streamed lines paints in a single frame.
///
/// The drain offers each received command through [`Self::push`], which returns
/// a command that is ready to handle whenever the incoming command starts a new
/// run (or the accumulator is empty). Non-append commands never merge, so runs
/// stay separated and order is preserved. [`Self::flush`] drains the tail after
/// the slice ends.
pub(super) struct CommandCoalescer<C: TuiCommand> {
    pending: Option<C>,
}

impl<C: TuiCommand> CommandCoalescer<C> {
    pub(super) fn new() -> Self {
        Self { pending: None }
    }

    /// Offer a drained command; returns the command to handle, if any.
    pub(super) fn push(&mut self, command: C) -> Option<C> {
        let mut incoming = Some(command);
        if let Some(current) = self.pending.as_mut()
            && let Some(next) = incoming.take()
        {
            incoming = current.absorb_transcript_append(next).err();
        }
        match incoming {
            Some(command) => self.pending.replace(command),
            None => None,
        }
    }

    /// Return the last accumulated command when the drain slice ends.
    pub(super) fn flush(&mut self) -> Option<C> {
        self.pending.take()
    }
}

/// Drain pending commands, coalescing adjacent transcript appends into single
/// batches. Stops after `max_runs` handled commands or `max_drained` raw
/// commands (whichever comes first) so a flood cannot starve input. Returns the
/// ordered commands to handle and whether the channel disconnected.
fn drain_coalesced<C: TuiCommand>(
    commands: &mut UnboundedReceiver<C>,
    max_runs: usize,
    max_drained: usize,
) -> (Vec<C>, bool) {
    let mut coalescer = CommandCoalescer::<C>::new();
    let mut ready = Vec::new();
    let mut runs = 0usize;
    let mut received = 0usize;
    let mut disconnected = false;
    while runs < max_runs && received < max_drained {
        let command = match commands.try_recv() {
            Ok(command) => command,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                disconnected = true;
                break;
            }
        };
        received += 1;
        if let Some(command) = coalescer.push(command) {
            runs += 1;
            ready.push(command);
        }
    }
    if let Some(command) = coalescer.flush() {
        ready.push(command);
    }
    (ready, disconnected)
}

pub(super) async fn drive_terminal<B: Backend, S: TuiSessionDriver>(
    terminal: &mut Terminal<B>,
    session: &mut S,
    commands: &mut UnboundedReceiver<S::Command>,
    events: &UnboundedSender<S::Event>,
    inputs: &mut EventListener,
    event_channels: EventChannels,
    runtime_options: DriveRuntimeOptions<S::Event>,
    event_stream: &mut super::EventStreamController,
) -> Result<()> {
    #[cfg(not(unix))]
    let _ = (runtime_options.use_alternate_screen, runtime_options.keyboard_flags, &runtime_options.fullscreen);

    let mut cursor_steady = false;
    let mut mouse_pointer = MousePointerShape::Default;
    'main: loop {
        // If terminal restoration was claimed by someone else (host backstop or
        // panic hook), the main screen buffer is live: drawing another frame
        // would leak transcript content into the CLI scrollback.
        if crate::tui::core_tui::panic_hook::is_restore_claimed() {
            session.request_exit();
            break 'main;
        }

        // Drain pending commands so a burst of background work cannot starve
        // input and redraws. Adjacent transcript appends coalesce into one batched
        // command (one run, one frame); every other command separates runs and
        // preserves order. The run budget bounds latency, while the raw-command
        // budget bounds a merged appends burst so it cannot monopolize the loop.
        let (ready, disconnected) = drain_coalesced(commands, MAX_COMMANDS_PER_TURN, MAX_DRAINED_COMMANDS_PER_TURN);
        if disconnected {
            session.request_exit();
        }
        for command in ready {
            let action = handle_inline_command(terminal, session, inputs, &event_channels, command)?;
            process_event_stream_action(action, &event_channels, inputs, event_stream).await;
        }

        // Update terminal title based on current activity state
        session.update_terminal_title();

        // Keep active-rate ticks while animations or transient expiries are
        // pending, independently of recent user input. Input, commands (new
        // messages), and crossterm events already render immediately in the
        // same wakeup; ticks only drive spinner/shimmer frames (80/33ms),
        // drag auto-scroll steps (50ms), and expiry cleanup. A truly idle
        // session therefore rests at the low upkeep rate instead of polling.
        if session.needs_animation_tick() {
            event_channels.record_input();
        }

        // Render if dirty (catches command-driven changes)
        render_if_dirty(terminal, session, &event_channels, &mut cursor_steady, &mut mouse_pointer, None)?;

        if session.should_exit() {
            break 'main;
        }

        // Bias input over commands: when both are ready, prefer user input to
        // minimize keypress-to-screen latency (Dan Luu's key finding).
        tokio::select! {
            biased;

            result = inputs.recv() => {
                match result {
                    Some(TerminalEvent::Crossterm(event)) => {
                        // Record input for adaptive tick rate (switches to active Hz)
                        event_channels.record_input();
                        if should_count_as_user_activity(&event)
                            && let Some(counter) = runtime_options.input_activity_counter.as_ref()
                        {
                            counter.fetch_add(1, Ordering::Relaxed);
                        }
                        handle_focus_change_event(&event, runtime_options.focus_callback.as_ref());

                        // Skip event processing if the TUI is suspended (e.g., external editor is running)
                        if !event_channels.rx_paused.load(Ordering::Acquire) {
                            // Only coalesce scroll events when no modal/palette is active
                            // (otherwise Up/Down should navigate the list, not scroll)
                            let can_coalesce_scroll = !has_active_navigation_ui(session);
                            let mut scroll_accum =
                                ScrollAccumulator::new(runtime_options.fullscreen.scroll_speed);
                            let input_started_at = Instant::now();
                            let mut processed_terminal_events = 1;

                            // Try to accumulate the first event as scroll (only if safe)
                            let first_coalesced = can_coalesce_scroll
                                && scroll_accum.try_accumulate(&event);
                            if !first_coalesced {
                                // Not coalesced, process normally
                                session.handle_event(
                                    event,
                                    events,
                                    runtime_options
                                        .event_callback
                                        .as_ref()
                                        .map(|callback| callback.as_ref()),
                                );
                            }

                            // Process all other pending events, coalescing scroll events when safe
                            while processed_terminal_events < MAX_TERMINAL_EVENTS_PER_TURN
                                && !session.should_exit()
                            {
                                let Ok(next_event) = inputs.try_recv() else {
                                    break;
                                };
                                processed_terminal_events += 1;
                                match next_event {
                                    TerminalEvent::Crossterm(evt) => {
                                        if should_count_as_user_activity(&evt)
                                            && let Some(counter) = runtime_options.input_activity_counter.as_ref()
                                        {
                                            counter.fetch_add(1, Ordering::Relaxed);
                                        }
                                        handle_focus_change_event(
                                            &evt,
                                            runtime_options.focus_callback.as_ref(),
                                        );
                                        // Re-check modal state (it may have changed)
                                        let can_coalesce = !has_active_navigation_ui(session);
                                        let coalesced = can_coalesce
                                            && scroll_accum.try_accumulate(&evt);
                                        if !coalesced {
                                            // Not a scroll event or can't coalesce - apply accumulated first
                                            if scroll_accum.has_scroll() {
                                                scroll_accum.apply(session);
                                                scroll_accum = ScrollAccumulator::new(
                                                    runtime_options.fullscreen.scroll_speed,
                                                );
                                            }
                                            // Then process this event
                                            session.handle_event(
                                                evt,
                                                events,
                                                runtime_options
                                                    .event_callback
                                                    .as_ref()
                                                    .map(|callback| callback.as_ref()),
                                            );
                                        }
                                    }
                                    TerminalEvent::Tick => {
                                        scroll_accum.apply(session);
                                        scroll_accum = ScrollAccumulator::new(
                                            runtime_options.fullscreen.scroll_speed,
                                        );
                                        session.handle_tick();
                                    }
                                }
                            }

                            // Apply any remaining accumulated scroll events
                            if scroll_accum.has_scroll() {
                                scroll_accum.apply(session);
                            }

                            render_if_dirty(
                                terminal,
                                session,
                                &event_channels,
                                &mut cursor_steady,
                                &mut mouse_pointer,
                                Some(input_started_at),
                            )?;
                        }
                    }
                    Some(TerminalEvent::Tick) => {
                        session.handle_tick();
                        render_if_dirty(
                            terminal,
                            session,
                            &event_channels,
                            &mut cursor_steady,
                            &mut mouse_pointer,
                            None,
                        )?;
                    }
                    None => {
                        if commands.is_closed() {
                            break 'main;
                        }
                    }
                }
            }
            command = commands.recv() => {
                match command {
                    Some(command) => {
                        let action = handle_inline_command(
                            terminal, session, inputs, &event_channels, command,
                        )?;
                        process_event_stream_action(
                            action,
                            &event_channels,
                            inputs,
                            event_stream,
                        )
                        .await;
                        continue 'main;
                    }
                    None => {
                        session.request_exit();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::core_tui::session::Session;
    use crate::tui::core_tui::types::InlineCommand as CoreCommand;
    use crate::tui::core_tui::types::InlineTheme;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use std::sync::Arc;
    use vtcode_commons::ui_protocol::{InlineMessageKind, InlineSegment, InlineTextStyle};

    fn agent_line(text: &str) -> CoreCommand {
        CoreCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![InlineSegment {
                text: text.into(),
                style: Arc::new(InlineTextStyle::default()),
            }],
        }
    }

    fn batch_texts(lines: &[vtcode_commons::ui_protocol::InlineTranscriptLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.segments.iter().map(|segment| segment.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn command_coalescer_folds_a_burst_into_one_batch() {
        let mut coalescer = CommandCoalescer::new();
        assert!(coalescer.push(agent_line("one")).is_none());
        assert!(coalescer.push(agent_line("two")).is_none());
        assert!(coalescer.push(agent_line("three")).is_none());

        match coalescer.flush().expect("a burst flushes one command") {
            CoreCommand::AppendLines { lines } => assert_eq!(batch_texts(&lines), ["one", "two", "three"]),
            _ => panic!("expected a single AppendLines batch"),
        }
        assert!(coalescer.flush().is_none(), "a flush is not repeatable");
    }

    #[test]
    fn command_coalescer_releases_the_batch_before_a_non_append() {
        let mut coalescer = CommandCoalescer::new();
        assert!(coalescer.push(agent_line("one")).is_none());
        assert!(coalescer.push(agent_line("two")).is_none());

        let ready = coalescer
            .push(CoreCommand::SetInput("draft".to_owned()))
            .expect("the batch releases before the non-append command");
        match ready {
            CoreCommand::AppendLines { lines } => assert_eq!(batch_texts(&lines), ["one", "two"]),
            _ => panic!("expected the batched appends to release first"),
        }
        assert!(
            matches!(coalescer.flush(), Some(CoreCommand::SetInput(value)) if value == "draft"),
            "the non-append becomes the next pending command"
        );
    }

    #[test]
    fn drain_coalesced_collapses_a_burst_into_one_run() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<CoreCommand>();
        for index in 0..100 {
            sender.send(agent_line(&format!("line-{index}"))).unwrap();
        }

        let (ready, disconnected) =
            drain_coalesced(&mut receiver, MAX_COMMANDS_PER_TURN, MAX_DRAINED_COMMANDS_PER_TURN);

        assert!(!disconnected);
        assert_eq!(ready.len(), 1, "a burst within the raw cap collapses into one run");
        match &ready[0] {
            CoreCommand::AppendLines { lines } => {
                assert_eq!(lines.len(), 100);
                assert_eq!(batch_texts(lines)[0], "line-0");
                assert_eq!(batch_texts(lines)[99], "line-99");
            }
            _ => panic!("expected one AppendLines batch"),
        }
    }

    #[test]
    fn drain_coalesced_keeps_non_append_runs_in_order() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<CoreCommand>();
        sender.send(agent_line("a")).unwrap();
        sender.send(CoreCommand::SetInput("draft".to_owned())).unwrap();
        sender.send(agent_line("b")).unwrap();

        let (ready, _) = drain_coalesced(&mut receiver, MAX_COMMANDS_PER_TURN, MAX_DRAINED_COMMANDS_PER_TURN);

        assert_eq!(ready.len(), 3, "a non-append separates the append runs");
        assert!(matches!(&ready[0], CoreCommand::AppendLine { segments, .. } if segments[0].text == "a"));
        assert!(matches!(&ready[1], CoreCommand::SetInput(value) if value == "draft"));
        assert!(matches!(&ready[2], CoreCommand::AppendLine { segments, .. } if segments[0].text == "b"));
    }

    #[test]
    fn drain_coalesced_yields_after_the_raw_cap() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<CoreCommand>();
        let overflow = 10;
        for index in 0..(MAX_DRAINED_COMMANDS_PER_TURN + overflow) {
            sender.send(agent_line(&format!("line-{index}"))).unwrap();
        }

        let (ready, _) = drain_coalesced(&mut receiver, MAX_COMMANDS_PER_TURN, MAX_DRAINED_COMMANDS_PER_TURN);
        match &ready[0] {
            CoreCommand::AppendLines { lines } => assert_eq!(lines.len(), MAX_DRAINED_COMMANDS_PER_TURN),
            _ => panic!("expected one bounded AppendLines batch"),
        }

        // The remainder stays queued and drains on the next frame.
        let (rest, _) = drain_coalesced(&mut receiver, MAX_COMMANDS_PER_TURN, MAX_DRAINED_COMMANDS_PER_TURN);
        match &rest[0] {
            CoreCommand::AppendLines { lines } => assert_eq!(lines.len(), overflow),
            _ => panic!("expected the overflow batch"),
        }
    }

    #[test]
    fn drain_coalesced_reports_disconnect() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<CoreCommand>();
        sender.send(agent_line("only")).unwrap();
        drop(sender);

        let (ready, disconnected) =
            drain_coalesced(&mut receiver, MAX_COMMANDS_PER_TURN, MAX_DRAINED_COMMANDS_PER_TURN);

        assert!(disconnected);
        assert!(matches!(&ready[0], CoreCommand::AppendLine { segments, .. } if segments[0].text == "only"));
    }

    #[test]
    fn paused_redraw_preserves_dirty_state() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut session = Session::new(InlineTheme::default(), None, 24);
        let (_listener, channels) = EventListener::new();
        channels.pause();
        session.mark_dirty();
        render_if_dirty(&mut terminal, &mut session, &channels, &mut false, &mut MousePointerShape::Default, None)
            .unwrap();
        assert!(session.take_redraw());
    }

    #[test]
    fn activity_counts_input_but_not_focus_or_resize() {
        let page_up = Event::Key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert!(should_count_as_user_activity(&page_up));
        assert!(should_count_as_user_activity(&Event::Paste("text".to_owned())));
        assert!(!should_count_as_user_activity(&Event::FocusGained));
        assert!(!should_count_as_user_activity(&Event::Resize(80, 24)));
    }
}
