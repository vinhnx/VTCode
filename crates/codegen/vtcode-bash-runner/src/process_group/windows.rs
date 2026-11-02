//! Windows process absence is determined by a handle, never localized taskkill text.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

/// Open a non-inheritable synchronization handle. Missing PIDs are already clean;
/// permission failures remain errors. The owned handle is closed on every path.
#[expect(
    unsafe_code,
    reason = "Windows process handles require the Win32 FFI; handles are validated and RAII-owned"
)]
fn open_process(pid: u32) -> io::Result<Option<OwnedHandle>> {
    if pid == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "process ID must be nonzero"));
    }
    // SAFETY: OpenProcess accepts scalar arguments; no pointers are supplied.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER.cast_signed()) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    // SAFETY: OpenProcess returned a valid, uniquely owned, non-inheritable
    // handle. OwnedHandle closes it exactly once, including on error returns.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

#[expect(
    unsafe_code,
    reason = "A zero-time Win32 wait queries an owned synchronization handle"
)]
fn has_exited(process: &OwnedHandle) -> io::Result<bool> {
    // SAFETY: The borrowed handle remains valid for this zero-time wait and
    // was opened with PROCESS_SYNCHRONIZE access.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        status => Err(io::Error::other(format!("unexpected process wait status {status}"))),
    }
}

/// Kill a live process tree without writing helper output to the caller's
/// protocol streams. A concurrently exited process is successful cleanup.
pub(super) fn kill_process(pid: u32) -> io::Result<()> {
    let Some(process) = open_process(pid)? else {
        return Ok(());
    };
    if has_exited(&process)? {
        return Ok(());
    }
    let output = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()?;
    if output.status.success() || has_exited(&process)? {
        Ok(())
    } else {
        // Helper output may contain unrelated process names or private paths.
        Err(io::Error::other(format!("taskkill failed for process {pid}: {}", output.status)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn cleanup_accepts_exited_child_and_repeated_cleanup() {
        let mut child = Command::new("cmd")
            .args(["/C", "exit 0"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        kill_process(pid).unwrap();
        kill_process(pid).unwrap();
    }

    #[test]
    fn cleanup_terminates_live_child() {
        let mut child = Command::new("ping")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let process = open_process(child.id()).unwrap().unwrap();
        assert!(!has_exited(&process).unwrap());
        let result = kill_process(child.id());
        if result.is_err() {
            let _ = child.kill();
        }
        let status = child.wait().unwrap();
        result.unwrap();
        assert!(!status.success());
        assert!(has_exited(&process).unwrap());
    }

    #[test]
    fn cleanup_accepts_absent_pid() {
        // Windows process IDs are aligned; this value cannot name a process.
        kill_process(u32::MAX).unwrap();
    }

    #[test]
    fn cleanup_rejects_zero_instead_of_claiming_absence() {
        assert_eq!(kill_process(0).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
