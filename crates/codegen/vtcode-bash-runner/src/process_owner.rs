//! Native command lifetime support for a binary-owned task cleanup endpoint.
//!
//! This is cleanup, not command admission: callers must validate the original
//! command and select its sandbox launcher before applying this helper.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Private task endpoint inherited by VT Code's managed command launchers.
pub const OWNER_SOCKET_ENV: &str = "VTCODE_COMPANION_OWNER_SOCKET";
/// Private native entry point shared with the binary-owned task launcher.
pub const HELPER_MODE: &str = "--vtcode-owned-exec";
/// Prefix of the private directory that contains the task owner endpoint.
pub const OWNER_DIRECTORY_PREFIX: &str = "vtc-";
/// Basename of the task owner endpoint inside its private directory.
pub const OWNER_SOCKET_FILE: &str = "owner";
const READY: u8 = 1;

/// Exact argv for the shipped native helper. Original argv is never interpreted
/// as shell syntax; `arg0` is restored when replacing the helper with the target.
pub struct OwnedCommand {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub owner_socket: PathBuf,
}

impl OwnedCommand {
    pub fn new(
        executable: &Path,
        owner_socket: &Path,
        program: &OsStr,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
        arg0: Option<&OsStr>,
    ) -> Self {
        let mut arguments = vec![
            OsString::from(HELPER_MODE),
            arg0.unwrap_or(program).to_owned(),
            program.to_owned(),
        ];
        arguments.extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
        Self {
            executable: executable.to_owned(),
            arguments,
            owner_socket: owner_socket.to_owned(),
        }
    }

    /// Wrap `program` when this process is itself a managed command whose
    /// private task endpoint is still present.
    ///
    /// [`OWNER_SOCKET_ENV`] is inherited from the process environment, so a
    /// spurious, stale, or forged value must not be honored: wrapping an
    /// endpoint this process does not own fails closed inside the helper and
    /// would disable every managed command. A value that is not a live
    /// [`OWNER_SOCKET_ENV`] endpoint created by this user's task supervisor is
    /// therefore ignored, and the command spawns normally.
    pub fn from_environment(
        program: &OsStr,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
        arg0: Option<&OsStr>,
    ) -> io::Result<Option<Self>> {
        let Some(socket) = std::env::var_os(OWNER_SOCKET_ENV) else {
            return Ok(None);
        };
        let socket = PathBuf::from(socket);
        if !is_owned_endpoint(&socket) {
            return Ok(None);
        }
        Ok(Some(Self::new(&std::env::current_exe()?, &socket, program, args, arg0)))
    }
}

/// Whether `socket` belongs to a live task owner created by this user.
///
/// Provenance is structural, matching how the supervisor creates the endpoint:
/// a Unix socket named [`OWNER_SOCKET_FILE`] directly inside a directory that
/// is prefixed [`OWNER_DIRECTORY_PREFIX`], owned by the current user, granted
/// owner-only permissions, and located under the system temp root. Every part
/// must hold, so a path the supervisor could not have produced is rejected.
///
/// Exposed so the binary-owned supervisor can assert that the endpoint it
/// publishes satisfies the launcher's provenance contract.
pub fn is_owned_endpoint(socket: &Path) -> bool {
    is_owned_endpoint_under(socket, &std::env::temp_dir())
}

