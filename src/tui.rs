use std::ops::{Deref, DerefMut};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::cursor;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend as Backend;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Events that can be emitted by the terminal event handler
#[derive(Clone, Debug)]
pub enum Event {
    /// Initial event, sent when the TUI starts
    Init,
    /// Request to quit the application
    Quit,
    /// Error occurred in event handler
    Error,
    /// Event channel closed
    Closed,
    /// Tick event (at tick_rate frequency)
    Tick,
    /// Render event (at frame_rate frequency)
    Render,
    /// Terminal focus gained
    FocusGained,
    /// Terminal focus lost
    FocusLost,
    /// Text pasted to terminal
    Paste(String),
    /// Key pressed
    Key(KeyEvent),
    /// Mouse event
    Mouse(MouseEvent),
    /// Terminal resized
    Resize(u16, u16),
}

/// Terminal UI handler with event-driven architecture
///
/// Provides a modular approach to terminal management with:
/// - Raw mode and alternate screen handling
/// - Async event stream with crossterm
/// - Configurable tick and frame rates
/// - Mouse and bracketed paste support
/// - Graceful shutdown via cancellation tokens
pub struct Tui {
    pub terminal: ratatui::Terminal<Backend<std::io::Stderr>>,
    pub task: JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx: UnboundedReceiver<Event>,
    pub event_tx: UnboundedSender<Event>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}

impl Tui {
    /// Create a new TUI instance with default settings
    pub fn new() -> Result<Self> {
        let tick_rate = 4.0;
        let frame_rate = 60.0;
        let terminal = ratatui::Terminal::new(Backend::new(std::io::stderr()))?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancellation_token = CancellationToken::new();
        let task = tokio::spawn(async {});
        let mouse = false;
        let paste = false;

        Ok(Self {
            terminal,
            task,
            cancellation_token,
            event_rx,
            event_tx,
            frame_rate,
            tick_rate,
            mouse,
            paste,
        })
    }

    /// Set the tick rate (events per second)
    pub fn tick_rate(mut self, tick_rate: f64) -> Self {
        self.tick_rate = tick_rate;
        self
    }

    /// Set the frame rate (renders per second)
    pub fn frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    /// Enable or disable mouse capture
    pub fn mouse(mut self, mouse: bool) -> Self {
        self.mouse = mouse;
        self
    }

    /// Enable or disable bracketed paste mode
    pub fn paste(mut self, paste: bool) -> Self {
        self.paste = paste;
        self
    }

