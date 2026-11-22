//! Emergency process termination that skips libc `exit` cleanup on Unix.
//!
//! [`std::process::exit`] delegates to libc's `exit`, which runs `atexit`
//! handlers and thread-local destructors and tears down process-wide state Rust
//! shares with libc and the kernel — notably the main thread's alternate signal
//! stack. Calling it from a child thread races the still-running main thread:
//! a destructor can be dropped twice, or the alt stack the main thread still
//! relies on can be unmapped. VT Code's emergency exits run on a Tokio task
//! (double Ctrl+C) and on the `signal_hook` thread (SIGTERM), so both are
//! exposed to that class of bug.
//!
//! These paths have already restored the terminal and flushed their own output
//! before exiting, so they do not need libc's cleanup. On Unix we use the raw
//! `_exit` syscall to skip it entirely; other platforms keep
//! [`std::process::exit`].

/// Terminate the process immediately with `code`.
///
/// On Unix this is a raw `_exit` syscall: no Rust destructors, `atexit`
/// handlers, or thread-local destructors run after it. Other platforms fall
/// back to [`std::process::exit`] and follow its cleanup semantics. Callers
/// must flush any buffered output they care about *before* calling it.
#[cfg_attr(
    unix,
    expect(unsafe_code, reason = "raw _exit syscall skips all exit handlers by design")
)]
pub fn emergency_exit(code: i32) -> ! {
    #[cfg(unix)]
    {
        // SAFETY: `_exit` consumes only a process status, cannot return, and
        // touches no Rust-owned memory, so no invariant can be observed after
        // this point. Skipping libc cleanup is the intended behavior.
        unsafe { libc::_exit(code) }
    }
    #[cfg(not(unix))]
    {
        std::process::exit(code)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::OnceLock;

    const CHILD_ENV: &str = "VTCODE_EMERGENCY_EXIT_TEST_CHILD";
    const MARKER_ENV: &str = "VTCODE_EMERGENCY_EXIT_MARKER_PATH";
    const TEST_NAME: &str = "process_exit::tests::emergency_exit_skips_atexit_handlers";

    static MARKER_PATH: OnceLock<PathBuf> = OnceLock::new();

    /// Runs only if libc `exit` invokes `atexit` handlers. Its absence proves
    /// `emergency_exit` used `_exit`.
    extern "C" fn write_marker_on_exit() {
        let Some(path) = MARKER_PATH.get() else {
            return;
        };
        // `fs::write` reports I/O failures as `Err` rather than unwinding, so
        // this `extern "C"` callback stays non-unwinding during teardown.
        std::fs::write(path, b"x").unwrap_or_default();
    }

    #[test]
    #[expect(unsafe_code, reason = "test-only atexit registration to verify _exit semantics")]
    fn emergency_exit_skips_atexit_handlers() {
        let marker = std::env::temp_dir().join(format!("vtcode-emergency-exit-{}", std::process::id()));
        std::fs::remove_file(&marker).unwrap_or_default();

        if std::env::var_os(CHILD_ENV).is_some() {
            let path = std::env::var_os(MARKER_ENV).expect("marker path in child");
            MARKER_PATH.set(PathBuf::from(path)).unwrap_or_default();
            // SAFETY: the handler is an `extern "C" fn` that cannot unwind, and
            // `atexit` only registers it in this short-lived child process.
            assert_eq!(unsafe { libc::atexit(write_marker_on_exit) }, 0);
            emergency_exit(42);
        }

        let exe = std::env::current_exe().expect("test binary path");
        let output = Command::new(exe)
            .env(CHILD_ENV, "1")
            .env(MARKER_ENV, &marker)
            .arg("--exact")
            .arg(TEST_NAME)
            .output()
            .expect("spawn child test process");

        assert_eq!(output.status.code(), Some(42), "child stdout: {}", String::from_utf8_lossy(&output.stdout));
        assert!(!marker.exists(), "atexit handler ran; emergency_exit must use _exit on Unix");
        std::fs::remove_file(&marker).unwrap_or_default();
    }
}
