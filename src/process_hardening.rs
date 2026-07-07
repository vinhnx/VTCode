//! Process hardening and security measures.
//!
//! Provides early-process hardening functions called from `main()` to lock
//! down the process before untrusted code can run.

use vtcode_commons::env_lock;

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(any(target_os = "linux", target_os = "android"))]
use nix::sys::prctl;
#[cfg(unix)]
use nix::sys::resource::{Resource, getrlimit, setrlimit};

#[cfg(any(target_os = "linux", target_os = "android"))]
fn prctl_set_dumpable() -> std::io::Result<()> {
    prctl::set_dumpable(false)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{e}")))
}

/// Safe wrapper around macOS `ptrace(PT_DENY_ATTACH)`.
///
/// `PT_DENY_ATTACH` (request 31) tells the kernel to kill this process if
/// any debugger tries to attach. The call takes only integer arguments and a
/// null pointer that the kernel never dereferences for this request type.
#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "FFI call to macOS ptrace — unavoidable, but the call is safe by contract"
)]
fn ptrace_deny_attach() -> std::io::Result<()> {
    // SAFETY: `ptrace(PT_DENY_ATTACH, ...)` takes only integer arguments and a
    // null pointer that the kernel never dereferences for this request type.
    unsafe {
        let ret = libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0);
        if ret == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(unix)]
fn remove_env_var(key: OsString) {
    env_lock::remove_var(key);
}

/// Perform early process hardening as the first operation in `main()`.
///
/// Call this before any other operations, thread spawning, or heap
/// allocation to ensure the process is locked down before potential
/// adversaries can influence its state. Steps include:
/// - disabling core dumps
/// - disabling ptrace attach on Linux and macOS
/// - removing dangerous environment variables such as LD_PRELOAD and DYLD_*
pub fn pre_main_hardening() {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pre_main_hardening_linux();

    #[cfg(target_os = "macos")]
    pre_main_hardening_macos();

    #[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
    pre_main_hardening_bsd();

    #[cfg(windows)]
    pre_main_hardening_windows();

    // Post-hardening verification: check if core dumps are disabled.
    #[cfg(unix)]
    verify_hardening_effectiveness();

    // Check essential env vars are present.
    #[cfg(unix)]
    check_environment_sanity();
}

/// Verify that core dumps are disabled.
/// Runs after all platform hardening to catch misconfigurations early.
#[cfg(unix)]
fn verify_hardening_effectiveness() {
    let Ok((soft, _hard)) = getrlimit(Resource::RLIMIT_CORE) else {
        return;
    };
    if soft != 0 {
        eprintln!("warning: vtcode-process-hardening: RLIMIT_CORE is not zero after hardening");
    }
}

