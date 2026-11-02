//! Two-process checkpoint concurrency: separate OS processes running turns in
//! different sessions of one workspace must not block each other, while a
//! second process in the SAME session fails closed.
//!
//! `e2e_checkpoint_holder` is the child-process entry point, re-executed via
//! the current test binary with `VTCHECK_*` env vars. Without those vars it
//! passes vacuously so normal suite runs stay green.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};

/// How long the first holder keeps its whole-turn lease (generous for loaded CI).
const HOLDER_HOLD_SECS: u64 = 20;
/// Upper bound for a child to spawn, capture, and signal readiness.
const READY_TIMEOUT: Duration = Duration::from_secs(90);

fn holder_args() -> Option<(PathBuf, String, usize, u64, PathBuf)> {
    let workspace = std::env::var("VTCHECK_WORKSPACE").ok()?;
    let session = std::env::var("VTCHECK_SESSION").ok()?;
    let turn = std::env::var("VTCHECK_TURN").ok()?.parse().ok()?;
    let hold_secs = std::env::var("VTCHECK_HOLD_SECS").ok()?.parse().ok()?;
    let ready_file = std::env::var("VTCHECK_READY_FILE").ok()?;
    Some((PathBuf::from(workspace), session, turn, hold_secs, PathBuf::from(ready_file)))
}

#[tokio::test]
async fn e2e_checkpoint_holder() -> Result<()> {
    let Some((workspace, session, turn, hold_secs, ready_file)) = holder_args() else {
        return Ok(());
    };
    let manager = SnapshotManager::new(SnapshotConfig::new(workspace))?;
    // Render failures with Display (what a user would see), not Debug.
    let _lease = match manager.begin_prompt(turn, &session, "e2e holder turn", &[]).await {
        Ok(lease) => lease,
        Err(err) => {
            eprintln!("child begin failed: {err:#}");
            std::process::exit(1);
        }
    };
    std::fs::write(&ready_file, b"ready").context("signal readiness")?;
    tokio::time::sleep(Duration::from_secs(hold_secs)).await;
    // `_lease` drops here, releasing the session lock.
    Ok(())
}

fn spawn_holder(workspace: &Path, session: &str, turn: usize, hold_secs: u64, ready_file: &Path) -> Result<Child> {
    Command::new(std::env::current_exe().context("current test binary")?)
        .arg("e2e_checkpoint_holder")
        .arg("--exact")
        .arg("--nocapture")
        .env("VTCHECK_WORKSPACE", workspace)
        .env("VTCHECK_SESSION", session)
        .env("VTCHECK_TURN", turn.to_string())
        .env("VTCHECK_HOLD_SECS", hold_secs.to_string())
        .env("VTCHECK_READY_FILE", ready_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn holder child")
}

fn wait_for_ready(ready_file: &Path) -> Result<()> {
    let started = Instant::now();
    while !ready_file.exists() {
        if started.elapsed() >= READY_TIMEOUT {
            anyhow::bail!("holder never signaled readiness");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[test]
fn two_processes_checkpoint_different_sessions_concurrently() -> Result<()> {
    let dir = tempfile::tempdir().context("fixture root")?;
    let workspace = dir.path().join("ws");
    std::fs::create_dir_all(&workspace).context("workspace")?;
    std::fs::write(workspace.join("file.txt"), "v1").context("fixture file")?;

    // Process A holds a whole-turn lease in session A.
    let ready_a = dir.path().join("ready-a");
    let mut child_a = spawn_holder(&workspace, "session-a", 1, HOLDER_HOLD_SECS, &ready_a)?;
    wait_for_ready(&ready_a)?;

    // Process B begins a turn in session B while A holds its lease: must succeed.
    let ready_b = dir.path().join("ready-b");
    let output_b = spawn_holder(&workspace, "session-b", 1, 1, &ready_b)?
        .wait_with_output()
        .context("wait for B")?;
    assert!(
        output_b.status.success(),
        "session B must proceed concurrently: {}",
        String::from_utf8_lossy(&output_b.stderr)
    );

    // Process A2 retries session A while A holds it: must fail closed.
    assert!(child_a.try_wait().context("poll A")?.is_none(), "A must still hold its lease");
    let ready_a2 = dir.path().join("ready-a2");
    let output_a2 = spawn_holder(&workspace, "session-a", 2, 1, &ready_a2)?
        .wait_with_output()
        .context("wait for A2")?;
    assert!(!output_a2.status.success(), "same-session overlap must fail");
    let stderr = String::from_utf8_lossy(&output_a2.stderr);
    assert!(stderr.contains("Another turn is using this session"), "must name the contention: {stderr}");
    assert!(!ready_a2.exists(), "failed begin must not signal readiness");

    assert!(child_a.wait().context("wait for A")?.success(), "holder A must exit cleanly");

    // Both sessions kept disjoint records with their own owners.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("runtime")?;
    runtime.block_on(async {
        let manager = SnapshotManager::new(SnapshotConfig::new(workspace))?;
        let stored_a = manager.load_snapshot_for_session("session-a", 1).await?.context("a1 record")?;
        let stored_b = manager.load_snapshot_for_session("session-b", 1).await?.context("b1 record")?;
        assert_eq!(stored_a.metadata.session_id.as_deref(), Some("session-a"));
        assert_eq!(stored_b.metadata.session_id.as_deref(), Some("session-b"));
        assert_eq!(manager.next_turn_number("session-a")?, 2);
        assert_eq!(manager.next_turn_number("session-b")?, 2);
        Ok::<(), anyhow::Error>(())
    })
}
