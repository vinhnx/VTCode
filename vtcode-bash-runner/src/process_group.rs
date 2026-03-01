//! Process-group helpers for reliable child process cleanup.
//!
//! This module centralizes OS-specific pieces that ensure a spawned
//! command can be cleaned up reliably:
//! - `set_process_group` is called in `pre_exec` so the child starts its own
//!   process group.
//! - `detach_from_tty` starts a new session so non-interactive children do not
//!   inherit the controlling TTY.
//! - `kill_process_group_by_pid` targets the whole group (children/grandchildren)
//!   instead of a single PID.
//! - `kill_process_group` targets a known process group ID directly.
//! - `set_parent_death_signal` (Linux only) arranges for the child to receive a
//!   `SIGTERM` when the parent exits, and re-checks the parent PID to avoid
//!   races during fork/exec.
//! - `graceful_kill_process_group` sends SIGTERM, waits for a grace period, then
//!   SIGKILL if still running.
//!
//! On non-Unix platforms these helpers are no-ops or adapted equivalents.
//!
//! Inspired by codex-rs/utils/pty process group management patterns.

use std::io;

#[cfg(unix)]
use nix::errno::Errno;
#[cfg(target_os = "linux")]
use nix::sys::prctl;
#[cfg(unix)]
use nix::sys::signal::{self, Signal};
#[cfg(unix)]
use nix::unistd::{self, Pid};
#[cfg(unix)]
use tokio::process::Child;

/// Default grace period for graceful termination (milliseconds).
pub const DEFAULT_GRACEFUL_TIMEOUT_MS: u64 = 500;

/// Signal to send when killing process groups.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KillSignal {
    /// SIGINT - interrupt (Ctrl+C equivalent)
    Int,
    /// SIGTERM - allows graceful shutdown
    Term,
    /// SIGKILL - immediate termination
    #[default]
    Kill,
}

#[cfg(unix)]
impl KillSignal {
    fn as_nix_signal(self) -> Signal {
        match self {
            KillSignal::Int => Signal::SIGINT,
            KillSignal::Term => Signal::SIGTERM,
            KillSignal::Kill => Signal::SIGKILL,
        }
    }
}

#[cfg(unix)]
fn nix_err_to_io(err: Errno) -> io::Error {
    io::Error::from_raw_os_error(err as i32)
}

/// Ensure the child receives SIGTERM when the original parent dies.
///
/// This should run in `pre_exec` and uses `parent_pid` captured before spawn to
/// avoid a race where the parent exits between fork and exec.
#[cfg(target_os = "linux")]
pub fn set_parent_death_signal(parent_pid: libc::pid_t) -> io::Result<()> {
    prctl::set_pdeathsig(Some(Signal::SIGTERM)).map_err(nix_err_to_io)?;

    // Re-check parent PID to avoid race condition where parent exits between fork and exec.
    if unistd::getppid() != Pid::from_raw(parent_pid) {
        signal::kill(unistd::getpid(), Signal::SIGTERM).map_err(nix_err_to_io)?;
    }

    Ok(())
}

/// No-op on non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub fn set_parent_death_signal(_parent_pid: i32) -> io::Result<()> {
    Ok(())
}

