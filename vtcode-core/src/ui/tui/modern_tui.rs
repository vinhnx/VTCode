use std::io;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use futures::future::FutureExt;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
        Event as CrosstermEvent, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        self, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        supports_keyboard_enhancement,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use anyhow::{Context, Result};

use crate::config::types::UiSurfacePreference;

// Check if nix is available for Unix-specific signal handling
#[cfg(unix)]
use nix::sys::signal;

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
    pub terminal: ratatui::Terminal<CrosstermBackend<std::io::Stderr>>,
    pub task: tokio::task::JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx: mpsc::UnboundedReceiver<Event>,
    pub event_tx: mpsc::UnboundedSender<Event>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}

// A trait to allow both old and new TUI implementations to work with the same interface
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
    fn next_event(&mut self) -> std::pin::Pin<Box<dyn futures::Future<Output = Option<Self::Event>> + Send>>;
}

impl ModernTui {
    pub fn new() -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancellation_token = CancellationToken::new();
        let init_tx = event_tx.clone();
        let task = tokio::spawn(async move {
            let _ = init_tx.send(Event::Init);
        });

        Ok(Self {
            terminal: ratatui::Terminal::new(CrosstermBackend::new(std::io::stderr()))?,
            task,
            cancellation_token,
            event_rx,
            event_tx,
            frame_rate: 60.0,
            tick_rate: 4.0,
            mouse: false,
            paste: false,
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
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stderr();
        execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
        if self.mouse {
            execute!(stdout, event::EnableMouseCapture)
                .context("failed to enable mouse capture")?;
        }
        if self.paste {
            execute!(stdout, EnableBracketedPaste)
                .context("failed to enable bracketed paste")?;
        }
        
        // Enable focus change events if supported
        let _ = execute!(stdout, EnableFocusChange);

        // Enable keyboard enhancements if supported
        if supports_keyboard_enhancement().unwrap_or(false) {
            let _ = execute!(stdout, PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
            ));
        }

        let tick_delay = std::time::Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = std::time::Duration::from_secs_f64(1.0 / self.frame_rate);
        self.task = tokio::spawn({
            let cancellation_token = self.cancellation_token.clone();
            let event_tx = self.event_tx.clone();
            async move {
                let mut reader = crossterm::event::EventStream::new();
                let mut tick_interval = tokio::time::interval(tick_delay);
                let mut render_interval = tokio::time::interval(render_delay);
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
                                            event_tx.send(Event::Key(key)).unwrap();
                                        }
                                        CrosstermEvent::Mouse(mouse) => {
                                            event_tx.send(Event::Mouse(mouse)).unwrap();
                                        }
                                        CrosstermEvent::Resize(x, y) => {
                                            event_tx.send(Event::Resize(x, y)).unwrap();
                                        }
                                        CrosstermEvent::FocusGained => {
                                            event_tx.send(Event::FocusGained).unwrap();
                                        }
                                        CrosstermEvent::FocusLost => {
                                            event_tx.send(Event::FocusLost).unwrap();
                                        }
                                        CrosstermEvent::Paste(s) => {
                                            event_tx.send(Event::Paste(s)).unwrap();
                                        }
                                    }
                                }
                                Some(Err(_)) => {
                                    event_tx.send(Event::Error).unwrap();
                                }
                                None => {
                                    event_tx.send(Event::Closed).unwrap();
                                    break;
                                }
                            }
                        },
                        _ = tick_delay => {
                            event_tx.send(Event::Tick).unwrap();
                        },
                        _ = render_delay => {
                            event_tx.send(Event::Render).unwrap();
                        },
                    }
                }
            }
        });
        Ok(())
    }

    pub async fn exit(&mut self) -> Result<()> {
        self.cancellation_token.cancel();
        let _ = self.task.await;
        if terminal::is_raw_mode_enabled().unwrap_or(false) {
            terminal::disable_raw_mode().context("failed to disable raw mode")?;
        }
        execute!(io::stderr(), LeaveAlternateScreen)
            .context("failed to leave alternate screen")?;
        if self.mouse {
            execute!(io::stderr(), event::DisableMouseCapture)
                .context("failed to disable mouse capture")?;
        }
        execute!(io::stderr(), DisableBracketedPaste)
            .context("failed to disable bracketed paste")?;
        
        // Disable focus change events if they were enabled
        let _ = execute!(io::stderr(), DisableFocusChange);
        
        // Pop keyboard enhancements if they were pushed
        let _ = execute!(io::stderr(), PopKeyboardEnhancementFlags);

        Ok(())
    }

    pub async fn suspend(&mut self) -> Result<()> {
        self.cancellation_token.cancel();
        let _ = self.task.await;
        if terminal::is_raw_mode_enabled().unwrap_or(false) {
            terminal::disable_raw_mode().context("failed to disable raw mode")?;
        }
        execute!(io::stderr(), LeaveAlternateScreen)
            .context("failed to leave alternate screen")?;
        
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
        // Enable raw mode and enter alternate screen again
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stderr(), EnterAlternateScreen)
            .context("failed to enter alternate screen")?;
        if self.mouse {
            execute!(io::stderr(), event::EnableMouseCapture)
                .context("failed to enable mouse capture")?;
        }
        if self.paste {
            execute!(io::stderr(), EnableBracketedPaste)
                .context("failed to enable bracketed paste")?;
        }
        
        // Enable focus change and keyboard enhancements
        let _ = execute!(io::stderr(), EnableFocusChange);
        if supports_keyboard_enhancement().unwrap_or(false) {
            let _ = execute!(io::stderr(), PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
            ));
        }

        let tick_delay = std::time::Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = std::time::Duration::from_secs_f64(1.0 / self.frame_rate);
        self.task = tokio::spawn({
            let cancellation_token = self.cancellation_token.clone();
            let event_tx = self.event_tx.clone();
            async move {
                let mut reader = crossterm::event::EventStream::new();
                let mut tick_interval = tokio::time::interval(tick_delay);
                let mut render_interval = tokio::time::interval(render_delay);
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
                                            event_tx.send(Event::Key(key)).unwrap();
                                        }
                                        CrosstermEvent::Mouse(mouse) => {
                                            event_tx.send(Event::Mouse(mouse)).unwrap();
                                        }
                                        CrosstermEvent::Resize(x, y) => {
                                            event_tx.send(Event::Resize(x, y)).unwrap();
                                        }
                                        CrosstermEvent::FocusGained => {
                                            event_tx.send(Event::FocusGained).unwrap();
                                        }
                                        CrosstermEvent::FocusLost => {
                                            event_tx.send(Event::FocusLost).unwrap();
                                        }
                                        CrosstermEvent::Paste(s) => {
                                            event_tx.send(Event::Paste(s)).unwrap();
                                        }
                                    }
                                }
                                Some(Err(_)) => {
                                    event_tx.send(Event::Error).unwrap();
                                }
                                None => {
                                    event_tx.send(Event::Closed).unwrap();
                                    break;
                                }
                            }
                        },
                        _ = tick_delay => {
                            event_tx.send(Event::Tick).unwrap();
                        },
                        _ = render_delay => {
                            event_tx.send(Event::Render).unwrap();
                        },
                    }
                }
            }
        });
        Ok(())
    }
}

impl Drop for ModernTui {
    fn drop(&mut self) {
        // Don't panic if called from a thread where tokio runtime is not available
        if tokio::runtime::Handle::try_current().is_ok() {
            // Use a timeout to avoid hanging indefinitely
            let _ = tokio::time::timeout(std::time::Duration::from_millis(100), async {
                self.cancellation_token.cancel();
                let _ = self.task.await;
            }).now_or_never();
        }

        // Ensure cleanup even if async cleanup fails
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
    }
}
