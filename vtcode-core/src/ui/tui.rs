use anyhow::{Context, Result};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::TermionBackend};
use std::io;
use termion::raw::IntoRawMode;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::config::types::UiSurfacePreference;

mod session;
mod style;

pub use session::{
    RatatuiCommand, RatatuiEvent, RatatuiHandle, RatatuiMessageKind, RatatuiSegment,
    RatatuiSession, RatatuiTextStyle, RatatuiTheme,
};
pub use style::{convert_style, parse_tui_color, theme_from_styles};

use session::{RatatuiLoop, TerminalSurface};

pub fn spawn_session(
    theme: RatatuiTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
) -> Result<RatatuiSession> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
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
    mut commands: UnboundedReceiver<RatatuiCommand>,
    events: UnboundedSender<RatatuiEvent>,
    theme: RatatuiTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
) -> Result<()> {
    let surface = TerminalSurface::detect(surface_preference)?;
    let stdout = io::stdout()
        .into_raw_mode()
        .context("failed to enable raw mode")?;
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(surface.rows()),
        },
    )
    .context("failed to initialize ratatui terminal")?;
    terminal
        .clear()
        .context("failed to clear terminal for ratatui")?;

    terminal.hide_cursor().ok();

    let mut app = RatatuiLoop::new(theme, placeholder)?;

    loop {
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    let _ = app.handle_command(command);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    app.set_should_exit();
                    break;
                }
            }
        }

        if app.should_exit() {
            break;
        }

        if app.take_redraw() {
            terminal
                .draw(|frame| app.draw(frame))
                .context("failed to draw ratatui frame")?;
        }

        if app.should_exit() {
            break;
        }

        app.poll(&events)?;

        if app.should_exit() {
            break;
        }

        if app.take_redraw() {
            terminal
                .draw(|frame| app.draw(frame))
                .context("failed to draw ratatui frame")?;
        }
    }

    terminal.show_cursor().ok();
    terminal
        .clear()
        .context("failed to clear terminal after ratatui session")?;
    terminal.flush().ok();

    Ok(())
}
