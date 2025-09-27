use anyhow::{Context, Result};
use crossterm::event::{Event as CrosstermEvent, EventStream};
use futures::StreamExt;
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};
use std::io;
use std::panic;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing;

use crate::config::types::UiSurfacePreference;

mod events;
mod render;
mod state;
mod ui;
mod utils;

pub use state::{
    RatatuiCommand, RatatuiEvent, RatatuiHandle, RatatuiMessageKind, RatatuiSegment,
    RatatuiSession, RatatuiTextStyle, RatatuiTheme,
};
pub use utils::{convert_style, parse_tui_color, theme_from_styles};

use state::{RatatuiLoop, TerminalGuard, TerminalSurface};
use utils::create_ticker;

pub fn spawn_session(
    theme: RatatuiTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
) -> Result<RatatuiSession> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        // Set up panic handler to ensure terminal cleanup
        let _original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            // Force terminal cleanup on panic
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
            // Note: We don't restore the original hook as this is a panic handler
            // The process is likely terminating anyway
            eprintln!("TUI panic occurred: {:?}", panic_info);
        }));

        if let Err(err) =
            run_ratatui(command_rx, event_tx, theme, placeholder, surface_preference).await
        {
            tracing::error!(error = ?err, "ratatui session terminated unexpectedly");
        }
    });

    Ok(RatatuiSession {
        handle: RatatuiHandle { sender: command_tx },
        events: event_rx,
    })
}

async fn run_ratatui(
    commands: UnboundedReceiver<RatatuiCommand>,
    events: UnboundedSender<RatatuiEvent>,
    theme: RatatuiTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
) -> Result<()> {
    let surface = TerminalSurface::detect(surface_preference)
        .context("failed to resolve terminal surface")?;
    let mut stdout = io::stdout();
    let backend = CrosstermBackend::new(&mut stdout);
    let mut terminal = match surface {
        TerminalSurface::Alternate => {
            Terminal::new(backend).context("failed to initialize ratatui terminal")?
        }
        TerminalSurface::Inline { rows } => Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(rows),
            },
        )
        .context("failed to initialize ratatui terminal")?,
    };
    let _guard =
        TerminalGuard::activate(surface).context("failed to configure terminal for ratatui")?;
    terminal
        .clear()
        .context("failed to clear terminal for ratatui")?;

    let mut app = RatatuiLoop::new(theme, placeholder);
    let mut command_rx = commands;
    let mut event_stream = EventStream::new();
    let mut redraw = true;
    let mut ticker = create_ticker();

    loop {
        if app.drain_command_queue(&mut command_rx) {
            redraw = true;
        }

        if redraw {
            // Use the stable 0.14.1 approach - simple redraw without frame rate limiting
            terminal
                .draw(|frame| app.draw(frame))
                .context("failed to draw ratatui frame")?;
            redraw = false;
        }

        if app.should_exit() {
            break;
        }


        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    Some(command) => {
                        if app.handle_command(command) {
                            redraw = true;
                        }
                    }
                    None => {
                        tracing::debug!("command channel closed, exiting TUI");
                        app.set_should_exit();
                    }
                }
            }
            event = event_stream.next() => {
                match event {
                    Some(Ok(evt)) => {
                        if matches!(evt, CrosstermEvent::Resize(_, _)) {
                            terminal
                                .autoresize()
                                .context("failed to autoresize terminal viewport")?;
                        }
                        if app.handle_event(evt, &events)? {
                            redraw = true;
                        }
                    }
                    Some(Err(_)) => {
                        redraw = true;
                    }
                    None => {}
                }
            }
            _ = ticker.tick() => {
                if app.needs_tick() {
                    redraw = true;
                }
            }
        }

        if app.should_exit() {
            break;
        }
    }

    terminal.show_cursor().ok();
    terminal
        .clear()
        .context("failed to clear terminal after ratatui session")?;

    Ok(())
}
