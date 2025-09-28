use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use termion::{
    cursor,
    event::Event as TermionEvent,
    input::TermRead,
    raw::{IntoRawMode, RawTerminal},
    terminal_size,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

use crate::config::{constants::ui, types::UiSurfacePreference};

use super::{
    session::Session,
    types::{InlineCommand, InlineEvent, InlineTheme},
};

const INLINE_FALLBACK_ROWS: u16 = ui::DEFAULT_INLINE_VIEWPORT_ROWS;
const INPUT_POLL_INTERVAL_MS: u64 = 16;

struct CursorGuard {
    stdout: RawTerminal<io::Stdout>,
    restored: bool,
}

impl CursorGuard {
    fn new() -> io::Result<Self> {
        let mut stdout = io::stdout().into_raw_mode()?;
        write!(stdout, "{}{}", cursor::Hide, termion::clear::All)?;
        stdout.flush().ok();
        Ok(Self {
            stdout,
            restored: false,
        })
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.restored {
            write!(self.stdout, "{}{}", cursor::Show, termion::clear::All)?;
            self.stdout.flush().ok();
            self.restored = true;
        }
        Ok(())
    }
}

impl Drop for CursorGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

impl Write for CursorGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stdout.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}

struct InputListener {
    receiver: UnboundedReceiver<TermionEvent>,
    shutdown: Option<mpsc::Sender<()>>,
}

impl InputListener {
    fn spawn() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut stdin = termion::async_stdin().events();

            loop {
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                match stdin.next() {
                    Some(Ok(event)) => {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                    Some(Err(error)) => {
                        tracing::debug!(%error, "failed to read termion event");
                        break;
                    }
                    None => {
                        if tx.is_closed() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(INPUT_POLL_INTERVAL_MS));
                    }
                }
            }
        });

        Self {
            receiver: rx,
            shutdown: Some(shutdown_tx),
        }
    }

    async fn recv(&mut self) -> Option<TermionEvent> {
        self.receiver.recv().await
    }
}

impl Drop for InputListener {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

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

pub async fn run_tui(
    mut commands: UnboundedReceiver<InlineCommand>,
    events: UnboundedSender<InlineEvent>,
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
) -> Result<()> {
    let surface = TerminalSurface::detect(surface_preference, inline_rows)?;
    let mut stdout = CursorGuard::new().context("failed to prepare inline terminal")?;

    let mut session = Session::new(theme, placeholder, surface.rows());
    let mut inputs = InputListener::spawn();

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

    stdout
        .restore()
        .context("failed to restore cursor after inline session")?;

    Ok(())
}
