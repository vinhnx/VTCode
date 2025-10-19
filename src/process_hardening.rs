use anyhow::{Context, Result};

/// Apply process hardening safeguards to the current process.
///
/// These measures are inspired by the Codex process hardening sequence and are
/// intended to run as early as possible in the binary entry point.
pub fn apply_process_hardening() -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        harden_linux().context("failed to apply Linux process hardening")?;
    }

    #[cfg(target_os = "macos")]
    {
        harden_macos().context("failed to apply macOS process hardening")?;
    }

    #[cfg(windows)]
    {
        harden_windows().context("failed to apply Windows process hardening")?;
    }

    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const PRCTL_FAILED_EXIT_CODE: i32 = 5;

#[cfg(target_os = "macos")]
const PTRACE_DENY_ATTACH_FAILED_EXIT_CODE: i32 = 6;

#[cfg(unix)]
const SET_RLIMIT_CORE_FAILED_EXIT_CODE: i32 = 7;

#[cfg(any(target_os = "linux", target_os = "android"))]
fn harden_linux() -> Result<()> {
    // Disable ptrace attach / mark process non-dumpable.
    let ret_code = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    if ret_code != 0 {
        let err = std::io::Error::last_os_error();
        tracing::error!("prctl(PR_SET_DUMPABLE, 0) failed: {err}");
        std::process::exit(PRCTL_FAILED_EXIT_CODE);
    }

    set_core_file_size_limit_to_zero()
        .context("setrlimit(RLIMIT_CORE) failed while hardening Linux process")?;

    remove_env_vars_with_prefix("LD_");

    Ok(())
}

#[cfg(target_os = "macos")]
fn harden_macos() -> Result<()> {
    let ret_code = unsafe { libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0) };
    if ret_code == -1 {
        let err = std::io::Error::last_os_error();
        tracing::error!("ptrace(PT_DENY_ATTACH) failed: {err}");
        std::process::exit(PTRACE_DENY_ATTACH_FAILED_EXIT_CODE);
    }

    set_core_file_size_limit_to_zero()
        .context("setrlimit(RLIMIT_CORE) failed while hardening macOS process")?;

    remove_env_vars_with_prefix("DYLD_");

    Ok(())
}

#[cfg(windows)]
fn harden_windows() -> Result<()> {
    // TODO: Evaluate process mitigations for Windows builds.
    Ok(())
}

#[cfg(unix)]
fn set_core_file_size_limit_to_zero() -> Result<()> {
    let rlim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };

    let ret_code = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
    if ret_code != 0 {
        let err = std::io::Error::last_os_error();
        tracing::error!("setrlimit(RLIMIT_CORE) failed: {err}");
        std::process::exit(SET_RLIMIT_CORE_FAILED_EXIT_CODE);
    }

    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn remove_env_vars_with_prefix(prefix: &str) {
    let keys: Vec<String> = std::env::vars()
        .filter_map(|(key, _)| {
            if key.starts_with(prefix) {
                Some(key)
            } else {
                None
            }
        })
        .collect();

    for key in keys {
        unsafe {
            std::env::remove_var(key);
        }
    }
}
