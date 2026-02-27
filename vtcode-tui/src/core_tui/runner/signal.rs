use anyhow::{Context, Result};

#[cfg(unix)]
use signal_hook::consts::signal::{SIGINT, SIGTERM};
#[cfg(unix)]
use signal_hook::iterator::Signals;

pub(super) struct SignalCleanupGuard {
    #[cfg(unix)]
    handle: signal_hook::iterator::Handle,
    #[cfg(unix)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SignalCleanupGuard {
    #[cfg(unix)]
    pub(super) fn new() -> Result<Self> {
        let mut signals =
            Signals::new([SIGINT, SIGTERM]).context("failed to register signal handlers")?;
        let handle = signals.handle();
        let thread = std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = crate::ui::tui::panic_hook::restore_tui();
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
