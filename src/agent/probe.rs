//! Terminal palette probe coordination.
//!
//! The OSC color probe ([`probe_and_cache_terminal_palette_harmony`]) does
//! blocking I/O on `/dev/tty` with up to a 50 ms timeout.  Starting it
//! synchronously in `run_single_agent_loop` delays the first TUI render by
//! that full duration.
//!
//! Instead, [`start_terminal_palette_probe`] spawns the probe on a blocking
//! thread early during bootstrap so it overlaps with startup-context
//! resolution (config loading, auth probing, theme determination). The
//! typeable shell awaits it before crossterm owns the TTY (bounded ~190 ms
//! worst case, usually instant since startup already overlapped it), so
//! late `OSC 10/11/4` replies cannot win the `/dev/tty` read race against
//! the TUI event loop and leak as `10;rgb:...` keystrokes; the vendored
//! crossterm `parse_osc` backstop swallows stragglers from slow terminals.
//! `note_crossterm_raw_mode()` is still called before spawning the TUI so a
//! late probe `RawModeGuard` restore cannot undo crossterm raw mode, and
//! [`await_terminal_palette_probe`] after spawn settles theme before the
//! first model turn.
//!
//! # Correctness guarantees
//!
//! - **Race-safe**: a `PROBE_DONE` atomic flag is checked before waiting on
//!   the [`Notify`], so completion is observed even if the probe finishes
//!   before the awaiter registers its waiter.
//! - **Panic-safe**: a [`CompletionGuard`] sets `PROBE_DONE` and calls
//!   `notify_one()` in its `Drop`, so the awaiter is always released even
//!   if the probe panics inside `spawn_blocking`.
//! - **Fallback**: if the probe was never started (an unanticipated code
//!   path that still reaches the agent loop), [`await_terminal_palette_probe`]
//!   runs it synchronously, preserving the original behavior.
//! - **Timeout**: the OSC probe itself uses a 50 ms read timeout so a silent
//!   terminal cannot stall background drain for long.
//!
//! [`probe_and_cache_terminal_palette_harmony`]:
//!     vtcode_core::utils::terminal_color_probe::probe_and_cache_terminal_palette_harmony

use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;
use vtcode_core::utils::terminal_color_probe::probe_and_cache_terminal_palette_harmony;

static PROBE_NOTIFY: LazyLock<Notify> = LazyLock::new(Notify::new);
static PROBE_STARTED: AtomicBool = AtomicBool::new(false);
static PROBE_DONE: AtomicBool = AtomicBool::new(false);

/// Start the terminal palette probe on a blocking thread.
///
/// Uses the provided runtime handle so it can be called from synchronous
/// bootstrap code that is not inside a tokio runtime context.  Safe to call
/// multiple times — only the first call actually spawns the task.
pub(crate) fn start_terminal_palette_probe(handle: &tokio::runtime::Handle) {
    if PROBE_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    handle.spawn_blocking(|| {
        // Ensure completion is signalled even if the probe panics.
        // `Drop` runs during unwind before tokio catches the task panic,
        // so the awaiter is never left hanging.
        struct CompletionGuard;
        impl Drop for CompletionGuard {
            fn drop(&mut self) {
                PROBE_DONE.store(true, Ordering::Release);
                PROBE_NOTIFY.notify_one();
            }
        }
        let _guard = CompletionGuard;
        probe_and_cache_terminal_palette_harmony();
    });
}

/// Wait for the terminal palette probe to complete if it was started.
///
/// Returns immediately when the probe was not started (non-interactive
/// command) or has already completed.  If the probe was not started at all
/// (an unanticipated path that still reaches the agent loop), runs it
/// synchronously to preserve the original behavior.
pub(crate) async fn await_terminal_palette_probe() {
    if !PROBE_STARTED.load(Ordering::SeqCst) {
        // Fallback: probe was not started early.  Run it now to preserve
        // the original synchronous behavior.  This should not happen in
        // practice because bootstrap pre-starts the probe for all
        // interactive commands.
        probe_and_cache_terminal_palette_harmony();
        return;
    }
    wait_for_started_terminal_palette_probe().await;
}

/// Wait for a probe that was started during bootstrap without running a new
/// probe when startup has already failed before the probe was scheduled.
pub(crate) async fn finish_terminal_palette_probe() {
    if PROBE_STARTED.load(Ordering::SeqCst) {
        wait_for_started_terminal_palette_probe().await;
    }
}

async fn wait_for_started_terminal_palette_probe() {
    if PROBE_DONE.load(Ordering::Acquire) {
        return;
    }
    // Loop to guard against any missed notification; recheck the completion
    // flag after each wake.  `notify_one` retains a permit so the common
    // case (probe done before we get here) returns immediately.
    loop {
        PROBE_NOTIFY.notified().await;
        if PROBE_DONE.load(Ordering::Acquire) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PROBE_DONE, PROBE_STARTED, await_terminal_palette_probe, start_terminal_palette_probe};

    // Reset globals between tests.  These are process-global one-shot
    // flags, so parallel tests in the same binary can race on them.  The
    // tests below are designed to pass regardless of interleaving: the
    // fallback test just needs PROBE_STARTED == false at check time, and
    // the start test has a generous timeout so even a reset by another
    // test merely causes it to take the fallback path (still a pass).
    fn reset_globals() {
        PROBE_STARTED.store(false, std::sync::atomic::Ordering::SeqCst);
        PROBE_DONE.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    #[tokio::test]
    async fn await_without_start_runs_probe_synchronously() {
        reset_globals();
        // When the probe was never started, await should fall back to
        // running it synchronously and return (not hang).
        tokio::time::timeout(std::time::Duration::from_secs(5), await_terminal_palette_probe())
            .await
            .expect("fallback probe should not hang");
    }

    #[tokio::test]
    async fn await_returns_after_start_completes() {
        reset_globals();
        // Use the test runtime's own handle — no separate runtime to drop.
        start_terminal_palette_probe(&tokio::runtime::Handle::current());
        tokio::time::timeout(std::time::Duration::from_secs(5), await_terminal_palette_probe())
            .await
            .expect("probe should complete and release the awaiter");
        assert!(PROBE_DONE.load(std::sync::atomic::Ordering::Acquire));
    }
}
