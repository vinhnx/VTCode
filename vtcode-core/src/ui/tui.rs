use anyhow::{Context, Result};
use crossterm::event::{Event as CrosstermEvent, EventStream};
use futures::StreamExt;
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};
use std::io;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

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

pub fn spawn_session(theme: RatatuiTheme, placeholder: Option<String>) -> Result<RatatuiSession> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        if let Err(err) = run_ratatui(command_rx, event_tx, theme, placeholder).await {
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
) -> Result<()> {
    let surface = TerminalSurface::detect().context("failed to resolve terminal surface")?;
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
            terminal
                .draw(|frame| app.draw(frame))
                .context("failed to draw ratatui frame")?;
            redraw = false;
        }

        if app.should_exit() {
            break;
        }

        tokio::select! {
            biased;
            cmd = command_rx.recv() => {
                if let Some(command) = cmd {
                    if app.handle_command(command) {
                        redraw = true;
                    }
                } else {
                    app.set_should_exit();
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
