//! Emergency process termination that does not run `exit` handlers.
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

/// Terminate the process immediately with `code`, skipping `exit` handlers.
///
/// No Rust destructors, `atexit` handlers, or thread-local destructors run
/// after this call. Callers must flush any buffered output they care about
/// *before* calling it.
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
    use std::ffi::CString;
    use std::process::Command;
    use std::sync::OnceLock;

    const CHILD_ENV: &str = "VTCODE_EMERGENCY_EXIT_TEST_CHILD";
    const MARKER_ENV: &str = "VTCODE_EMERGENCY_EXIT_MARKER_PATH";
    const TEST_NAME: &str = "process_exit::tests::emergency_exit_skips_atexit_handlers";

    static MARKER_PATH: OnceLock<CString> = OnceLock::new();

    /// Runs only if libc `exit` invokes `atexit` handlers. Its absence proves
    /// `emergency_exit` used `_exit`.
    #[expect(unsafe_code, reason = "test-only atexit handler writing a marker with libc calls")]
    extern "C" fn write_marker_on_exit() {
        let Some(path) = MARKER_PATH.get() else {
            return;
        };
        // SAFETY: `path` is a nul-terminated CString initialized before the
        // handler was registered; open/write/close take validated arguments.
        unsafe {
            let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC, 0o600);
            if fd >= 0 {
                let byte = [b'x'];
                let _ = libc::write(fd, byte.as_ptr().cast(), 1);
                let _ = libc::close(fd);
            }
        }
    }

    #[test]
    #[expect(unsafe_code, reason = "test-only atexit registration to verify _exit semantics")]
    fn emergency_exit_skips_atexit_handlers() {
        let marker = std::env::temp_dir().join(format!("vtcode-emergency-exit-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);

        if std::env::var_os(CHILD_ENV).is_some() {
            let path = std::env::var_os(MARKER_ENV).expect("marker path in child");
            let _ = MARKER_PATH.get_or_init(|| CString::new(path.as_encoded_bytes()).expect("nul-free marker path"));
            // SAFETY: the handler is a `extern "C" fn`, never unwinds, and only
            // calls libc with arguments validated above.
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
        let _ = std::fs::remove_file(&marker);
    }
}
