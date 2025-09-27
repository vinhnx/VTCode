use std::io::{self, IsTerminal};
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::TermionBackend};
use termion::{event::Event as TermionEvent, input::TermRead, raw::IntoRawMode, terminal_size};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::config::{constants::ui, types::UiSurfacePreference};

use super::{
    session::Session,
    types::{RatatuiCommand, RatatuiEvent, RatatuiTheme},
};

const INLINE_FALLBACK_ROWS: u16 = ui::DEFAULT_INLINE_VIEWPORT_ROWS;
const INPUT_POLL_INTERVAL_MS: u64 = 16;

enum TerminalSurface {
    Inline { rows: u16 },
}

impl TerminalSurface {
    fn detect(preference: UiSurfacePreference, inline_rows: u16) -> Result<Self> {
        if matches!(preference, UiSurfacePreference::Alternate) {
            tracing::debug!(
                "alternate surface requested but inline viewport is currently supported"
            );
        }

        let fallback_rows = inline_rows.max(1);
        let resolved = if io::stdout().is_terminal() {
            match terminal_size() {
                Ok((_, 0)) => fallback_rows.max(INLINE_FALLBACK_ROWS),
                Ok((_, rows)) => rows,
                Err(error) => {
                    tracing::debug!(%error, "failed to determine terminal size");
                    fallback_rows.max(INLINE_FALLBACK_ROWS)
                }
            }
        } else {
            fallback_rows.max(INLINE_FALLBACK_ROWS)
        };

        Ok(Self::Inline {
            rows: resolved.max(1),
        })
    }

    fn rows(&self) -> u16 {
        match self {
            Self::Inline { rows } => *rows,
        }
    }
}

pub fn spawn_input_listener() -> UnboundedReceiver<TermionEvent> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        for event in stdin.lock().events() {
            match event {
                Ok(event) => {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "failed to read termion event");
                    break;
                }
            }
        }
    });
    rx
}

pub async fn run_tui(
    mut commands: UnboundedReceiver<RatatuiCommand>,
    events: UnboundedSender<RatatuiEvent>,
    theme: RatatuiTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
) -> Result<()> {
    let surface = TerminalSurface::detect(surface_preference, inline_rows)?;
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

    let mut session = Session::new(theme, placeholder);
    let mut inputs = spawn_input_listener();

    loop {
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    session.handle_command(command);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    session.request_exit();
                    break;
                }
            }
        }

        if session.take_redraw() {
            terminal
                .draw(|frame| session.draw(frame))
                .context("failed to draw ratatui frame")?;
        }

        if session.should_exit() {
            break;
        }

        tokio::select! {
            result = inputs.recv() => {
                match result {
                    Some(event) => {
                        session.handle_event(event, &events);
                        if session.take_redraw() {
                            terminal
                                .draw(|frame| session.draw(frame))
                                .context("failed to draw ratatui frame")?;
                        }
                    }
                    None => {
                        if commands.is_closed() {
                            break;
                        }
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(INPUT_POLL_INTERVAL_MS)) => {}
        }

        if session.should_exit() {
            break;
        }
    }

    terminal.show_cursor().ok();
    terminal
        .clear()
        .context("failed to clear terminal after ratatui session")?;
    terminal.flush().ok();

    Ok(())
}