/// [`is_owned_endpoint`] with an explicit temp root, so the containment gate is
/// testable without mutating the process environment.
fn is_owned_endpoint_under(socket: &Path, temp_root: &Path) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    if !socket.is_absolute() || socket.file_name() != Some(OsStr::new(OWNER_SOCKET_FILE)) {
        return false;
    }
    // `symlink_metadata` keeps a symlink from standing in for a real endpoint.
    let Ok(endpoint) = std::fs::symlink_metadata(socket) else {
        return false;
    };
    if !endpoint.file_type().is_socket() {
        return false;
    }
    let Some(directory) = socket.parent() else {
        return false;
    };
    let Ok(directory_metadata) = std::fs::metadata(directory) else {
        return false;
    };
    let owner_only = directory_metadata.permissions().mode() & 0o077 == 0;
    let owned_by_self = directory_metadata.is_dir() && directory_metadata.uid() == nix::unistd::geteuid().as_raw();
    if !owner_only || !owned_by_self {
        return false;
    }
    if !directory
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with(OWNER_DIRECTORY_PREFIX))
    {
        return false;
    }
    let Ok(directory) = vtcode_commons::canonicalize(directory) else {
        return false;
    };
    let temp_root = vtcode_commons::canonicalize(temp_root).unwrap_or_else(|_| temp_root.to_path_buf());
    directory.starts_with(temp_root)
}

/// Dispatch before agent environment hardening, worker threads, tracing, or the async runtime.
/// A successful helper replaces itself with the admitted program and does not
/// return. A missing/closed endpoint fails before the target can execute.
pub fn maybe_run_owned_command() -> io::Result<bool> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_none_or(|arg| arg != HELPER_MODE) {
        return Ok(false);
    }
    let arg0 = args
        .get(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target argv[0]"))?;
    let program = args
        .get(2)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target program"))?;
    if nix::unistd::getpgrp() != nix::unistd::getpid() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "owned command requires an isolated process group"));
    }
    let socket = std::env::var_os(OWNER_SOCKET_ENV)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing task owner endpoint"))?;
    let mut owner = UnixStream::connect(socket)?;
    owner.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut acknowledgement = [0];
    owner.read_exact(&mut acknowledgement)?;
    if acknowledgement != [READY] {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid task owner acknowledgement"));
    }
    // The monitor has no readiness timeout: it waits for EOF throughout the
    // command's lifetime. The two-second timeout only bounds startup above.
    owner.set_read_timeout(None)?;
    spawn_watchdog(&owner)?;
    drop(owner);
    // Ancestors may hold descriptors without CLOEXEC (observed: a system
    // kernel-control socket inherited at fd 24 flaking lifetime tests), so
    // the helper enforces a clean table itself instead of trusting ambient
    // CLOEXEC hygiene. Only stdio must survive: owner and watchdog channels
    // are already dropped above.
    close_all_above_stdio();
    let error = std::process::Command::new(program).arg0(arg0).args(args.iter().skip(3)).exec();
    Err(error)
}

/// Close every inherited descriptor above stdio, preserving 0, 1, and 2.
///
/// Call only when no Rust-owned descriptor above stdio may still be live;
/// the helper drops its owner and watchdog channels before exec, so by that
/// point every surviving fd is inherited junk. Sweeps to the process file
/// limit (floored at 256 so the checked range is always covered, capped at
/// 65536 to bound worst-case time); descriptors above a lowered limit are a
/// pathological case this best-effort sweep does not chase.
#[expect(
    unsafe_code,
    reason = "getrlimit/close take validated arguments; no Rust-owned fds above stdio are live here"
)]
fn close_all_above_stdio() {
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `getrlimit` writes only to the caller-owned `limit` out-param.
    let ceiling = if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } == 0 {
        limit.rlim_cur.clamp(256, 65536) as libc::c_int
    } else {
        8192
    };
    for fd in 3..ceiling {
        // SAFETY: `close` on an arbitrary small fd number is always sound;
        // invalid numbers fail with EBADF and stdio is out of range.
        unsafe {
            libc::close(fd);
        }
    }
}

