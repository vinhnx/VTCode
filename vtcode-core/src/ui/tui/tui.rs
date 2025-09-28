use std::io::{self, IsTerminal};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::{Backend, TermionBackend},
};
use termion::{
    event::Event as TermionEvent, input::TermRead, raw::IntoRawMode, screen::IntoAlternateScreen,
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
const ALTERNATE_SCREEN_ERROR: &str = "failed to enter alternate inline screen";

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

struct TerminalSurface {
    rows: u16,
    alternate: bool,
}

impl TerminalSurface {
    fn detect(preference: UiSurfacePreference, inline_rows: u16) -> Result<Self> {
        let fallback_rows = inline_rows.max(1);
        let stdout_is_terminal = io::stdout().is_terminal();
        let resolved_rows = if stdout_is_terminal {
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

        let resolved_rows = resolved_rows.max(1);
        let use_alternate = match preference {
            UiSurfacePreference::Alternate => stdout_is_terminal,
            UiSurfacePreference::Inline => false,
            UiSurfacePreference::Auto => stdout_is_terminal,
        };

        if use_alternate && !stdout_is_terminal {
            tracing::debug!("alternate surface requested but stdout is not a tty");
        }

        Ok(Self {
            rows: resolved_rows,
            alternate: use_alternate && stdout_is_terminal,
        })
    }

    fn rows(&self) -> u16 {
        self.rows
    }

    fn use_alternate(&self) -> bool {
        self.alternate
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
    let mut session = Session::new(theme, placeholder, surface.rows());
    let mut inputs = InputListener::spawn();

    if surface.use_alternate() {
        let stdout = io::stdout()
            .into_raw_mode()
            .context("failed to prepare inline terminal")?;
        let screen = stdout
            .into_alternate_screen()
            .context(ALTERNATE_SCREEN_ERROR)?;
        let backend = TermionBackend::new(screen);
        let mut terminal =
            Terminal::new(backend).context("failed to initialize inline terminal")?;
        prepare_terminal(&mut terminal)?;
        drive_terminal(
            &mut terminal,
            &mut session,
            &mut commands,
            &events,
            &mut inputs,
        )
        .await?;
        finalize_terminal(&mut terminal)?;
    } else {
        let stdout = io::stdout()
            .into_raw_mode()
            .context("failed to prepare inline terminal")?;
        let backend = TermionBackend::new(stdout);
        let mut terminal =
            Terminal::new(backend).context("failed to initialize inline terminal")?;
        prepare_terminal(&mut terminal)?;
        drive_terminal(
            &mut terminal,
            &mut session,
            &mut commands,
            &events,
            &mut inputs,
        )
        .await?;
        finalize_terminal(&mut terminal)?;
    }

    Ok(())
}

async fn drive_terminal<B: Backend>(
    terminal: &mut Terminal<B>,
    session: &mut Session,
    commands: &mut UnboundedReceiver<InlineCommand>,
    events: &UnboundedSender<InlineEvent>,
    inputs: &mut InputListener,
) -> Result<()> {
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
            terminal
                .draw(|frame| session.render(frame))
                .context("failed to draw inline session")?;
        }

        if session.should_exit() {
            break 'main;
        }

        tokio::select! {
            result = inputs.recv() => {
                match result {
                    Some(event) => {
                        session.handle_event(event, events);
                        if session.take_redraw() {
                            terminal
                                .draw(|frame| session.render(frame))
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

    Ok(())
}

fn prepare_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    terminal
        .hide_cursor()
        .context("failed to hide inline cursor")?;
    terminal
        .clear()
        .context("failed to clear inline terminal")?;
    Ok(())
}

fn finalize_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    terminal
        .show_cursor()
        .context("failed to show cursor after inline session")?;
    terminal
        .clear()
        .context("failed to clear inline terminal after session")?;
    terminal
        .flush()
        .context("failed to flush inline terminal after session")?;
    Ok(())
}
