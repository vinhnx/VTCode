use anyhow::{Context, Result};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_PRIVILEGE_NOT_HELD,
    GetLastError,
};
#[cfg(windows)]
use windows_sys::Win32::System::{
    SystemServices::{
        PROCESS_MITIGATION_DYNAMIC_CODE_POLICY, PROCESS_MITIGATION_DYNAMIC_CODE_POLICY_0,
        PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY,
        PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY_0, PROCESS_MITIGATION_IMAGE_LOAD_POLICY,
        PROCESS_MITIGATION_IMAGE_LOAD_POLICY_0,
    },
    Threading::{
        ProcessDynamicCodePolicy, ProcessExtensionPointDisablePolicy, ProcessImageLoadPolicy,
        SetProcessMitigationPolicy,
    },
};

#[cfg(windows)]
const PROCESS_MITIGATION_DYNAMIC_CODE_POLICY_PROHIBIT_DYNAMIC_CODE: u32 = 0x0000_0001;
#[cfg(windows)]
const PROCESS_MITIGATION_DYNAMIC_CODE_POLICY_ALLOW_THREAD_OPT_OUT: u32 = 0x0000_0002;
#[cfg(windows)]
const PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY_DISABLE_EXTENSION_POINTS: u32 = 0x0000_0001;
#[cfg(windows)]
const PROCESS_MITIGATION_IMAGE_LOAD_POLICY_NO_REMOTE_IMAGES: u32 = 0x0000_0001;
#[cfg(windows)]
const PROCESS_MITIGATION_IMAGE_LOAD_POLICY_NO_LOW_MANDATORY_LABEL_IMAGES: u32 = 0x0000_0002;
#[cfg(windows)]
const PROCESS_MITIGATION_IMAGE_LOAD_POLICY_PREFER_SYSTEM32_IMAGES: u32 = 0x0000_0004;

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

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
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
    let mut dynamic_code_policy = PROCESS_MITIGATION_DYNAMIC_CODE_POLICY {
        Anonymous: PROCESS_MITIGATION_DYNAMIC_CODE_POLICY_0 {
            Flags: PROCESS_MITIGATION_DYNAMIC_CODE_POLICY_PROHIBIT_DYNAMIC_CODE
                | PROCESS_MITIGATION_DYNAMIC_CODE_POLICY_ALLOW_THREAD_OPT_OUT,
        },
    };
    apply_mitigation_policy(
        ProcessDynamicCodePolicy,
        &mut dynamic_code_policy,
        "dynamic code",
    )?;

    let mut extension_point_policy = PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY {
        Anonymous: PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY_0 {
            Flags: PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY_DISABLE_EXTENSION_POINTS,
        },
    };
    apply_mitigation_policy(
        ProcessExtensionPointDisablePolicy,
        &mut extension_point_policy,
        "extension point",
    )?;

    let mut image_load_policy = PROCESS_MITIGATION_IMAGE_LOAD_POLICY {
        Anonymous: PROCESS_MITIGATION_IMAGE_LOAD_POLICY_0 {
            Flags: PROCESS_MITIGATION_IMAGE_LOAD_POLICY_NO_REMOTE_IMAGES
                | PROCESS_MITIGATION_IMAGE_LOAD_POLICY_NO_LOW_MANDATORY_LABEL_IMAGES
                | PROCESS_MITIGATION_IMAGE_LOAD_POLICY_PREFER_SYSTEM32_IMAGES,
        },
    };
    apply_mitigation_policy(ProcessImageLoadPolicy, &mut image_load_policy, "image load")?;

    Ok(())
}

#[cfg(windows)]
fn apply_mitigation_policy<T>(
    policy: windows_sys::Win32::System::Threading::PROCESS_MITIGATION_POLICY,
    data: &mut T,
    name: &str,
) -> Result<()> {
    let result = unsafe {
        SetProcessMitigationPolicy(
            policy,
            data as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<T>(),
        )
    };

    if result == 0 {
        let error = unsafe { GetLastError() };
        let io_error = std::io::Error::from_raw_os_error(error as i32);

        match error {
            ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => {
                tracing::warn!(
                    "skipping unsupported Windows {name} process mitigation: {io_error}"
                );
                return Ok(());
            }
            ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD => {
                tracing::warn!(
                    "insufficient privileges to enable Windows {name} process mitigation: {io_error}"
                );
                return Ok(());
            }
            _ => {
                return Err(io_error)
                    .with_context(|| format!("failed to enable {name} process mitigation"));
            }
        }
    }

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
