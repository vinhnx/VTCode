//! Two-process session exclusivity: a second OS process opening the same
//! session for writing fails closed with `SessionBusy`; different sessions
//! proceed concurrently.
//!
//! `e2e_mem_holder` is the child-process entry point, re-executed via the
//! current test binary with `VTMEM_*` env vars. Without those vars it passes
//! vacuously so normal suite runs stay green.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vtcode_memory::{DEFAULT_MAX_EVENTS, open, open_exclusive};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// How long the first holder keeps the session open (generous for loaded CI).
const HOLDER_HOLD_SECS: u64 = 15;
/// Upper bound for a child to spawn, open, and signal readiness.
const READY_TIMEOUT: Duration = Duration::from_secs(90);

fn holder_args() -> Option<(PathBuf, String, u64, PathBuf)> {
    let workspace = std::env::var("VTMEM_WORKSPACE").ok()?;
    let session = std::env::var("VTMEM_SESSION").ok()?;
    let hold_secs = std::env::var("VTMEM_HOLD_SECS").ok()?.parse().ok()?;
    let ready_file = std::env::var("VTMEM_READY_FILE").ok()?;
    Some((PathBuf::from(workspace), session, hold_secs, PathBuf::from(ready_file)))
}

#[test]
fn e2e_mem_holder() -> TestResult {
    let Some((workspace, session, hold_secs, ready_file)) = holder_args() else {
        return Ok(());
    };
    // Render failures with Display (what a user would see), not Debug.
    let _log = match open_exclusive(&workspace, &session, DEFAULT_MAX_EVENTS) {
        Ok(log) => log,
        Err(err) => {
            eprintln!("child open failed: {err}");
            std::process::exit(1);
        }
    };
    std::fs::write(&ready_file, b"ready")?;
    std::thread::sleep(Duration::from_secs(hold_secs));
    // `_log` drops here, releasing the session lock.
    Ok(())
}

fn spawn_holder(
    workspace: &std::path::Path,
    session: &str,
    hold_secs: u64,
    ready_file: &std::path::Path,
) -> std::io::Result<Child> {
    Command::new(std::env::current_exe()?)
        .arg("e2e_mem_holder")
        .arg("--exact")
        .arg("--nocapture")
        .env("VTMEM_WORKSPACE", workspace)
        .env("VTMEM_SESSION", session)
        .env("VTMEM_HOLD_SECS", hold_secs.to_string())
        .env("VTMEM_READY_FILE", ready_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn wait_for_ready(ready_file: &std::path::Path) -> TestResult {
    let started = Instant::now();
    while !ready_file.exists() {
        if started.elapsed() >= READY_TIMEOUT {
            return Err("holder never signaled readiness".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[test]
fn two_processes_share_a_workspace_but_not_a_session() -> TestResult {
    let dir = tempfile::tempdir()?;
    let workspace = dir.path().join("ws");
    std::fs::create_dir_all(&workspace)?;

    // Process A holds session A open for writing.
    let ready_a = dir.path().join("ready-a");
    let mut child_a = spawn_holder(&workspace, "session-a", HOLDER_HOLD_SECS, &ready_a)?;
    wait_for_ready(&ready_a)?;

    // Process B opens a different session: must succeed concurrently.
    let ready_b = dir.path().join("ready-b");
    let output_b = spawn_holder(&workspace, "session-b", 1, &ready_b)?.wait_with_output()?;
    assert!(
        output_b.status.success(),
        "session B must open concurrently: {}",
        String::from_utf8_lossy(&output_b.stderr)
    );

    // Process A2 opens session A for writing while A holds it: must fail closed.
    assert!(child_a.try_wait()?.is_none(), "A must still hold its session");
    let ready_a2 = dir.path().join("ready-a2");
    let output_a2 = spawn_holder(&workspace, "session-a", 1, &ready_a2)?.wait_with_output()?;
    assert!(!output_a2.status.success(), "same-session writer overlap must fail");
    let stderr = String::from_utf8_lossy(&output_a2.stderr);
    assert!(stderr.contains("already open in another process"), "must name the contention: {stderr}");
    assert!(stderr.contains("session-a"), "must name the session: {stderr}");
    assert!(stderr.contains("vtcode --fork-session session-a"), "must print the exact fork retry: {stderr}");
    assert!(!ready_a2.exists(), "refused open must not signal readiness");

    assert!(child_a.wait()?.success(), "holder A must exit cleanly");

    // Both sessions are intact and readable afterwards.
    let manifest_a = open(&workspace, "session-a", DEFAULT_MAX_EVENTS)?.manifest();
    let manifest_b = open(&workspace, "session-b", DEFAULT_MAX_EVENTS)?.manifest();
    assert_eq!(manifest_a.session_id, "session-a");
    assert_eq!(manifest_b.session_id, "session-b");
    Ok(())
}
