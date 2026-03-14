use std::io;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::Backend,
    crossterm::{cursor::SetCursorStyle, execute},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::ui::tui::{
    session::Session,
    types::{FocusChangeCallback, InlineCommand, InlineEvent, InlineEventCallback},
};

use super::events::{EventChannels, EventListener, ScrollAccumulator, TerminalEvent};

/// Check if session has any modal or palette active that uses keyboard navigation
fn has_active_navigation_ui(session: &Session) -> bool {
    session.has_active_overlay()
        || session.file_palette_active
        || session.history_picker_state.active
        || crate::ui::tui::session::slash::slash_navigation_available(session)
}

fn handle_focus_change_event(
    event: &crossterm::event::Event,
    focus_callback: Option<&FocusChangeCallback>,
) {
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
fn suspend_to_shell<B: Backend>(
    terminal: &mut Terminal<B>,
    session: &mut Session,
    inputs: &mut EventListener,
    event_channels: &EventChannels,
    use_alternate_screen: bool,
    keyboard_flags: crossterm::event::KeyboardEnhancementFlags,
) -> Result<()> {
    use ratatui::crossterm::{
        event::{
            DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
            EnableFocusChange, EnableMouseCapture, PushKeyboardEnhancementFlags,
        },
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };
    use signal_hook::{consts::signal::SIGTSTP, low_level::raise};

    event_channels.pause();
    inputs.clear_queue();

    let suspend_result = (|| -> Result<()> {
        let mut stderr = io::stderr();
        if use_alternate_screen {
            execute!(stderr, LeaveAlternateScreen)
                .context("failed to leave alternate screen before suspend")?;
        }

        if let Err(error) = execute!(
            stderr,
            DisableMouseCapture,
            DisableFocusChange,
            DisableBracketedPaste
        ) {
            tracing::debug!(%error, "failed to disable terminal enhancements before suspend");
        }
        if let Err(error) = disable_raw_mode() {
            tracing::debug!(%error, "failed to disable raw mode before suspend");
        }

        raise(SIGTSTP).context("failed to suspend process with SIGTSTP")?;

        enable_raw_mode().context("failed to re-enable raw mode after resume")?;
        if let Err(error) = execute!(
            stderr,
            EnableBracketedPaste,
            EnableMouseCapture,
            EnableFocusChange
        ) {
            tracing::debug!(%error, "failed to re-enable terminal enhancements after resume");
        }
        if !keyboard_flags.is_empty()
            && let Err(error) = execute!(stderr, PushKeyboardEnhancementFlags(keyboard_flags))
        {
            tracing::debug!(%error, "failed to restore keyboard enhancement flags after resume");
        }
        if use_alternate_screen {
            execute!(stderr, EnterAlternateScreen)
                .context("failed to re-enter alternate screen after resume")?;
            terminal.clear().map_err(|error| {
                anyhow::anyhow!("failed to clear terminal after resume: {}", error)
            })?;
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

fn handle_inline_command(
    terminal: &mut Terminal<impl Backend>,
    session: &mut Session,
    inputs: &mut EventListener,
    event_channels: &EventChannels,
    command: InlineCommand,
) -> Result<()> {
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
            terminal
                .clear()
                .map_err(|e| anyhow::anyhow!("failed to clear terminal for redraw: {}", e))?;
            session.handle_command(InlineCommand::ForceRedraw);
        }
        cmd => {
            session.handle_command(cmd);
        }
    }

    Ok(())
}

/// Maximum number of commands to drain per turn to prevent unbounded latency
/// under load. Ensures input events and redraws are not starved by a flood of
/// background commands (e.g., during tool execution or PTY output bursts).
const MAX_COMMANDS_PER_TURN: usize = 16;

/// Render immediately if the session is dirty and the event loop is not paused.
///
/// Placing this call right after input processing (same wakeup) eliminates an
/// extra loop iteration between keypress and screen update — the single highest-
/// leverage latency improvement per Dan Luu's terminal latency research.
fn render_if_dirty<B: Backend>(
    terminal: &mut Terminal<B>,
    session: &mut Session,
    event_channels: &EventChannels,
    cursor_steady: &mut bool,
) -> Result<()> {
    if event_channels
        .rx_paused
        .load(Ordering::Acquire)
        || !session.take_redraw()
    {
        return Ok(());
    }

    let desired_steady = session.use_steady_cursor();
    if desired_steady != *cursor_steady {
        let style = if desired_steady {
            SetCursorStyle::SteadyBlock
        } else {
            SetCursorStyle::DefaultUserShape
        };
        execute!(io::stderr(), style)
            .context("failed to update cursor style for inline session")?;
        *cursor_steady = desired_steady;
    }

    terminal
        .draw(|frame| session.render(frame))
        .map_err(|e| anyhow::anyhow!("failed to draw inline session: {}", e))?;
    Ok(())
}

pub(super) struct DriveRuntimeOptions {
    pub(super) event_callback: Option<InlineEventCallback>,
    pub(super) focus_callback: Option<FocusChangeCallback>,
    pub(super) use_alternate_screen: bool,
    pub(super) keyboard_flags: crossterm::event::KeyboardEnhancementFlags,
}

pub(super) async fn drive_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    session: &mut Session,
    commands: &mut UnboundedReceiver<InlineCommand>,
    events: &UnboundedSender<InlineEvent>,
    inputs: &mut EventListener,
    event_channels: EventChannels,
    runtime_options: DriveRuntimeOptions,
) -> Result<()> {
    #[cfg(not(unix))]
    let _ = (
        runtime_options.use_alternate_screen,
        runtime_options.keyboard_flags,
    );

    let mut cursor_steady = false;
    'main: loop {
        // Drain a bounded number of pending commands to prevent unbounded latency
        // under load (e.g., during heavy PTY output or tool execution).
        for _ in 0..MAX_COMMANDS_PER_TURN {
            match commands.try_recv() {
                Ok(command) => {
                    handle_inline_command(terminal, session, inputs, &event_channels, command)?;
                }
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

        // Render if dirty (catches command-driven changes)
        render_if_dirty(terminal, session, &event_channels, &mut cursor_steady)?;

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
                        handle_focus_change_event(&event, runtime_options.focus_callback.as_ref());

                        #[cfg(unix)]
                        if is_suspend_shortcut(&event) {
                            if let Err(error) = suspend_to_shell(
                                terminal,
                                session,
                                inputs,
                                &event_channels,
                                runtime_options.use_alternate_screen,
                                runtime_options.keyboard_flags,
                            ) {
                                tracing::warn!(%error, "failed to suspend inline session");
                            }
                            continue 'main;
                        }

                        // Skip event processing if the TUI is suspended (e.g., external editor is running)
                        if !event_channels.rx_paused.load(Ordering::Acquire) {
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
                                    runtime_options
                                        .event_callback
                                        .as_ref()
                                        .map(|callback| callback.as_ref()),
                                );
                            }

                            // Process all other pending events, coalescing scroll events when safe
                            while let Ok(next_event) = inputs.try_recv() {
                                match next_event {
                                    TerminalEvent::Crossterm(evt) => {
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
                                                scroll_accum = ScrollAccumulator::default();
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
                                        // Ticks are handled by the main loop's redraw check
                                    }
                                }
                            }

                            // Apply any remaining accumulated scroll events
                            if scroll_accum.has_scroll() {
                                scroll_accum.apply(session);
                            }
                        }

                        // Render immediately after input batch — same wakeup, no
                        // extra loop iteration. This is the single highest-leverage
                        // change for keypress-to-screen latency.
                        render_if_dirty(terminal, session, &event_channels, &mut cursor_steady)?;
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
            command = commands.recv() => {
                match command {
                    Some(command) => {
                        handle_inline_command(terminal, session, inputs, &event_channels, command)?;
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

#[cfg(all(test, unix))]
mod tests {
    use super::is_suspend_shortcut;
    use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn ctrl_z_is_suspend_shortcut() {
        let event = CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert!(is_suspend_shortcut(&event));
    }

    #[test]
    fn plain_z_is_not_suspend_shortcut() {
        let event = CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert!(!is_suspend_shortcut(&event));
    }
}
