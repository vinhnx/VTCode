//! Process hardening and security measures.
//!
//! Provides early-process hardening functions that run before `main()` via
//! linker-section constructors, plus a public `pre_main_hardening()` entry
//! point for explicit calls from `main()`.

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(unix)]
use ctor::ctor;

#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(unsafe_code)]
fn prctl_set_dumpable() -> i32 {
    // SAFETY: `prctl` is called with the documented `PR_SET_DUMPABLE` command and
    // integer arguments only. No pointers are dereferenced.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn ptrace_deny_attach() -> i32 {
    // SAFETY: `ptrace(PT_DENY_ATTACH, ...)` is the documented macOS hardening call.
    // The null pointer argument is not dereferenced by this request type.
    unsafe { libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0) }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn remove_env_var(key: OsString) {
    // SAFETY: Caller must ensure this runs during single-threaded early process
    // startup, before any threads are spawned, which satisfies the environment
    // mutation safety requirement.
    unsafe { std::env::remove_var(key) }
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
    let ret_code = prctl_set_dumpable();
    if ret_code != 0 {
        eprintln!(
            "ERROR: prctl(PR_SET_DUMPABLE, 0) failed: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(PRCTL_FAILED_EXIT_CODE);
    }

    // For "defense in depth," set the core file size limit to 0.
    set_core_file_size_limit_to_zero();

    // VT Code is primarily MUSL-linked in release builds, which means that variables such
    // as LD_PRELOAD are ignored anyway, but just to be sure, clear them here.
    let ld_keys = env_keys_with_prefix(std::env::vars_os(), b"LD_");
    for key in ld_keys {
        remove_env_var(key);
    }
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn pre_main_hardening_bsd() {
    cap_stack_rlimit();

    // FreeBSD/OpenBSD: set RLIMIT_CORE to 0 and clear LD_* env vars
    set_core_file_size_limit_to_zero();

    let ld_keys = env_keys_with_prefix(std::env::vars_os(), b"LD_");
    for key in ld_keys {
        remove_env_var(key);
    }
}

#[cfg(target_os = "macos")]
fn pre_main_hardening_macos() {
    cap_stack_rlimit();

    // Prevent debuggers from attaching to this process.
    let ret_code = ptrace_deny_attach();
    if ret_code == -1 {
        eprintln!(
            "ERROR: ptrace(PT_DENY_ATTACH) failed: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(PTRACE_DENY_ATTACH_FAILED_EXIT_CODE);
    }

    // Set the core file size limit to 0 to prevent core dumps.
    set_core_file_size_limit_to_zero();

    // Remove all DYLD_* environment variables, which can be used to subvert
    // library loading.
    let dyld_keys = env_keys_with_prefix(std::env::vars_os(), b"DYLD_");
    for key in dyld_keys {
        remove_env_var(key);
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn set_core_file_size_limit_to_zero() {
    let rlim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `rlim` is fully initialized and passed by shared reference for the
    // duration of the syscall only.
    let ret_code = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
    if ret_code != 0 {
        eprintln!(
            "ERROR: setrlimit(RLIMIT_CORE) failed: {}",
            std::io::Error::last_os_error()
        );
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
#[allow(unsafe_code)]
fn cap_stack_rlimit() {
    const STACK_CAP_BYTES: u64 = 8 * 1024 * 1024;

    let mut current: libc::rlimit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `current` points to valid writable memory for the syscall to fill.
    let ret = unsafe { libc::getrlimit(libc::RLIMIT_STACK, &mut current) };
    if ret != 0 {
        return;
    }
    // Only cap if the soft limit is currently unlimited.
    if current.rlim_cur != libc::RLIM_INFINITY {
        return;
    }
    let capped = libc::rlimit {
        rlim_cur: STACK_CAP_BYTES,
        rlim_max: STACK_CAP_BYTES,
    };
    // SAFETY: `capped` is fully initialized and passed by shared reference for
    // the duration of the syscall only.
    let _ = unsafe { libc::setrlimit(libc::RLIMIT_STACK, &capped) };
}

#[cfg(windows)]
fn pre_main_hardening_windows() {
    // Windows process hardening would involve using Job Objects to limit
    // resource usage and restrict UI access, or using restricted tokens.
    // This is currently a future enhancement.
}

// ===========================================================================
// Pre-main constructors (life before main)
//
// These run before main() via linker-section constructor tables. Priority
// 0-100 is reserved for the C runtime; we use 101+ to run after libc init.
// Constraints: no heap allocation, no locks, no panics, no stdio.
// ===========================================================================

/// Post-hardening sanity: verify that core dumps are disabled on Unix.
///
/// Runs at priority 101 — after C runtime init but before `main()`.
/// This is a lightweight check that catches misconfigurations early.
#[cfg(unix)]
#[ctor(unsafe, priority = 101)]
fn verify_hardening_effectiveness() {
    use std::mem::MaybeUninit;

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        // SAFETY: rlim is zeroed and passed by mutable ref to getrlimit which
        // fills it. The syscall is infallible on valid pointers.
        #[allow(unsafe_code)]
        let mut rlim: libc::rlimit = unsafe { MaybeUninit::zeroed().assume_init() };
        // SAFETY: getrlimit fills rlim via mutable pointer; the struct is
        // zeroed above so no uninitialized memory is exposed.
        #[allow(unsafe_code)]
        let ret = unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut rlim) };
        if ret == 0 && rlim.rlim_cur != 0 {
            // Core dumps are not disabled. Write a warning directly to stderr
            // using libc (stdio may not be initialized yet).
            // SAFETY: msg is a static byte string; write is a POSIX syscall
            // that does not dereference the pointer beyond msg.len() bytes.
            #[allow(unsafe_code)]
            unsafe {
                let msg =
                    b"warning: vtcode-process-hardening: RLIMIT_CORE is not zero after hardening\n";
                libc::write(
                    libc::STDERR_FILENO,
                    msg.as_ptr() as *const libc::c_void,
                    msg.len(),
                );
            }
        }
    }
}

/// Environment sanity: warn if essential env vars are missing.
///
/// Runs at priority 102. Checks that `HOME` (and on Unix, `USER`) are set,
/// which are required for config resolution. Uses raw libc writes because
/// stdio may not be initialized yet.
#[cfg(unix)]
#[ctor(unsafe, priority = 102)]
fn check_environment_sanity() {
    let home_set = std::env::var_os("HOME").is_some();
    if !home_set {
        // SAFETY: msg is a static byte string; write is a POSIX syscall
        // that does not dereference the pointer beyond msg.len() bytes.
        #[allow(unsafe_code)]
        unsafe {
            let msg =
                b"warning: HOME environment variable is not set; config resolution may fail\n";
            libc::write(
                libc::STDERR_FILENO,
                msg.as_ptr() as *const libc::c_void,
                msg.len(),
            );
        }
    }

    // USER is not strictly required on all platforms but is a strong signal.
    #[cfg(not(target_os = "windows"))]
    {
        let user_set = std::env::var_os("USER").is_some();
        if !user_set {
            // SAFETY: msg is a static byte string; write is a POSIX syscall
            // that does not dereference the pointer beyond msg.len() bytes.
            #[allow(unsafe_code)]
            unsafe {
                let msg = b"warning: USER environment variable is not set; some features may not work correctly\n";
                libc::write(
                    libc::STDERR_FILENO,
                    msg.as_ptr() as *const libc::c_void,
                    msg.len(),
                );
            }
        }
    }
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