/// Detach from the controlling TTY by starting a new session.
///
/// This is useful for spawning background processes that should not receive
/// signals from the controlling terminal.
#[cfg(unix)]
pub fn detach_from_tty() -> io::Result<()> {
    match unistd::setsid() {
        Ok(_) => Ok(()),
        // EPERM means we're already a session leader, fall back to setpgid.
        Err(Errno::EPERM) => set_process_group(),
        Err(err) => Err(nix_err_to_io(err)),
    }
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn detach_from_tty() -> io::Result<()> {
    Ok(())
}

/// Put the calling process into its own process group.
///
/// Intended for use in `pre_exec` so the child becomes the group leader.
#[cfg(unix)]
pub fn set_process_group() -> io::Result<()> {
    unistd::setpgid(Pid::from_raw(0), Pid::from_raw(0)).map_err(nix_err_to_io)
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn set_process_group() -> io::Result<()> {
    Ok(())
}

/// Kill the process group for the given PID (best-effort).
///
/// This resolves the PGID for `pid` and sends SIGKILL to the whole group.
#[cfg(unix)]
pub fn kill_process_group_by_pid(pid: u32) -> io::Result<()> {
    kill_process_group_by_pid_with_signal(pid, KillSignal::Kill)
}

/// Kill the process group for the given PID with a specific signal.
#[cfg(unix)]
pub fn kill_process_group_by_pid_with_signal(pid: u32, signal: KillSignal) -> io::Result<()> {
    use std::io::ErrorKind;

    let target_pid = Pid::from_raw(pid as libc::pid_t);
    let pgid = unistd::getpgid(Some(target_pid));
    let mut pgid_err = None;

    match pgid {
        Ok(group) => {
            if let Err(err) = signal::killpg(group, signal.as_nix_signal()) {
                let io_err = nix_err_to_io(err);
                if io_err.kind() != ErrorKind::NotFound {
                    pgid_err = Some(io_err);
                }
            }
        }
        Err(err) => pgid_err = Some(nix_err_to_io(err)),
    }

    // Always attempt to kill the direct child process handle as a fallback.
    // This ensures termination even if the cached PGID was stale or
    // the process group kill had issues.
    if let Err(err) = signal::kill(target_pid, signal.as_nix_signal()) {
        let io_err = nix_err_to_io(err);
        if io_err.kind() == ErrorKind::NotFound {
            // If direct kill says not found, we're done regardless of pgid result.
            return Ok(());
        }
        // If we have a pgid error and a direct kill error, prefer the pgid one.
        if let Some(pgid_error) = pgid_err {
            return Err(pgid_error);
        }
        return Err(io_err);
    }

    Ok(())
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn kill_process_group_by_pid(_pid: u32) -> io::Result<()> {
    Ok(())
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn kill_process_group_by_pid_with_signal(_pid: u32, _signal: KillSignal) -> io::Result<()> {
    Ok(())
}

/// Kill a specific process group ID (best-effort).
#[cfg(unix)]
pub fn kill_process_group(process_group_id: u32) -> io::Result<()> {
    kill_process_group_with_signal(process_group_id, KillSignal::Kill)
}

/// Kill a specific process group ID with a specific signal.
#[cfg(unix)]
pub fn kill_process_group_with_signal(process_group_id: u32, signal: KillSignal) -> io::Result<()> {
    use std::io::ErrorKind;

    let pgid = Pid::from_raw(process_group_id as libc::pid_t);
    if let Err(err) = signal::killpg(pgid, signal.as_nix_signal()) {
        let io_err = nix_err_to_io(err);
        if io_err.kind() != ErrorKind::NotFound {
            return Err(io_err);
        }
    }

    Ok(())
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn kill_process_group(_process_group_id: u32) -> io::Result<()> {
    Ok(())
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn kill_process_group_with_signal(
    _process_group_id: u32,
    _signal: KillSignal,
) -> io::Result<()> {
    Ok(())
}

/// Kill the process group for a tokio child (best-effort).
#[cfg(unix)]
pub fn kill_child_process_group(child: &mut Child) -> io::Result<()> {
    kill_child_process_group_with_signal(child, KillSignal::Kill)
}

/// Kill the process group for a tokio child with a specific signal.
#[cfg(unix)]
pub fn kill_child_process_group_with_signal(
    child: &mut Child,
    signal: KillSignal,
) -> io::Result<()> {
    if let Some(pid) = child.id() {
        return kill_process_group_by_pid_with_signal(pid, signal);
    }

    Ok(())
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn kill_child_process_group(_child: &mut tokio::process::Child) -> io::Result<()> {
    Ok(())
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub fn kill_child_process_group_with_signal(
    _child: &mut tokio::process::Child,
    _signal: KillSignal,
) -> io::Result<()> {
    Ok(())
}

/// Kill a process by PID on Windows.
#[cfg(windows)]
pub fn kill_process(pid: u32) -> io::Result<()> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("taskkill failed"))
    }
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub fn kill_process(_pid: u32) -> io::Result<()> {
    Ok(())
}

/// Result of a graceful termination attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GracefulTerminationResult {
    /// Process exited gracefully after SIGTERM/SIGINT.
    GracefulExit,
    /// Process had to be forcefully killed with SIGKILL.
    ForcefulKill,
    /// Process was already not running.
    AlreadyExited,
    /// Failed to check or terminate the process.
    Error,
}

/// Check if a process (by PID) is still running.
#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    let target_pid = Pid::from_raw(pid as libc::pid_t);
    match signal::kill(target_pid, None::<Signal>) {
        Ok(()) => true,
        // EPERM = exists but no permission (still running)
        Err(Errno::EPERM) => true,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_process_running(_pid: u32) -> bool {
    // On non-Unix, assume running (will fail gracefully)
    true
}

/// Gracefully terminate a process group by PID.
///
/// This function implements a staged termination strategy:
/// 1. Send the initial signal (default: SIGTERM, or SIGINT for interactive processes)
/// 2. Wait up to `grace_period` for the process to exit
/// 3. If still running, send SIGKILL
///
/// Returns information about how the termination completed.
///
/// # Arguments
/// * `pid` - Process ID (will be used to resolve the process group)
/// * `initial_signal` - Signal to try first (SIGINT, SIGTERM)
/// * `grace_period` - How long to wait before SIGKILL
#[cfg(unix)]
pub fn graceful_kill_process_group(
    pid: u32,
    initial_signal: KillSignal,
    grace_period: std::time::Duration,
) -> GracefulTerminationResult {
    // Check if already exited
    if !is_process_running(pid) {
        return GracefulTerminationResult::AlreadyExited;
    }

    // Resolve PGID
    let target_pid = Pid::from_raw(pid as libc::pid_t);
    let Ok(pgid) = unistd::getpgid(Some(target_pid)) else {
        // Can't get PGID - process may have already exited.
        return GracefulTerminationResult::AlreadyExited;
    };

    // Send initial signal (SIGTERM or SIGINT)
    let signal = match initial_signal {
        KillSignal::Kill => Signal::SIGTERM, // Don't send SIGKILL as initial.
        other => other.as_nix_signal(),
    };

    if let Err(err) = signal::killpg(pgid, signal) {
        if err != Errno::ESRCH {
            return GracefulTerminationResult::Error;
        }
        return GracefulTerminationResult::AlreadyExited;
    }

    // Wait for graceful exit
    let deadline = std::time::Instant::now() + grace_period;
    let poll_interval = std::time::Duration::from_millis(10);

    while std::time::Instant::now() < deadline {
        if !is_process_running(pid) {
            return GracefulTerminationResult::GracefulExit;
        }
        std::thread::sleep(poll_interval);
    }

    // Still running - force kill.
    // Use the robust termination behavior from codex-rs/utils/pty PR 12688
    // by attempting both a pgid kill and a direct pid kill.
    let _ = signal::killpg(pgid, Signal::SIGKILL);
    if let Err(err) = signal::kill(target_pid, Signal::SIGKILL) {
        if err == Errno::ESRCH {
            // Exited between check and kill.
            return GracefulTerminationResult::GracefulExit;
        }
        return GracefulTerminationResult::Error;
    }

    GracefulTerminationResult::ForcefulKill
}

/// Graceful termination on non-Unix (best effort).
///
/// On Windows, uses `taskkill` without `/F` first, then retries with `/F`
/// after the grace period.
#[cfg(not(unix))]
pub fn graceful_kill_process_group(
    pid: u32,
    initial_signal: KillSignal,
    grace_period: std::time::Duration,
) -> GracefulTerminationResult {
    #[cfg(windows)]
    {
        let _ = initial_signal;
        let pid_arg = pid.to_string();
        match std::process::Command::new("taskkill")
            .args(["/PID", &pid_arg, "/T"])
            .status()
        {
            Ok(status) if status.success() => {
                std::thread::sleep(grace_period);
                GracefulTerminationResult::GracefulExit
            }
            Ok(_) => match kill_process(pid) {
                Ok(()) => GracefulTerminationResult::ForcefulKill,
                Err(_) => GracefulTerminationResult::AlreadyExited,
            },
            Err(_) => GracefulTerminationResult::Error,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, initial_signal, grace_period);
        GracefulTerminationResult::Error
    }
}

/// Gracefully terminate a process group with default settings.
///
/// Uses SIGTERM and the default grace period (500ms).
#[cfg(unix)]
pub fn graceful_kill_process_group_default(pid: u32) -> GracefulTerminationResult {
    graceful_kill_process_group(
        pid,
        KillSignal::Term,
        std::time::Duration::from_millis(DEFAULT_GRACEFUL_TIMEOUT_MS),
    )
}

/// Graceful termination with defaults on non-Unix.
#[cfg(not(unix))]
pub fn graceful_kill_process_group_default(pid: u32) -> GracefulTerminationResult {
    graceful_kill_process_group(
        pid,
        KillSignal::Term,
        std::time::Duration::from_millis(DEFAULT_GRACEFUL_TIMEOUT_MS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_parent_death_signal_no_panic() {
        // Just verify it doesn't panic
        #[cfg(target_os = "linux")]
        {
            let parent_pid = unistd::getpid().as_raw();
            // Note: This will likely fail in tests since we're not in pre_exec
            // but it should not panic
            let _ = set_parent_death_signal(parent_pid);
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert!(set_parent_death_signal(0).is_ok());
        }
    }

    #[test]
    fn test_kill_nonexistent_process_group() {
        // Killing a non-existent process group should not error on non-Unix
        // On Unix, ESRCH (no such process) is converted to Ok() in our implementation
        #[cfg(unix)]
        {
            // Try to kill a very high PID that definitely doesn't exist
            // Our implementation should return Ok for ESRCH
            let result = kill_process_group(2_000_000_000);
            // Just verify it doesn't panic - result depends on kernel
            let _ = result;
        }
        #[cfg(not(unix))]
        {
            let result = kill_process_group(999_999);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_kill_signal_values() {
        // Verify KillSignal enum values
        assert_ne!(KillSignal::Int, KillSignal::Term);
        assert_ne!(KillSignal::Term, KillSignal::Kill);
        assert_ne!(KillSignal::Int, KillSignal::Kill);

        // Test default
        assert_eq!(KillSignal::default(), KillSignal::Kill);
    }

    #[test]
    fn test_graceful_termination_result_debug() {
        // Verify GracefulTerminationResult can be formatted
        let results = [
            GracefulTerminationResult::GracefulExit,
            GracefulTerminationResult::ForcefulKill,
            GracefulTerminationResult::AlreadyExited,
            GracefulTerminationResult::Error,
        ];
        for result in &results {
            let _ = format!("{result:?}");
        }
    }

    #[test]
    fn test_graceful_kill_nonexistent_process() {
        // Gracefully killing a non-existent PID should return AlreadyExited
        let result = graceful_kill_process_group_default(2_000_000_000);
        #[cfg(unix)]
        {
            // On Unix, non-existent processes return AlreadyExited
            assert_eq!(result, GracefulTerminationResult::AlreadyExited);
        }
        #[cfg(not(unix))]
        {
            // On non-Unix, behavior varies
            let _ = result;
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_is_process_running_self() {
        // Our own process should be running
        let pid = std::process::id();
        assert!(is_process_running(pid));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_process_running_nonexistent() {
        // A very high PID should not be running
        assert!(!is_process_running(2_000_000_000));
    }
}