    /// Start the event handler task
    fn start(&mut self) {
        let tick_delay = Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = Duration::from_secs_f64(1.0 / self.frame_rate);

        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let _cancellation_token = self.cancellation_token.clone();
        let _event_tx = self.event_tx.clone();

        self.task = tokio::spawn(async move {
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);

            let _ = _event_tx.send(Event::Init);

            loop {
                let tick_delay_fut = tick_interval.tick();
                let render_delay_fut = render_interval.tick();
                let event_fut = tokio::task::spawn_blocking(|| crossterm::event::read());

                tokio::select! {
                    _ = _cancellation_token.cancelled() => {
                        break;
                    }
                    _ = tick_delay_fut => {
                        let _ = _event_tx.send(Event::Tick);
                    }
                    _ = render_delay_fut => {
                        let _ = _event_tx.send(Event::Render);
                    }
                    result = event_fut => {
                        match result {
                            Ok(Ok(evt)) => {
                                match evt {
                                    CrosstermEvent::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            let _ = _event_tx.send(Event::Key(key));
                                        }
                                    }
                                    CrosstermEvent::Mouse(mouse) => {
                                        let _ = _event_tx.send(Event::Mouse(mouse));
                                    }
                                    CrosstermEvent::Resize(x, y) => {
                                        let _ = _event_tx.send(Event::Resize(x, y));
                                    }
                                    CrosstermEvent::FocusLost => {
                                        let _ = _event_tx.send(Event::FocusLost);
                                    }
                                    CrosstermEvent::FocusGained => {
                                        let _ = _event_tx.send(Event::FocusGained);
                                    }
                                    CrosstermEvent::Paste(s) => {
                                        let _ = _event_tx.send(Event::Paste(s));
                                    }
                                }
                            }
                            _ => {
                                let _ = _event_tx.send(Event::Error);
                            }
                        }
                    }
                }
            }
        });
    }

    /// Stop the event handler task gracefully
    pub fn stop(&self) -> Result<()> {
        self.cancel();
        let mut counter = 0;
        while !self.task.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > 50 {
                self.task.abort();
            }
            if counter > 100 {
                tracing::error!("Failed to abort task in 100 milliseconds for unknown reason");
                break;
            }
        }
        Ok(())
    }

    /// Enter terminal raw mode and alternate screen
    pub fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stderr(), EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            crossterm::execute!(std::io::stderr(), EnableMouseCapture)?;
        }
        if self.paste {
            crossterm::execute!(std::io::stderr(), EnableBracketedPaste)?;
        }
        self.start();
        Ok(())
    }

    /// Exit terminal raw mode and alternate screen
    pub fn exit(&mut self) -> Result<()> {
        self.stop()?;
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.flush()?;
            if self.paste {
                crossterm::execute!(std::io::stderr(), DisableBracketedPaste)?;
            }
            if self.mouse {
                crossterm::execute!(std::io::stderr(), DisableMouseCapture)?;
            }
            crossterm::execute!(std::io::stderr(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    /// Cancel the event handler task
    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    /// Suspend the TUI (for SIGTSTP on Unix)
    #[cfg(not(windows))]
    pub fn suspend(&mut self) -> Result<()> {
        self.exit()?;
        signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP)?;
        Ok(())
    }

    /// Resume the TUI after suspension
    pub fn resume(&mut self) -> Result<()> {
        self.enter()?;
        Ok(())
    }

    /// Get the next event from the event channel
    pub async fn next(&mut self) -> Option<Event> {
        self.event_rx.recv().await
    }
}

impl Default for Tui {
    fn default() -> Self {
        Self::new().expect("Failed to create default Tui instance")
    }
}

impl Deref for Tui {
    type Target = ratatui::Terminal<Backend<std::io::Stderr>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.exit();
    }
}

/// Helper trait for suspending TUI to launch external applications
pub trait ExternalAppLauncher {
    /// Temporarily suspend TUI, run a closure with terminal released, then resume
    /// 
    /// This pattern is essential for launching external editors/git clients while maintaining
    /// proper terminal state. Follows the Ratatui recipe:
    /// https://ratatui.rs/recipes/apps/spawn-vim/
    /// 
    /// The sequence is:
    /// 1. Stop event handler
    /// 2. Leave alternate screen
    /// 3. Drain pending events (critical!)
    /// 4. Disable raw mode
    /// 5. Run closure (external app can use terminal freely)
    /// 6. Re-enable raw mode
    /// 7. Re-enter alternate screen
    /// 8. Clear terminal (removes artifacts)
    /// 9. Restart event handler
    async fn with_suspended_tui<F, T>(&mut self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>;
}

impl ExternalAppLauncher for Tui {
    async fn with_suspended_tui<F, T>(&mut self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        // 1. Stop event handler (but keep state for resume)
        self.stop()?;

        // 2. Leave alternate screen
        crossterm::execute!(std::io::stderr(), LeaveAlternateScreen)
            .context("failed to leave alternate screen")?;

        // 3. Drain any pending crossterm events BEFORE disabling raw mode
        // CRITICAL: This prevents the external app from receiving garbage input like
        // terminal capability responses or buffered keystrokes sent to the TUI.
        while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }

        // 4. Disable raw mode
        crossterm::terminal::disable_raw_mode()
            .context("failed to disable raw mode")?;

        // 5. Run the closure (external app can use terminal freely)
        let result = f();

        // 6. Re-enable raw mode
        crossterm::terminal::enable_raw_mode()
            .context("failed to re-enable raw mode")?;

        // 7. Re-enter alternate screen
        crossterm::execute!(std::io::stderr(), EnterAlternateScreen)
            .context("failed to re-enter alternate screen")?;

        // 8. Clear terminal to remove any artifacts
        // This prevents ANSI escape codes from external apps' background color requests
        // from appearing in the TUI.
        crossterm::execute!(
            std::io::stderr(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )
        .context("failed to clear terminal")?;

        // 9. Restart event handler
        self.start();

        result
    }
}