#[expect(
    unsafe_code,
    reason = "Fork children execute only async-signal-safe syscalls and exit without Rust destructors"
)]
fn spawn_watchdog(owner: &UnixStream) -> io::Result<()> {
    let (mut ready_reader, ready_writer) = UnixStream::pair()?;
    ready_reader.set_read_timeout(Some(Duration::from_secs(2)))?;
    let owner_fd = owner.as_raw_fd();
    let ready_fd = ready_writer.as_raw_fd();
    if owner_fd <= 2 || ready_fd <= 2 || ready_reader.as_raw_fd() <= 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "owned command requires open standard streams"));
    }
    let group = nix::unistd::getpgrp().as_raw();
    // SAFETY: The child branches use only fork, close, sigaction, read, write,
    // kill, and _exit. They never allocate, lock, unwind, or run Rust drops.
    let intermediate = unsafe { libc::fork() };
    if intermediate == -1 {
        return Err(io::Error::last_os_error());
    }
    if intermediate == 0 {
        // SAFETY: The child branches below use only async-signal-safe calls
        // and never return to Rust. Double-forking keeps shell wait independent.
        let monitor = unsafe { libc::fork() };
        if monitor == -1 {
            // SAFETY: Exit immediately without running fork-inherited drops.
            unsafe { libc::_exit(1) };
        }
        if monitor != 0 {
            // SAFETY: The intermediate child has no further work or cleanup.
            unsafe { libc::_exit(0) };
        }
        // SAFETY: Close only this child's copy of the validated socket fd.
        unsafe { libc::close(ready_reader.as_raw_fd()) };
        // SAFETY: Valid inherited fds and the isolated positive group ID were
        // established before fork. The monitor cannot return or run drops.
        unsafe { run_watchdog(owner_fd, ready_fd, group) };
    }
    drop(ready_writer);
    let mut status = 0;
    loop {
        // SAFETY: `status` is writable and this waits only for our intermediate
        // child. The monitor is reparented and never belongs to the command's
        // child-wait relationship.
        let waited = unsafe { libc::waitpid(intermediate, &mut status, 0) };
        if waited == intermediate {
            break;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    if !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
        return Err(io::Error::other("failed to start command lifetime watchdog"));
    }
    let mut ready = [0];
    ready_reader.read_exact(&mut ready)?;
    if ready != [READY] {
        return Err(io::Error::other("command lifetime watchdog did not initialize"));
    }
    Ok(())
}

