use anyhow::{Context, Result};

#[cfg(unix)]
use signal_hook::consts::signal::SIGTERM;
#[cfg(unix)]
use signal_hook::iterator::Signals;

/// Guard that performs emergency terminal restoration on `SIGTERM`.
///
/// `SIGINT` is deliberately **not** handled here because the TUI runs in raw
/// mode where Ctrl+C is delivered as a key event, not a Unix signal.  The async
/// signal handler in `session_setup/signal.rs` (via `tokio::signal::ctrl_c()`)
/// owns the Ctrl+C state machine (Cancel → Exit).  Handling `SIGINT` in *both*
/// places caused a split-brain race: this thread would call `restore_tui()` +
/// `process::exit(130)` while the async handler hadn't finished shutting down,
/// leaving the terminal half-restored and leaking escape codes.
///
/// `SIGTERM` is still handled here as an emergency fallback because the process
/// may not have a running Tokio reactor to observe it through the async path.
pub(super) struct SignalCleanupGuard {
    #[cfg(unix)]
    handle: signal_hook::iterator::Handle,
    #[cfg(unix)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SignalCleanupGuard {
    #[cfg(unix)]
    pub(super) fn new() -> Result<Self> {
        let mut signals = Signals::new([SIGTERM]).context("failed to register SIGTERM handler")?;
        let handle = signals.handle();
        let thread = std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = crate::ui::tui::panic_hook::restore_tui();
                vtcode_commons::trace_flush::flush_trace_log();
                std::process::exit(130);
            }
        });

        Ok(Self {
            handle,
            thread: Some(thread),
        })
    }

    #[cfg(not(unix))]
    pub(super) fn new() -> Result<Self> {
        Ok(Self {})
    }
}

impl Drop for SignalCleanupGuard {
    #[cfg(unix)]
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    #[cfg(not(unix))]
    fn drop(&mut self) {}
}
