#[cfg(unix)]
use anyhow::Context;
use anyhow::Result;

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
/// `process::exit` while the async handler hadn't finished shutting down,
/// leaving the terminal half-restored and leaking escape codes.
///
/// `SIGTERM` is still handled here as an emergency fallback because the process
/// may not have a running Tokio reactor to observe it through the async path.
/// Exits with 143 (128 + SIGTERM), the standard supervisor-kill status.
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
                let _ = crate::tui::ui::tui::panic_hook::restore_tui();
                vtcode_commons::trace_flush::flush_trace_log();
                // Clear feedback on the emergency path: without this a
                // supervisor `kill` leaves only a bare prompt and the user
                // cannot tell whether the terminal was restored cleanly.
                // `restore_tui()` already cleared the `^C`/escape line, so a
                // leading CR+LF keeps this notice on its own row even when
                // output processing is still off.
                eprintln!("\r\nReceived SIGTERM — terminal restored, exiting.");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let _ = std::io::Write::flush(&mut std::io::stderr());
                std::process::exit(143);
            }
        });

        Ok(Self { handle, thread: Some(thread) })
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
            // Bounded join: `close()` wakes the signal thread, so it normally
            // exits within milliseconds — but an unbounded `join()` here runs
            // on every normal `run_tui` return and would park the fullscreen
            // teardown (and shell return) if the platform ever ignores the
            // close. Spin briefly, then detach: the process is exiting and the
            // thread's only remaining job (emergency SIGTERM restore) stays
            // valid detached.
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
            while !thread.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if thread.is_finished() {
                let _ = thread.join();
            } else {
                tracing::warn!("SIGTERM handler thread did not exit after close; detaching");
                drop(thread);
            }
        }
    }

    #[cfg(not(unix))]
    fn drop(&mut self) {}
}