/// # Safety
/// Called only in the double-forked child with valid borrowed socket fds above
/// stdio and a positive, isolated group ID. Never returns or drops Rust values.
#[expect(
    unsafe_code,
    reason = "Minimal fork-child watchdog uses only async-signal-safe POSIX calls"
)]
unsafe fn run_watchdog(owner_fd: libc::c_int, ready_fd: libc::c_int, group: libc::pid_t) -> ! {
    // SAFETY: POSIX sigaction has a valid all-zero representation before its
    // handler and mask are initialized below.
    let mut ignore: libc::sigaction = unsafe { std::mem::zeroed() };
    ignore.sa_sigaction = libc::SIG_IGN;
    // SAFETY: The pointer addresses a writable stack-owned signal mask.
    unsafe { libc::sigemptyset(&mut ignore.sa_mask) };
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGPIPE] {
        // SAFETY: Supported signal numbers and a fully initialized action;
        // ignoring cancellation pins group identity until SIGKILL or owner EOF.
        if unsafe { libc::sigaction(signal, &ignore, std::ptr::null_mut()) } == -1 {
            // SAFETY: Failed monitor initialization must not run Rust drops.
            unsafe { libc::_exit(1) };
        }
    }
    for fd in 0..=2 {
        // SAFETY: Close only this child's standard descriptors, never the
        // validated lifetime socket fds above stdio.
        unsafe { libc::close(fd) };
    }
    let ready = READY;
    // SAFETY: The borrowed socket fd is valid and the pointer addresses one
    // initialized byte on this child's stack.
    if unsafe { libc::write(ready_fd, std::ptr::addr_of!(ready).cast(), 1) } != 1 {
        // SAFETY: Failed readiness must exit without Rust drops.
        unsafe { libc::_exit(1) };
    }
    // SAFETY: Only this child's readiness descriptor copy is closed.
    unsafe { libc::close(ready_fd) };
    let mut byte = 0u8;
    loop {
        // SAFETY: The owner fd remains valid; the pointer is a writable byte.
        let read = unsafe { libc::read(owner_fd, std::ptr::addr_of_mut!(byte).cast(), 1) };
        if read == -1 && nix::errno::Errno::last() == nix::errno::Errno::EINTR {
            continue;
        }
        // EOF, invalid protocol data, and I/O errors all fail closed.
        // SAFETY: This monitor remains in the positive isolated group it pins;
        // negating that group targets only its own managed command group.
        unsafe { libc::kill(-group, libc::SIGKILL) };
        // SAFETY: Even if kill fails, never return into fork-inherited Rust.
        unsafe { libc::_exit(1) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::net::UnixListener;

    fn private_directory(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::create_dir(&path).expect("create private directory");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("owner-only directory");
        path
    }

    fn bind_endpoint(directory: &Path, name: &str) -> PathBuf {
        let socket = directory.join(name);
        // The bound path stays a socket file after the listener is dropped.
        drop(UnixListener::bind(&socket).expect("bind endpoint"));
        socket
    }

    #[test]
    fn accepts_a_supervisor_shaped_endpoint() {
        let root = TempDir::new().expect("temp root");
        let directory = private_directory(root.path(), "vtc-accepted");
        let socket = bind_endpoint(&directory, OWNER_SOCKET_FILE);

        assert!(is_owned_endpoint(&socket));
        assert!(is_owned_endpoint_under(&socket, &std::env::temp_dir()));
    }

    #[test]
    fn ignores_a_stale_or_invalid_endpoint() {
        let root = TempDir::new().expect("temp root");
        let missing = root.path().join("vtc-stale").join(OWNER_SOCKET_FILE);
        // A stale value points at an endpoint the supervisor has removed.
        assert!(!is_owned_endpoint(&missing));

        let directory = private_directory(root.path(), "vtc-stale");
        std::fs::write(&missing, b"not a socket").expect("regular file");
        assert!(!is_owned_endpoint(&missing));

        let _foreign = bind_endpoint(&directory, "endpoint");
        assert!(!is_owned_endpoint(&directory.join("endpoint")));
        assert!(!is_owned_endpoint(Path::new(OWNER_SOCKET_FILE)));
    }

    #[test]
    fn ignores_an_endpoint_outside_a_private_directory() {
        let root = TempDir::new().expect("temp root");
        let directory = root.path().join("vtc-public");
        std::fs::create_dir(&directory).expect("public directory");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).expect("world-accessible");
        let socket = bind_endpoint(&directory, OWNER_SOCKET_FILE);

        assert!(!is_owned_endpoint(&socket));
    }

    #[test]
    fn ignores_a_directory_the_supervisor_did_not_name() {
        let root = TempDir::new().expect("temp root");
        let directory = private_directory(root.path(), "other-owner");
        let socket = bind_endpoint(&directory, OWNER_SOCKET_FILE);

        assert!(!is_owned_endpoint(&socket));
    }

    #[test]
    fn ignores_a_symlinked_endpoint() {
        let root = TempDir::new().expect("temp root");
        let target = bind_endpoint(root.path(), "target.sock");
        let directory = private_directory(root.path(), "vtc-symlinked");
        let socket = directory.join(OWNER_SOCKET_FILE);
        symlink(&target, &socket).expect("symlink endpoint");

        assert!(!is_owned_endpoint(&socket));
    }

    #[test]
    fn ignores_an_endpoint_outside_the_temp_root() {
        let root = TempDir::new().expect("temp root");
        let other_root = TempDir::new().expect("other root");
        let directory = private_directory(root.path(), "vtc-containment");
        let socket = bind_endpoint(&directory, OWNER_SOCKET_FILE);

        assert!(is_owned_endpoint(&socket));
        assert!(!is_owned_endpoint_under(&socket, other_root.path()));
    }
}
