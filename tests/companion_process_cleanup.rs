//! Exercise the shipped lifetime helper with real pipe and PTY process groups.
#![cfg(unix)]

#[path = "../src/cli/companion/process_owner.rs"]
mod process_owner;

use std::ffi::OsStr;
use std::os::fd::{FromRawFd, OwnedFd};
use std::process::Stdio;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader as AsyncBufReader};
use vtcode_bash_runner::process_owner::{OWNER_SOCKET_ENV, OwnedCommand};
use vtcode_core::config::PtyConfig;
use vtcode_core::tools::pty::{PtyManager, PtySize};

struct GroupCleanup(Pid);
impl Drop for GroupCleanup {
    fn drop(&mut self) {
        let _cleanup_result = killpg(self.0, Signal::SIGKILL);
    }
}

fn owned(socket: &std::path::Path, args: &[&str], arg0: Option<&OsStr>) -> OwnedCommand {
    OwnedCommand::new(assert_cmd::cargo::cargo_bin!("vtcode"), socket, OsStr::new("/bin/sh"), args, arg0)
}

fn pipe_command(spec: &OwnedCommand, cwd: &std::path::Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(&spec.executable);
    let _ = command
        .args(&spec.arguments)
        .env(OWNER_SOCKET_ENV, &spec.owner_socket)
        .current_dir(cwd)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

fn wait_gone(pid: Pid) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if kill(pid, None) == Err(Errno::ESRCH) {
            return;
        }
        assert!(Instant::now() < deadline, "process {pid} survived task ownership cleanup");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
async fn native_helper_preserves_admitted_environment() {
    const PROBE: &str = "VTCODE_OWNER_ENV_PROBE";
    let loader_key = if cfg!(target_os = "macos") {
        "DYLD_VTCODE_OWNER_TEST"
    } else {
        "LD_VTCODE_OWNER_TEST"
    };
    if std::env::var_os(PROBE).is_some() {
        assert_eq!(std::env::var(loader_key).unwrap(), "admitted-loader-value");
        assert_eq!(std::env::var("VTCODE_OWNER_ORDINARY_TEST").unwrap(), "ordinary-value");
        return;
    }
    // Reuse the subprocess-fixture pattern from program_status_pty_tests. The
    // direct launch proves the OS loader preserves this harmless test key.
    let target = std::env::current_exe().unwrap();
    let args = ["--exact", "native_helper_preserves_admitted_environment", "--nocapture"];
    let direct = tokio::process::Command::new(&target)
        .args(args)
        .env(PROBE, "1")
        .env(loader_key, "admitted-loader-value")
        .env("VTCODE_OWNER_ORDINARY_TEST", "ordinary-value")
        .output()
        .await
        .unwrap();
    assert!(direct.status.success(), "direct control: {}", String::from_utf8_lossy(&direct.stdout));
    let temp = TempDir::new().unwrap();
    let mut owner = process_owner::TaskProcessOwner::new().unwrap();
    let spec =
        OwnedCommand::new(assert_cmd::cargo::cargo_bin!("vtcode"), owner.socket(), target.as_os_str(), args, None);
    let output = pipe_command(&spec, temp.path())
        .env(PROBE, "1")
        .env(loader_key, "admitted-loader-value")
        .env("VTCODE_OWNER_ORDINARY_TEST", "ordinary-value")
        .output()
        .await
        .unwrap();
    owner.shutdown().unwrap();
    assert!(
        output.status.success(),
        "owned launch: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn owner_shutdown_kills_pipe_group_and_background_child_without_touching_unrelated_process() {
    let temp = TempDir::new().unwrap();
    let mut owner = process_owner::TaskProcessOwner::new().unwrap();
    let spec = owned(owner.socket(), &["-c", "sleep 60 & child=$!; printf '%s\\n' \"$child\"; wait"], None);
    let mut child = pipe_command(&spec, temp.path()).spawn().unwrap();
    let root = Pid::from_raw(i32::try_from(child.id().unwrap()).unwrap());
    let _cleanup = GroupCleanup(root);
    let mut stdout = AsyncBufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut line))
            .await
            .unwrap()
            .unwrap()
            > 0
    );
    let background = Pid::from_raw(line.trim().parse().unwrap());
    let mut unrelated = tokio::process::Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    // Killing a group leader alone leaves the group's other members alive.
    kill(root, Signal::SIGKILL).unwrap();
    assert!(kill(background, None).is_ok());
    owner.shutdown().unwrap();
    assert!(
        !tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    wait_gone(background);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), stdout.read_line(&mut line))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert!(unrelated.try_wait().unwrap().is_none());
    unrelated.kill().await.unwrap();
    let _ = unrelated.wait().await.unwrap();
}

