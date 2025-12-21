use anyhow::{Context, Result};
use async_trait::async_trait;
use crossterm::{
    cursor,
    event::{
        self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste,
        EnableFocusChange, Event as CrosstermEvent, KeyboardEnhancementFlags, KeyEventKind,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        self, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        supports_keyboard_enhancement,
    },
};
use futures::StreamExt;
use futures::future::FutureExt;
use ratatui::backend::CrosstermBackend;
use std::{
    io,
    ops::{Deref, DerefMut},
    time::Duration,
};
use crate::ui::tui::panic_hook::TuiPanicGuard;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::error;

// Check if nix is available for Unix-specific signal handling

// Event enum that covers all terminal events following the Ratatui recipe
#[derive(Clone, Debug)]
pub enum Event {
    Init,
    Quit,
    Error,
    Closed,
    Tick,
    Render,
    FocusGained,
    FocusLost,
    Paste(String),
    Key(event::KeyEvent),
    Mouse(event::MouseEvent),
    Resize(u16, u16),
}

// Terminal User Interface (TUI) struct following the Ratatui recipe
pub struct ModernTui {
    pub terminal: ratatui::Terminal<CrosstermBackend<std::io::Stdout>>,
    pub task: tokio::task::JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx: mpsc::UnboundedReceiver<Event>,
    pub event_tx: mpsc::UnboundedSender<Event>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
    pub panic_guard: Option<TuiPanicGuard>,
}

// A trait to allow both old and new TUI implementations to work with the same interface
#[async_trait]
pub trait TuiInterface {
    type Event;
    type Error;

    async fn enter(&mut self) -> Result<(), Self::Error>;
    async fn exit(&mut self) -> Result<(), Self::Error>;
    async fn suspend(&mut self) -> Result<(), Self::Error>;
    async fn resume(&mut self) -> Result<(), Self::Error>;
    fn draw<F>(&mut self, f: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut ratatui::Frame);
    fn next_event(
        &mut self,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Option<Self::Event>> + Send>>;
}

impl ModernTui {
    pub fn new() -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancellation_token = CancellationToken::new();
        let task = tokio::spawn(async {});

        Ok(Self {
            terminal: ratatui::Terminal::new(CrosstermBackend::new(std::io::stdout()))?,
            task,
            cancellation_token,
            event_rx,
            event_tx,
            frame_rate: 60.0,
            tick_rate: 4.0,
            mouse: false,
            paste: false,
            panic_guard: None,
        })
    }

    pub fn tick_rate(mut self, tick_rate: f64) -> Self {
        self.tick_rate = tick_rate;
        self
    }

    pub fn frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    pub fn mouse(mut self, mouse: bool) -> Self {
        self.mouse = mouse;
        self
    }

    pub fn paste(mut self, paste: bool) -> Self {
        self.paste = paste;
        self
    }

    pub async fn enter(&mut self) -> Result<()> {
        self.panic_guard = Some(TuiPanicGuard::new());
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, cursor::Hide)
            .context("failed to enter alternate screen")?;
        if self.mouse {
            execute!(stdout, event::EnableMouseCapture).context("failed to enable mouse capture")?;
        }
        if self.paste {
            execute!(stdout, EnableBracketedPaste).context("failed to enable bracketed paste")?;
        }

        // Enable focus change events if supported
        let _ = execute!(stdout, EnableFocusChange);