/// Check that essential environment variables are set.
#[cfg(unix)]
fn check_environment_sanity() {
    if std::env::var_os("HOME").is_none() {
        eprintln!("warning: HOME environment variable is not set; config resolution may fail");
    }

    if std::env::var_os("USER").is_none() {
        eprintln!(
            "warning: USER environment variable is not set; some features may not work correctly"
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const PRCTL_FAILED_EXIT_CODE: i32 = 5;

#[cfg(target_os = "macos")]
const PTRACE_DENY_ATTACH_FAILED_EXIT_CODE: i32 = 6;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
const SET_RLIMIT_CORE_FAILED_EXIT_CODE: i32 = 7;

#[cfg(any(target_os = "linux", target_os = "android"))]
fn pre_main_hardening_linux() {
    cap_stack_rlimit();

    // Disable ptrace attach / mark process non-dumpable.
    if let Err(e) = prctl_set_dumpable() {
        eprintln!("ERROR: prctl(PR_SET_DUMPABLE) failed: {e}");
        std::process::exit(PRCTL_FAILED_EXIT_CODE);
    }

    // For "defense in depth," set the core file size limit to 0.
    set_core_file_size_limit_to_zero();

    // VT Code is primarily MUSL-linked in release builds, which means that variables such
    // as LD_PRELOAD are ignored anyway, but just to be sure, clear them here.
    remove_env_vars_with_prefix(b"LD_");
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn pre_main_hardening_bsd() {
    cap_stack_rlimit();

    // FreeBSD/OpenBSD: set RLIMIT_CORE to 0 and clear LD_* env vars
    set_core_file_size_limit_to_zero();

    remove_env_vars_with_prefix(b"LD_");
}

#[cfg(target_os = "macos")]
fn pre_main_hardening_macos() {
    cap_stack_rlimit();

    // Prevent debuggers from attaching to this process.
    if let Err(e) = ptrace_deny_attach() {
        eprintln!("ERROR: ptrace(PT_DENY_ATTACH) failed: {e}");
        std::process::exit(PTRACE_DENY_ATTACH_FAILED_EXIT_CODE);
    }

    // Set the core file size limit to 0 to prevent core dumps.
    set_core_file_size_limit_to_zero();

    // Remove all DYLD_* environment variables, which can be used to subvert
    // library loading.
    remove_env_vars_with_prefix(b"DYLD_");
}

#[cfg(unix)]
fn set_core_file_size_limit_to_zero() {
    if let Err(e) = setrlimit(Resource::RLIMIT_CORE, 0, 0) {
        eprintln!("ERROR: setrlimit(RLIMIT_CORE) failed: {e}");
        std::process::exit(SET_RLIMIT_CORE_FAILED_EXIT_CODE);
    }
}

/// Cap the main thread stack size if it is currently unlimited.
///
/// This is a complementary hardening measure to Rust's built-in stack clash
/// protection (`-C probe-stack=inline`).  While probe-stack prevents attackers
/// from jumping over the kernel guard page with a single large allocation,
/// capping `RLIMIT_STACK` bounds stack resource exhaustion attacks.
///
/// Linux allows `RLIMIT_STACK` to be `RLIM_INFINITY` (unlimited).  We set it
/// to a fixed cap (8 MiB) when that is the case.  If the current stack usage
/// already exceeds the cap the call will fail harmlessly with `EINVAL`.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn cap_stack_rlimit() {
    const STACK_CAP_BYTES: u64 = 8 * 1024 * 1024;

    let Ok((soft, _hard)) = getrlimit(Resource::RLIMIT_STACK) else {
        return;
    };
    if soft != libc::RLIM_INFINITY {
        return;
    }
    let _ = setrlimit(Resource::RLIMIT_STACK, STACK_CAP_BYTES, STACK_CAP_BYTES);
}

#[cfg(windows)]
fn pre_main_hardening_windows() {
    // Windows process hardening would involve using Job Objects to limit
    // resource usage and restrict UI access, or using restricted tokens.
    // This is currently a future enhancement.
}

#[cfg(unix)]
fn env_keys_with_prefix<I>(vars: I, prefix: &[u8]) -> Vec<OsString>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    vars.into_iter()
        .filter_map(|(key, _)| {
            key.as_os_str()
                .as_bytes()
                .starts_with(prefix)
                .then_some(key)
        })
        .collect()
}

/// Remove all environment variables whose names start with the given prefix.
#[cfg(unix)]
fn remove_env_vars_with_prefix(prefix: &[u8]) {
    let keys = env_keys_with_prefix(std::env::vars_os(), prefix);
    for key in keys {
        remove_env_var(key);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn env_keys_with_prefix_handles_non_utf8_entries() {
        let non_utf8_key1 = OsStr::from_bytes(b"R\xD6DBURK").to_os_string();
        assert!(non_utf8_key1.clone().into_string().is_err());

        let non_utf8_key2 = OsString::from_vec(vec![b'L', b'D', b'_', 0xF0]);
        assert!(non_utf8_key2.clone().into_string().is_err());

        let non_utf8_value = OsString::from_vec(vec![0xF0, 0x9F, 0x92, 0xA9]);

        let keys = env_keys_with_prefix(
            vec![
                (non_utf8_key1, non_utf8_value.clone()),
                (non_utf8_key2.clone(), non_utf8_value),
            ],
            b"LD_",
        );

        assert_eq!(
            keys,
            vec![non_utf8_key2],
            "non-UTF-8 env entries with LD_ prefix should be retained"
        );
    }

    #[test]
    fn env_keys_with_prefix_filters_only_matching_keys() {
        let ld_test_var = OsStr::from_bytes(b"LD_TEST");
        let vars = vec![
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (ld_test_var.to_os_string(), OsString::from("1")),
            (OsString::from("DYLD_FOO"), OsString::from("bar")),
        ];

        let keys = env_keys_with_prefix(vars, b"LD_");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].as_os_str(), ld_test_var);
    }

    #[test]
    fn env_keys_with_prefix_returns_empty_when_no_matches_exist() {
        let vars = vec![
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (OsString::from("HOME"), OsString::from("/tmp/home")),
        ];

        let keys = env_keys_with_prefix(vars, b"LD_");
        assert!(keys.is_empty());
    }

    #[test]
    fn env_keys_with_prefix_matches_exact_prefix_and_is_case_sensitive() {
        let vars = vec![
            (OsString::from("LD_"), OsString::from("exact-prefix")),
            (OsString::from("Ld_TEST"), OsString::from("mixed-case")),
            (OsString::from("LD_PRELOAD"), OsString::from("/tmp/lib.so")),
        ];

        let keys = env_keys_with_prefix(vars, b"LD_");
        assert_eq!(
            keys,
            vec![OsString::from("LD_"), OsString::from("LD_PRELOAD")]
        );
    }

    #[test]
    fn env_keys_with_prefix_supports_dyld_prefix_filtering() {
        let vars = vec![
            (
                OsString::from("DYLD_INSERT_LIBRARIES"),
                OsString::from("/tmp/inject.dylib"),
            ),
            (OsString::from("DYLD_FOO"), OsString::from("bar")),
            (
                OsString::from("LD_PRELOAD"),
                OsString::from("/tmp/other.so"),
            ),
        ];

        let keys = env_keys_with_prefix(vars, b"DYLD_");
        assert_eq!(
            keys,
            vec![
                OsString::from("DYLD_INSERT_LIBRARIES"),
                OsString::from("DYLD_FOO")
            ]
        );
    }
}
