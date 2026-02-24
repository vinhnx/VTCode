//! Integration module to bridge the modern TUI with the existing Session-based UI

use crate::config::types::UiSurfacePreference;
use crate::config::{KeyboardProtocolConfig, keyboard_protocol_to_flags};
use crate::ui::tui::log::{clear_tui_log_sender, register_tui_log_sender, set_log_theme_name};
use crate::ui::tui::session::Session;
use crate::ui::tui::types::{InlineCommand, InlineEvent, InlineEventCallback, InlineTheme};
use anyhow::Result;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::SetTitle;
use tokio::sync::mpsc;

use super::modern_tui::{Event, ModernTui};

/// Terminal title displayed when VT Code TUI is active
const TERMINAL_TITLE: &str = "> VT Code";

pub struct ModernTuiConfig {
    pub theme: InlineTheme,
    pub placeholder: Option<String>,
    pub surface_preference: UiSurfacePreference,
    pub inline_rows: u16,
    pub show_logs: bool,
    pub log_theme: Option<String>,
    pub event_callback: Option<InlineEventCallback>,
    pub keyboard_protocol: KeyboardProtocolConfig,
}

pub async fn run_modern_tui(
    mut command_rx: mpsc::UnboundedReceiver<InlineCommand>,
    event_tx: mpsc::UnboundedSender<InlineEvent>,
    config: ModernTuiConfig,
) -> Result<()> {
    // Create a new ModernTui instance
    let keyboard_flags = keyboard_protocol_to_flags(&config.keyboard_protocol);
    let mut tui = ModernTui::new()?
        .tick_rate(4.0)
        .frame_rate(30.0)
        .mouse(true)
        .paste(true)
        .keyboard_flags(keyboard_flags);

    // Create the session
    let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
    set_log_theme_name(config.log_theme.clone());
    register_tui_log_sender(log_tx);
    let mut session = Session::new_with_logs(
        config.theme,
        config.placeholder,
        config.inline_rows,
        config.show_logs,
    );
    session.set_log_receiver(log_rx);

    // Enter the TUI
    tui.enter().await?;

    // Set custom terminal title
    execute!(std::io::stderr(), SetTitle(TERMINAL_TITLE))
        .unwrap_or_else(|_| tracing::debug!("failed to set terminal title"));

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
                                // Update animation state on a tick when needed
                                if session.use_steady_cursor() || session.thinking_spinner.is_active {
                                    session.handle_tick();
                                }
                            }
                            Event::Render => {
                                if session.use_steady_cursor() || session.thinking_spinner.is_active {
                                    session.handle_tick();
                                }
                                // Draw the session to the terminal with performance tracking
                                let start = std::time::Instant::now();
                                tui.terminal.draw(|frame| {
                                    session.render(frame);
                                })?;
                                let duration = start.elapsed();

                                // Warn if frame rendering exceeds 60 FPS budget (16.67ms)
                                if duration > std::time::Duration::from_millis(16) {
                                    tracing::warn!(
                                        "Slow frame render: {:?} (target: <16ms for 60 FPS)",
                                        duration
                                    );
                                }
                            }
                            Event::Resize(_width, height) => {
                                // Handle resize by telling the session about new dimensions
                                session.apply_view_rows(height);
                            }
                            Event::Key(key_event) => {
                                if let Some(inline_event) = session.process_key(key_event) {
                                    // Send event to agent
                                    let _ = event_tx.send(inline_event.clone());

                                    // Also call the callback if present
                                    if let Some(ref callback) = config.event_callback {
                                        callback(&inline_event);
                                    }

                                    // Handle special events like submit
                                    if let InlineEvent::Submit(_) = inline_event {
                                        session.mark_dirty();
                                    }
                                }
                            }
                            Event::Mouse(mouse_event) => {
                                match mouse_event.kind {
                                    MouseEventKind::ScrollDown => {
                                        session.scroll_line_down();
                                    }
                                    MouseEventKind::ScrollUp => {
                                        session.scroll_line_up();
                                    }
                                    MouseEventKind::Down(MouseButton::Left) => {
                                        session.mouse_selection.start_selection(
                                            mouse_event.column,
                                            mouse_event.row,
                                        );
                                        session.handle_transcript_click(mouse_event);
                                        session.mark_dirty();
                                    }
                                    MouseEventKind::Drag(MouseButton::Left) => {
                                        session.mouse_selection.update_selection(
                                            mouse_event.column,
                                            mouse_event.row,
                                        );
                                        session.mark_dirty();
                                    }
                                    MouseEventKind::Up(MouseButton::Left) => {
                                        session.mouse_selection.finish_selection(
                                            mouse_event.column,
                                            mouse_event.row,
                                        );
                                        session.mark_dirty();
                                    }
                                    _ => {}
                                }
                            }
                            Event::Paste(content) => {
                                session.insert_paste_text(&content);
                                session.check_file_reference_trigger();
                                session.mark_dirty();
                            }
                            Event::FocusGained => {
                                // Handle focus gained - update notification system to prevent notifications
                                crate::notifications::set_global_terminal_focused(true);
                            }
                            Event::FocusLost => {
                                // Handle focus lost - allow notifications to be sent
                                crate::notifications::set_global_terminal_focused(false);
                            }
                            Event::Init => {
                                // Initial setup after TUI is entered with performance tracking
                                let start = std::time::Instant::now();
                                tui.terminal.draw(|frame| {
                                    session.render(frame);
                                })?;
                                let duration = start.elapsed();
                                tracing::debug!("Initial frame render: {:?}", duration);
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

    // Clear terminal title on exit
    let _ = execute!(std::io::stderr(), SetTitle(""));

    clear_tui_log_sender();

    Ok(())
}

/// Helper function to create a modern TUI session that can be used similarly to the existing one
pub fn spawn_modern_session(config: ModernTuiConfig) -> Result<super::InlineSession> {
    // Initialize panic hook to restore terminal state if a panic occurs
    crate::ui::tui::panic_hook::init_panic_hook();

    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        // Create a guard to mark TUI as initialized during the session
        let _guard = crate::ui::tui::panic_hook::TuiPanicGuard::new();

        if let Err(error) = run_modern_tui(command_rx, event_tx, config).await {
            tracing::error!(%error, "modern inline session terminated unexpectedly");
        }
    });

    Ok(super::InlineSession {
        handle: super::InlineHandle { sender: command_tx },
        events: event_rx,
    })
}
