//! Integration module to bridge the modern TUI with the existing Session-based UI

use anyhow::Result;
use tokio::sync::mpsc;
use crate::ui::tui::session::Session;
use crate::ui::tui::types::{InlineCommand, InlineEvent, InlineEventCallback, InlineTheme};
use crate::config::types::UiSurfacePreference;

use super::modern_tui::{ModernTui, Event};

pub async fn run_modern_tui(
    mut command_rx: mpsc::UnboundedReceiver<InlineCommand>,
    event_tx: mpsc::UnboundedSender<InlineEvent>,
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    show_timeline_pane: bool,
    event_callback: Option<InlineEventCallback>,
) -> Result<()> {
    // Create a new ModernTUI instance
    let mut tui = ModernTui::new()?
        .tick_rate(4.0)
        .frame_rate(30.0)
        .mouse(true)
        .paste(true);

    // Create the session
    let mut session = Session::new(theme, placeholder, inline_rows, show_timeline_pane);

    // Enter the TUI
    tui.enter().await?;

    // Main event loop
    'main: loop {
        tokio::select! {
            // Handle external commands (from agent to UI)
            command = command_rx.recv() => {
                match command {
                    Some(inline_cmd) => {
                        session.handle_command(inline_cmd);
                        if session.should_exit() {
                            break 'main;
                        }
                    },
                    None => break, // Channel closed
                }
            },
            
            // Handle terminal events 
            maybe_event = tui.event_rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        match event {
                            Event::Quit => {
                                break 'main;
                            }
                            Event::Tick => {
                                // Update logic can go here if needed
                            }
                            Event::Render => {
                                // Draw the session to the terminal
                                tui.terminal.draw(|frame| {
                                    session.render(frame);
                                })?;
                            }
                            Event::Resize(width, height) => {
                                // Handle resize by telling the session about new dimensions
                                session.apply_view_rows(height);
                            }
                            Event::Key(key_event) => {
                                if let Some(inline_event) = session.process_key(key_event) {
                                    // Send event to agent
                                    let _ = event_tx.send(inline_event.clone());
                                    
                                    // Also call the callback if present
                                    if let Some(ref callback) = event_callback {
                                        callback(&inline_event);
                                    }
                                    
                                    // Handle special events like submit
                                    if let InlineEvent::Submit(_) = inline_event {
                                        session.mark_dirty();
                                    }
                                }
                            }
                            Event::Mouse(mouse_event) => {
                                // Handle mouse events if needed
                                match mouse_event.kind {
                                    crossterm::event::MouseEventKind::ScrollDown => {
                                        session.scroll_line_down();
                                    }
                                    crossterm::event::MouseEventKind::ScrollUp => {
                                        session.scroll_line_up();
                                    }
                                    _ => {}
                                }
                                // Redraw after mouse event
                                tui.terminal.draw(|frame| {
                                    session.render(frame);
                                })?;
                            }
                            Event::Paste(content) => {
                                session.insert_text(&content);
                            }
                            Event::FocusGained => {
                                // Handle focus gained
                            }
                            Event::FocusLost => {
                                // Handle focus lost
                            }
                            Event::Init => {
                                // Initial setup after TUI is entered
                                tui.terminal.draw(|frame| {
                                    session.render(frame);
                                })?;
                            }
                            Event::Closed => {
                                break 'main;
                            }
                            Event::Error => {
                                // Handle error event - maybe log or show error in UI
                            }
                        }
                    }
                    None => {
                        // Event channel closed, exit
                        break 'main;
                    }
                }
            }
        }

        if session.should_exit() {
            break 'main;
        }
    }

    // Exit the TUI
    tui.exit().await?;
    
    Ok(())
}

/// Helper function to create a modern TUI session that can be used similarly to the existing one
pub fn spawn_modern_session(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    show_timeline_pane: bool,
    event_callback: Option<InlineEventCallback>,
) -> Result<super::InlineSession> {
    // Initialize panic hook to restore terminal state if a panic occurs
    crate::ui::tui::panic_hook::init_panic_hook();
    
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        // Create a guard to mark TUI as initialized during the session
        let _guard = crate::ui::tui::panic_hook::TuiPanicGuard::new();
        
        if let Err(error) = run_modern_tui(
            command_rx,
            event_tx,
            theme,
            placeholder,
            surface_preference,
            inline_rows,
            show_timeline_pane,
            event_callback,
        ).await {
            tracing::error!(%error, "modern inline session terminated unexpectedly");
        }
    });

    Ok(super::InlineSession {
        handle: super::InlineHandle { sender: command_tx },
        events: event_rx,
    })
}