#[test]
fn owner_shutdown_kills_pty_group_and_closes_its_output() {
    let temp = TempDir::new().unwrap();
    let workspace = vtcode_commons::canonicalize(temp.path()).unwrap();
    let mut owner = process_owner::TaskProcessOwner::new().unwrap();
    let spec = owned(owner.socket(), &["-c", "sleep 60 & child=$!; printf '%s\\n' \"$child\"; wait"], None);
    let manager = PtyManager::new(workspace.clone(), PtyConfig::default());
    // Set the endpoint only in this child; tests never change shared process
    // environment. exec retains the PTY session leader's PID and group.
    let mut command = vec![
        "/bin/sh".into(),
        "-c".into(),
        "export VTCODE_COMPANION_OWNER_SOCKET=\"$1\"; shift; exec \"$@\"".into(),
        "case".into(),
        spec.owner_socket.to_string_lossy().into_owned(),
        spec.executable.to_string_lossy().into_owned(),
    ];
    command.extend(spec.arguments.iter().map(|arg| arg.to_string_lossy().into_owned()));
    let metadata = manager
        .create_session("owner-pty".into(), command, workspace, PtySize::default())
        .unwrap();
    let root = Pid::from_raw(i32::try_from(metadata.child_pid.unwrap()).unwrap());
    let _cleanup = GroupCleanup(root);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = String::new();
    while !output.contains('\n') {
        if let Some(chunk) = manager.read_session_output("owner-pty", true).unwrap() {
            output.push_str(&chunk);
        }
        assert!(Instant::now() < deadline, "missing PTY readiness: {output:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
    let background = Pid::from_raw(output.trim().parse().unwrap());
    owner.shutdown().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = manager.is_session_completed("owner-pty").unwrap() {
            assert_ne!(status, 0);
            break;
        }
        assert!(Instant::now() < deadline, "PTY child was not reaped");
        std::thread::sleep(Duration::from_millis(10));
    }
    wait_gone(background);
    while !manager.is_output_drained("owner-pty").unwrap() {
        let _ = manager.read_session_output("owner-pty", true).unwrap();
        assert!(Instant::now() < deadline, "PTY output did not close");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = manager.close_session("owner-pty").unwrap();
}

#[tokio::test]
async fn native_helper_preserves_literal_arguments_and_does_not_join_watchdog_with_shell_wait() {
    let temp = TempDir::new().unwrap();
    let mut owner = process_owner::TaskProcessOwner::new().unwrap();
    let literal = "$(touch must-not-exist); literal argument";
    let spec = owned(owner.socket(), &["-c", "sleep 0.02 & wait; printf '%s\\n' \"$1\"", "case", literal], None);
    let output = tokio::time::timeout(Duration::from_secs(5), pipe_command(&spec, temp.path()).output())
        .await
        .unwrap()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, format!("{literal}\n").as_bytes());
    assert!(!temp.path().join("must-not-exist").exists());
    owner.shutdown().unwrap();
}

#[tokio::test]
async fn native_helper_restores_arg0_and_refuses_missing_owner_before_execution() {
    let temp = TempDir::new().unwrap();
    let mut owner = process_owner::TaskProcessOwner::new().unwrap();
    let spec = owned(owner.socket(), &["-c", "printf '%s' \"$0\""], Some(OsStr::new("retained-arg0")));
    let output = pipe_command(&spec, temp.path()).output().await.unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"retained-arg0");
    owner.shutdown().unwrap();
    let spec = owned(owner.socket(), &["-c", "touch must-not-exist"], None);
    let output = pipe_command(&spec, temp.path()).output().await.unwrap();
    assert!(!output.status.success());
    assert!(!temp.path().join("must-not-exist").exists());
}

/// Open a socket pair deliberately without CLOEXEC to simulate system
/// libraries that open descriptors without it (observed: macOS
/// kernel-control socket inherited at fd 24). `OwnedFd` closes both ends on
/// drop, including test failure, so sibling tests in this process are
/// unaffected.
#[expect(
    unsafe_code,
    reason = "raw socketpair is the only way to plant non-CLOEXEC sockets; see SAFETY below"
)]
fn plant_non_cloexec_sockets() -> [OwnedFd; 2] {
    let mut pair: [libc::c_int; 2] = [0, 0];
    // SAFETY: `socketpair` with a valid out-array yields two open, owned fds
    // on success; the assert aborts before wrapping on failure.
    let paired = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, pair.as_mut_ptr()) };
    assert_eq!(paired, 0);
    // SAFETY: `socketpair` succeeded, so this fd is open and owned by us, and
    // this `OwnedFd` takes that ownership exactly once.
    let first = unsafe { OwnedFd::from_raw_fd(pair[0]) };
    // SAFETY: same as above for the second end of the pair.
    let second = unsafe { OwnedFd::from_raw_fd(pair[1]) };
    [first, second]
}

#[tokio::test]
async fn native_helper_closes_lifetime_sockets_before_executing_target() {
    // Without the helper's close sweep these planted sockets leak into the
    // target and fail the assertion below, deterministically.
    let _planted = plant_non_cloexec_sockets();
    let temp = TempDir::new().unwrap();
    let mut owner = process_owner::TaskProcessOwner::new().unwrap();
    let script = "import os, stat\nsockets=[]\nfor fd in range(3,256):\n try:\n  if stat.S_ISSOCK(os.fstat(fd).st_mode): sockets.append(fd)\n except OSError: pass\nassert not sockets, sockets\nprint('no inherited sockets')";
    let spec = OwnedCommand::new(
        assert_cmd::cargo::cargo_bin!("vtcode"),
        owner.socket(),
        OsStr::new("/usr/bin/env"),
        ["python3", "-c", script],
        None,
    );
    let output = tokio::time::timeout(Duration::from_secs(5), pipe_command(&spec, temp.path()).output())
        .await
        .unwrap()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, b"no inherited sockets\n");
    owner.shutdown().unwrap();
}

#[tokio::test]
async fn owner_shutdown_removes_background_child_after_command_exits_normally() {
    let temp = TempDir::new().unwrap();
    let mut owner = process_owner::TaskProcessOwner::new().unwrap();
    let spec = owned(owner.socket(), &["-c", "sleep 60 & printf '%s\\n' \"$!\""], None);
    let mut child = pipe_command(&spec, temp.path()).spawn().unwrap();
    let _cleanup = GroupCleanup(Pid::from_raw(i32::try_from(child.id().unwrap()).unwrap()));
    let mut output = AsyncBufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap()
            > 0
    );
    let background = Pid::from_raw(line.trim().parse().unwrap());
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(kill(background, None).is_ok());
    owner.shutdown().unwrap();
    wait_gone(background);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