        // Enable keyboard enhancements if supported
        if supports_keyboard_enhancement().unwrap_or(false) {
            let _ = execute!(
                stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
                )
            );
        }

        self.start_event_task();
        Ok(())
    }

    pub async fn exit(&mut self) -> Result<()> {
        self.stop().await?;
        if terminal::is_raw_mode_enabled().unwrap_or(false) {
            self.terminal.flush().context("failed to flush terminal")?;
            if self.paste {
                execute!(io::stdout(), DisableBracketedPaste)
                    .context("failed to disable bracketed paste")?;
            }
            if self.mouse {
                execute!(io::stdout(), event::DisableMouseCapture)
                    .context("failed to disable mouse capture")?;
            }
            execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)
                .context("failed to leave alternate screen")?;
            terminal::disable_raw_mode().context("failed to disable raw mode")?;
            let _ = execute!(io::stdout(), DisableFocusChange);
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }

        self.panic_guard = None;

        Ok(())
    }

    pub async fn suspend(&mut self) -> Result<()> {
        self.stop().await?;
        if terminal::is_raw_mode_enabled().unwrap_or(false) {
            let _ = self.terminal.flush();
            if self.paste {
                let _ = execute!(io::stdout(), DisableBracketedPaste);
            }
            if self.mouse {
                let _ = execute!(io::stdout(), event::DisableMouseCapture);
            }
            let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
            let _ = terminal::disable_raw_mode();
            let _ = execute!(io::stdout(), DisableFocusChange);
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }

        self.panic_guard = None;

        // Execute suspend command to allow job control (Ctrl+Z)
        #[cfg(unix)]
        {
            // Unix-specific suspend using signal
            use nix::sys::signal;
            signal::raise(signal::Signal::SIGTSTP).context("failed to send SIGTSTP")?;
        }

        Ok(())
    }

    pub async fn resume(&mut self) -> Result<()> {
        self.enter().await
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.event_rx.recv().await
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.cancel();
        let mut counter = 0;
        while !self.task.is_finished() {
            tokio::time::sleep(Duration::from_millis(1)).await;
            counter += 1;
            if counter > 50 {
                self.task.abort();
            }
            if counter > 100 {
                error!("Failed to abort event task in 100 milliseconds");
                break;
            }
        }
        Ok(())
    }

    fn start_event_task(&mut self) {
        let tick_delay = Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = Duration::from_secs_f64(1.0 / self.frame_rate);

        self.cancel();
        self.cancellation_token = CancellationToken::new();

        let cancellation_token = self.cancellation_token.clone();
        let event_tx = self.event_tx.clone();
        self.task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);
            let _ = event_tx.send(Event::Init);

            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next().fuse();

                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        break;
                    }
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    CrosstermEvent::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            let _ = event_tx.send(Event::Key(key));
                                        }
                                    }
                                    CrosstermEvent::Mouse(mouse) => {
                                        let _ = event_tx.send(Event::Mouse(mouse));
                                    }
                                    CrosstermEvent::Resize(x, y) => {
                                        let _ = event_tx.send(Event::Resize(x, y));
                                    }
                                    CrosstermEvent::FocusGained => {
                                        let _ = event_tx.send(Event::FocusGained);
                                    }
                                    CrosstermEvent::FocusLost => {
                                        let _ = event_tx.send(Event::FocusLost);
                                    }
                                    CrosstermEvent::Paste(s) => {
                                        let _ = event_tx.send(Event::Paste(s));
                                    }
                                }
                            }
                            Some(Err(_)) => {
                                let _ = event_tx.send(Event::Error);
                            }
                            None => {
                                let _ = event_tx.send(Event::Closed);
                                break;
                            }
                        }
                    },
                    _ = tick_delay => {
                        let _ = event_tx.send(Event::Tick);
                    },
                    _ = render_delay => {
                        let _ = event_tx.send(Event::Render);
                    },
                }
            }
        });
    }
}

impl Drop for ModernTui {
    fn drop(&mut self) {
        self.cancel();
        self.task.abort();

        let _ = self.terminal.flush();
        if self.paste {
            let _ = execute!(io::stderr(), DisableBracketedPaste);
        }
        if self.mouse {
            let _ = execute!(io::stderr(), event::DisableMouseCapture);
        }
        let _ = execute!(io::stderr(), LeaveAlternateScreen, cursor::Show);
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), DisableFocusChange);
        let _ = execute!(io::stderr(), PopKeyboardEnhancementFlags);
    }
}

impl Deref for ModernTui {
    type Target = ratatui::Terminal<CrosstermBackend<std::io::Stdout>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for ModernTui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}
