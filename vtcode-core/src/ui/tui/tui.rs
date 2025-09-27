use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use termion::{event::Event as TermionEvent, input::TermRead, raw::IntoRawMode, terminal_size};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::config::{constants::ui, types::UiSurfacePreference};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineTheme},
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
    mut commands: UnboundedReceiver<InlineCommand>,
    events: UnboundedSender<InlineEvent>,
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
) -> Result<()> {
    let surface = TerminalSurface::detect(surface_preference, inline_rows)?;
    let mut stdout = io::stdout()
        .into_raw_mode()
        .context("failed to enable raw mode")?;
    write!(stdout, "{}{}", termion::cursor::Hide, termion::clear::All)?;
    stdout.flush().ok();

    let mut session = Session::new(theme, placeholder, surface.rows());
    let mut inputs = spawn_input_listener();

    'main: loop {
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
            session
                .render(&mut stdout)
                .context("failed to draw inline session")?;
        }

        if session.should_exit() {
            break 'main;
        }

        tokio::select! {
            result = inputs.recv() => {
                match result {
                    Some(event) => {
                        session.handle_event(event, &events);
                        if session.take_redraw() {
                            session
                                .render(&mut stdout)
                                .context("failed to draw inline session")?;
                        }
                    }
                    None => {
                        if commands.is_closed() {
                            break 'main;
                        }
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(INPUT_POLL_INTERVAL_MS)) => {}
        }

        if session.should_exit() {
            break 'main;
        }
    }

    write!(stdout, "{}{}", termion::cursor::Show, termion::clear::All)?;
    stdout.flush().ok();

    Ok(())
}
