use std::io;

use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::Backend,
    crossterm::{cursor::SetCursorStyle, execute},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::ui::tui::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineEventCallback},
};

use super::events::{EventChannels, EventListener, ScrollAccumulator, TerminalEvent};

/// Check if session has any modal or palette active that uses keyboard navigation
fn has_active_navigation_ui(session: &Session) -> bool {
    session.modal.is_some()
        || session.file_palette_active
        || crate::ui::tui::session::slash::slash_navigation_available(session)
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

pub(super) async fn drive_terminal<B: Backend>(
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
                        handle_inline_command(terminal, session, inputs, &event_channels, command)?;
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
                            while let Ok(next_event) = inputs.try_recv() {
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
