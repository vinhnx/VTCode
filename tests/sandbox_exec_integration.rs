//! Kernel-level enforcement tests for the built-in Linux sandbox launcher.
//!
//! These spawn the real `vtcode sandbox-exec` binary with serialized helper
//! protocol flags and assert Landlock/seccomp behavior. They need a kernel
//! with Landlock (Linux 5.13+) and are skipped gracefully otherwise (e.g.
//! containers whose seccomp profile blocks `landlock_create_ruleset` — run
//! those with `--security-opt seccomp=unconfined`).
#![allow(missing_docs, reason = "Platform-specific integration test.")]
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use assert_cmd::cargo::cargo_bin_cmd;

/// Exit code used by the launcher for its own failures (see
/// `src/cli/sandbox_exec.rs`).
const EXIT_LAUNCHER_ERROR: i32 = 125;

static LAUNCHER_SUPPORTED: LazyLock<bool> = LazyLock::new(|| {
    let output = sandbox_command(Path::new("/tmp"))
        .args([
            "--sandbox-policy-cwd",
            "/tmp",
            "--sandbox-policy",
            r#"{"type":"read_only"}"#,
            "--seccomp-profile",
            "{}",
            "--resource-limits",
            "{}",
            "--",
            "true",
        ])
        .output()
        .expect("spawn vtcode sandbox-exec");
    if output.status.success() {
        return true;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() == Some(EXIT_LAUNCHER_ERROR) && stderr.contains("Landlock") {
        eprintln!("skipping sandbox-exec integration tests: {stderr}");
        return false;
    }
    panic!("sandbox-exec canary unexpectedly failed: {stderr}");
});

fn sandbox_command(cwd: &Path) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("vtcode");
    cmd.arg("sandbox-exec").current_dir(cwd);
    cmd
}

/// Run a `sh -c` command under the given sandbox policy JSON.
fn run_sandboxed(policy: &str, cwd: &Path, home: Option<&Path>, script: &str) -> std::process::Output {
    let mut cmd = sandbox_command(cwd);
    cmd.args([
        "--sandbox-policy-cwd",
        &cwd.display().to_string(),
        "--sandbox-policy",
        policy,
        "--seccomp-profile",
        "{}",
        "--resource-limits",
        "{}",
        "--",
        "sh",
        "-c",
        script,
    ]);
    if let Some(home) = home {
        cmd.env("HOME", home);
    }
    cmd.output().expect("spawn sandboxed sh")
}

fn workspace_write_policy(root: &Path) -> String {
    format!(r#"{{"type":"workspace_write","writable_roots":[{{"root":"{}"}}]}}"#, root.display())
}

fn read_only_policy() -> &'static str {
    r#"{"type":"read_only"}"#
}

#[test]
fn workspace_write_allows_workspace_writes() {
    if !*LAUNCHER_SUPPORTED {
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let output = run_sandboxed(
        &workspace_write_policy(workspace.path()),
        workspace.path(),
        None,
        "echo hi > inside.txt && cat inside.txt",
    );
    assert!(
        output.status.success(),
        "workspace write should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(workspace.path().join("inside.txt").exists());
}

#[test]
fn workspace_write_denies_writes_outside_writable_roots() {
    if !*LAUNCHER_SUPPORTED {
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let escape = tempfile::tempdir().expect("escape tempdir");
    let outside: PathBuf = escape.path().join("escape.txt");
    let script = format!("echo hi > {}", outside.display());
    let output = run_sandboxed(&workspace_write_policy(workspace.path()), workspace.path(), None, &script);
    assert!(!output.status.success(), "write outside writable roots must fail");
    assert!(!outside.exists(), "escaped file must not exist");
}

#[test]
fn read_only_denies_workspace_writes_but_allows_reads() {
    if !*LAUNCHER_SUPPORTED {
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    std::fs::write(workspace.path().join("existing.txt"), "content").expect("seed file");

    let write = run_sandboxed(read_only_policy(), workspace.path(), None, "echo nope > blocked.txt");
    assert!(!write.status.success(), "read-only policy must deny writes");

    let read = run_sandboxed(read_only_policy(), workspace.path(), None, "cat existing.txt");
    assert!(
        read.status.success(),
        "read-only policy must allow reads: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&read.stdout).trim(), "content");
}

#[test]
fn sensitive_paths_are_not_readable() {
    if !*LAUNCHER_SUPPORTED {
        return;
    }
    let home = tempfile::tempdir().expect("home tempdir");
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).expect("create .ssh");
    std::fs::write(ssh.join("id_ed25519"), "secret-key-material").expect("seed key");

    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let blocked = run_sandboxed(
        &workspace_write_policy(workspace.path()),
        workspace.path(),
        Some(home.path()),
        "cat $HOME/.ssh/id_ed25519",
    );
    assert!(!blocked.status.success(), "reading ~/.ssh must be denied by the sandbox");

    let home_file = home.path().join("plain.txt");
    std::fs::write(&home_file, "fine").expect("seed file");
    let allowed = run_sandboxed(
        &workspace_write_policy(workspace.path()),
        workspace.path(),
        Some(home.path()),
        &format!("cat {}", home_file.display()),
    );
    assert!(
        allowed.status.success(),
        "non-sensitive home files stay readable: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );
}

#[test]
fn network_sockets_are_denied_under_strict_seccomp() {
    if !*LAUNCHER_SUPPORTED {
        return;
    }
    if Command::new("bash").arg("--version").output().is_err() {
        eprintln!("skipping: bash (for /dev/tcp) unavailable");
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    // AF_INET socket creation returns EPERM (seccomp), which bash reports as
    // "Permission denied" rather than "Connection refused".
    let output = sandbox_command(workspace.path())
        .args([
            "--sandbox-policy-cwd",
            &workspace.path().display().to_string(),
            "--sandbox-policy",
            read_only_policy(),
            "--seccomp-profile",
            "{}",
            "--resource-limits",
            "{}",
            "--",
            "bash",
            "-c",
            "exec 3<>/dev/tcp/127.0.0.1/9",
        ])
        .output()
        .expect("spawn sandboxed bash");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Permission denied"),
        "socket() should fail with EPERM under strict seccomp, got: {stderr}"
    );
}

#[test]
fn namespace_creation_is_denied() {
    if !*LAUNCHER_SUPPORTED {
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let has_unshare = Command::new("unshare").arg("--help").output().is_ok();
    if !has_unshare {
        eprintln!("skipping: unshare binary unavailable");
        return;
    }
    let output = sandbox_command(workspace.path())
        .args([
            "--sandbox-policy-cwd",
            &workspace.path().display().to_string(),
            "--sandbox-policy",
            read_only_policy(),
            "--seccomp-profile",
            "{}",
            "--resource-limits",
            "{}",
            "--",
            "unshare",
            "--user",
            "true",
        ])
        .output()
        .expect("spawn sandboxed unshare");
    assert!(!output.status.success(), "unshare --user must fail under strict seccomp");
}
