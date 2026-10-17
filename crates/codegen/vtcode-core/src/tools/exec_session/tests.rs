use hashbrown::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;
use tokio::time::{Duration, timeout};

use super::{
    BackgroundShortcutResult, EXEC_SESSION_COMPLETION_COMMAND_MAX_BYTES, EXEC_SESSION_PREVIEW_HEAD_BYTES,
    EXEC_SESSION_PREVIEW_TAIL_BYTES, ExecSessionManager, MAX_BACKGROUND_PROCESSES, RetainedSessionPreview,
    bounded_completion_command,
};
use crate::config::PtyConfig;
use crate::tools::pty::{PtyCloseMode, PtySize};
use crate::tools::registry::PtySessionManager;
use crate::utils::path::canonicalize_workspace;

#[test]
fn completion_command_is_single_line_redacted_and_bounded() {
    let secret = concat!("password=", "supersecretvalue");
    let command = format!("cargo\ncheck\u{1b} {secret} {}", "x".repeat(1_024));

    let bounded = bounded_completion_command(&command);

    assert!(bounded.starts_with("cargo check "));
    assert!(!bounded.chars().any(char::is_control));
    assert!(!bounded.contains("supersecretvalue"));
    assert!(bounded.contains("[REDACTED_SECRET]"));
    assert!(bounded.len() <= EXEC_SESSION_COMPLETION_COMMAND_MAX_BYTES);
    assert!(bounded.ends_with("..."));
}

#[test]
fn completion_command_preserves_short_safe_labels() {
    assert_eq!(bounded_completion_command("cargo check --locked"), "cargo check --locked");
}

#[tokio::test]
#[cfg(all(unix, feature = "tui"))]
async fn pty_session_limit_holds_until_exec_session_close() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions =
        PtySessionManager::new(workspace_root.clone(), PtyConfig { max_sessions: 1, ..Default::default() });
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "run-1".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root.clone(),
            size,
            HashMap::new(),
            None,
        )
        .await?;

    let second = manager
        .create_pty_session(
            "run-2".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root.clone(),
            size,
            HashMap::new(),
            None,
        )
        .await;
    assert!(second.is_err());
    assert!(second.unwrap_err().to_string().contains("Maximum PTY sessions"));

    manager.close_session("run-1").await?;
    manager
        .create_pty_session(
            "run-3".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root,
            size,
            HashMap::new(),
            None,
        )
        .await?;
    manager.close_session("run-3").await?;

    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn pipe_session_activity_receiver_notifies_on_output() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "run-1".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf hello".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("run-1")
        .await?
        .expect("pipe sessions should expose activity receiver");

    let output = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(output) = manager.read_session_output("run-1", true).await? {
                return Ok::<String, anyhow::Error>(output);
            }
            activity_rx.changed().await?;
        }
    })
    .await??;
    assert!(output.contains("hello"));

    manager.close_session("run-1").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn concurrent_pipe_session_create_with_same_id_creates_exactly_one() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    let (a, b) = tokio::join!(
        manager.create_pipe_session(
            "same-id".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        ),
        manager.create_pipe_session(
            "same-id".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        ),
    );

    assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1, "exactly one concurrent create must win: {a:?} {b:?}");
    let loser = a.err().or_else(|| b.err()).expect("the loser should error");
    assert!(loser.to_string().contains("already exists"), "loser error should report duplicate: {loser}");

    manager.close_session("same-id").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn background_sessions_are_bounded_and_fourth_launch_does_not_spawn() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    for index in 0..MAX_BACKGROUND_PROCESSES {
        manager
            .create_pipe_session_with_sandbox_and_background(
                format!("background-{index}").into(),
                vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
                workspace_root.clone(),
                HashMap::new(),
                false,
                true,
            )
            .await?;
    }

    assert_eq!(manager.active_background_processes(), MAX_BACKGROUND_PROCESSES);
    let fourth = manager
        .create_pipe_session_with_sandbox_and_background(
            "background-fourth".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
            false,
            true,
        )
        .await
        .expect_err("the fourth background launch must be rejected before spawning");
    assert!(fourth.to_string().contains("maximum background process limit"));
    assert_eq!(manager.active_background_processes(), MAX_BACKGROUND_PROCESSES);
    assert_eq!(manager.list_sessions().await.len(), MAX_BACKGROUND_PROCESSES);

    for index in 0..MAX_BACKGROUND_PROCESSES {
        manager.close_session(&format!("background-{index}")).await?;
    }
    Ok(())
}

#[tokio::test]
async fn active_cleanup_preserves_background_sessions() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "foreground-cleanup".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;
    manager
        .create_pipe_session_with_sandbox_and_background(
            "background-survives-cleanup".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root,
            HashMap::new(),
            false,
            true,
        )
        .await?;

    manager.terminate_active_sessions_async().await?;
    assert!(manager.snapshot_session("foreground-cleanup").await.is_err());
    assert!(manager.snapshot_session("background-survives-cleanup").await?.background);

    manager.close_session("background-survives-cleanup").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn managed_background_sessions_do_not_consume_raw_background_slots_or_drawer_entries() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    let metadata = manager
        .create_pipe_session_for_managed_background(
            "managed-background".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    assert!(metadata.background);
    assert_eq!(manager.active_background_processes(), 0);
    assert!(manager.background_session_snapshots().await.is_empty());
    manager.close_session("managed-background").await?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn truncated_repeated_output_keeps_preview_marker() {
    let mut preview = RetainedSessionPreview::default();
    preview.append(&"x".repeat(EXEC_SESSION_PREVIEW_HEAD_BYTES + EXEC_SESSION_PREVIEW_TAIL_BYTES + 1));

    assert!(preview.truncated);
    assert!(preview.render().contains("[output preview truncated]"));
}

#[tokio::test]
#[cfg(unix)]
async fn exited_background_sessions_release_slots_but_remain_inspectable() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session_with_sandbox_and_background(
            "background-exited".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf done".to_string()],
            workspace_root.clone(),
            HashMap::new(),
            false,
            true,
        )
        .await?;

    timeout(Duration::from_secs(2), async {
        loop {
            if manager.active_background_processes() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background watcher should release an exited slot");

    let session = manager.snapshot_session("background-exited").await?;
    assert!(session.background);
    assert!(session.exit_code.is_some());
    assert_eq!(manager.list_sessions().await.len(), 1, "exited background metadata is retained");
    manager.close_session("background-exited").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn managed_background_completion_is_published_once_after_confirmed_clean_exit() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let mut completions = manager.subscribe_completion();

    manager
        .create_pipe_session_for_managed_background(
            "managed-completion-clean".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let completion = timeout(Duration::from_secs(2), completions.recv()).await??;
    assert_eq!(completion.session_id.as_str(), "managed-completion-clean");
    assert!(completion.managed_background);
    assert!(completion.command.contains("exit 0"));
    assert_eq!(completion.exit_code, 0);
    assert!(timeout(Duration::from_millis(100), completions.recv()).await.is_err());

    manager.close_session("managed-completion-clean").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn background_completion_waits_for_final_output_capture() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let mut completions = manager.subscribe_completion();

    manager
        .create_pipe_session_with_sandbox_and_background(
            "background-final-output".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "i=0; while [ \"$i\" -lt 4096 ]; do printf 'padding-%04d\\n' \"$i\"; i=$((i + 1)); done; printf 'final-output-sentinel\\n'"
                    .to_string(),
            ],
            workspace_root,
            HashMap::new(),
            false,
            true,
        )
        .await?;

    let completion = timeout(Duration::from_secs(3), completions.recv()).await??;
    assert_eq!(completion.exit_code, 0);
    let snapshot = manager.background_session_snapshot("background-final-output").await?;
    assert!(
        snapshot.preview.contains("final-output-sentinel"),
        "completion must not outrun final output capture: {}",
        snapshot.preview
    );

    manager.close_session("background-final-output").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn pruning_exited_background_session_does_not_abort_pending_completion() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let mut completions = manager.subscribe_completion();

    manager
        .create_pipe_session_with_sandbox_and_background(
            "background-prune-race".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "(sleep 0.3; printf 'late-output-sentinel\\n') & exit 0".to_string(),
            ],
            workspace_root,
            HashMap::new(),
            false,
            true,
        )
        .await?;

    timeout(Duration::from_secs(2), async {
        loop {
            if manager.is_session_completed("background-prune-race").await?.is_some() {
                break Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;

    assert!(
        manager.prune_exited_session("background-prune-race").await?.is_none(),
        "pruning must not abort a watcher that still owns completion delivery"
    );
    let completion = timeout(Duration::from_secs(2), completions.recv()).await??;
    assert_eq!(completion.session_id.as_str(), "background-prune-race");
    assert_eq!(completion.exit_code, 0);
    let snapshot = manager.background_session_snapshot("background-prune-race").await?;
    assert!(snapshot.preview.contains("late-output-sentinel"));

    manager.close_session("background-prune-race").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn managed_background_completion_reports_nonzero_exit_and_running_sessions_are_silent() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let mut completions = manager.subscribe_completion();

    manager
        .create_pipe_session_for_managed_background(
            "managed-completion-failing".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 7".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;

    let completion = timeout(Duration::from_secs(2), completions.recv()).await??;
    assert_eq!(completion.session_id.as_str(), "managed-completion-failing");
    assert!(completion.managed_background);
    assert_eq!(completion.exit_code, 7);
    assert!(timeout(Duration::from_millis(100), completions.recv()).await.is_err());
    manager.close_session("managed-completion-failing").await?;

    manager
        .create_pipe_session_for_managed_background(
            "managed-completion-running".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;
    assert!(timeout(Duration::from_millis(100), completions.recv()).await.is_err());
    manager.close_session("managed-completion-running").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn user_background_completion_is_published_for_the_parent_run_loop() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let mut completions = manager.subscribe_completion();

    manager
        .create_pipe_session_with_sandbox_and_background(
            "user-background-completion".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()],
            workspace_root,
            HashMap::new(),
            false,
            true,
        )
        .await?;

    let completion = timeout(Duration::from_secs(2), completions.recv()).await??;
    assert_eq!(completion.session_id.as_str(), "user-background-completion");
    assert!(!completion.managed_background);
    assert!(completion.command.contains("exit 0"));
    assert_eq!(completion.exit_code, 0);
    manager.close_session("user-background-completion").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn background_ui_snapshot_retains_drained_output_and_hides_foreground_sessions() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session_with_sandbox_and_background(
            "background-preview".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf retained".to_string()],
            workspace_root.clone(),
            HashMap::new(),
            false,
            true,
        )
        .await?;
    manager
        .create_pipe_session(
            "foreground-hidden".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("background-preview")
        .await?
        .expect("pipe sessions should expose activity receiver");
    let drained = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(output) = manager.read_session_output("background-preview", true).await? {
                break Ok::<String, anyhow::Error>(output);
            }
            activity_rx.changed().await?;
        }
    })
    .await??;
    assert!(drained.contains("retained"));

    let snapshots = manager.background_session_snapshots().await;
    assert_eq!(snapshots.len(), 1, "foreground sessions must not enter the drawer");
    assert_eq!(snapshots[0].metadata.id.as_str(), "background-preview");
    assert!(snapshots[0].preview.contains("retained"));

    timeout(Duration::from_secs(2), async {
        loop {
            if manager.is_session_completed("background-preview").await?.is_some() {
                break Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    let completed = manager.background_session_snapshot("background-preview").await?;
    assert!(completed.metadata.exit_code.is_some());

    manager.close_session("background-preview").await?;
    manager.close_session("foreground-hidden").await?;
    assert!(manager.background_session_snapshots().await.is_empty());
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn force_termination_retains_active_background_session_until_close() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let mut completions = manager.subscribe_completion();

    manager
        .create_pipe_session_with_sandbox_and_background(
            "background-force".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "printf 'before-termination\\n'; sleep 5".to_string(),
            ],
            workspace_root,
            HashMap::new(),
            false,
            true,
        )
        .await?;

    timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = manager.background_session_snapshot("background-force").await?;
            if snapshot.preview.contains("before-termination") {
                break Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;

    assert!(!manager.force_terminate_or_close("background-force").await?);
    let completion = timeout(Duration::from_secs(3), completions.recv()).await??;
    assert_eq!(completion.session_id.as_str(), "background-force");
    assert!(!completion.managed_background);
    assert!(completion.termination_requested);
    assert_ne!(completion.exit_code, 0);

    let snapshot = manager.background_session_snapshot("background-force").await?;
    assert!(snapshot.metadata.exit_code.is_some());
    assert!(snapshot.termination_requested);
    assert!(snapshot.preview.contains("before-termination"));
    timeout(Duration::from_secs(2), async {
        loop {
            if manager.active_background_processes() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("force-terminated session should release its background slot");

    assert!(manager.force_terminate_or_close("background-force").await?);
    assert!(manager.background_session_snapshots().await.is_empty());
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn ctrl_b_promotes_foreground_session_and_respects_capacity() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "foreground".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;
    assert_eq!(manager.request_foreground_background(), Some(BackgroundShortcutResult::Requested));
    assert_eq!(manager.take_background_shortcut_result(), Some(BackgroundShortcutResult::Requested));
    assert!(manager.promote_requested_session("foreground").await?);
    assert!(manager.snapshot_session("foreground").await?.background);
    assert_eq!(manager.active_background_processes(), 1);
    manager.close_session("foreground").await?;

    for index in 0..MAX_BACKGROUND_PROCESSES {
        manager
            .create_pipe_session_with_sandbox_and_background(
                format!("capacity-{index}").into(),
                vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
                workspace_root.clone(),
                HashMap::new(),
                false,
                true,
            )
            .await?;
    }
    manager
        .create_pipe_session(
            "capacity-foreground".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;
    assert_eq!(manager.request_foreground_background(), Some(BackgroundShortcutResult::AtCapacity));
    assert_eq!(manager.take_background_shortcut_result(), Some(BackgroundShortcutResult::AtCapacity));

    manager.close_session("capacity-foreground").await?;
    for index in 0..MAX_BACKGROUND_PROCESSES {
        manager.close_session(&format!("capacity-{index}")).await?;
    }
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn foreground_watcher_clears_completed_session_without_wait_polling() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let foreground_count = Arc::new(AtomicUsize::new(0));
    manager.set_foreground_pty_counter(Arc::clone(&foreground_count));

    manager
        .create_pipe_session(
            "foreground-complete".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 7".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    timeout(Duration::from_secs(2), async {
        loop {
            if manager.foreground_session.lock().is_none() && foreground_count.load(Ordering::Acquire) == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("completed foreground session should be cleared without a wait poll");

    assert_eq!(foreground_count.load(Ordering::Acquire), 0);
    assert_eq!(manager.request_foreground_background(), None);
    let retained = manager.snapshot_session("foreground-complete").await?;
    assert_eq!(retained.exit_code, Some(7));
    manager.close_session("foreground-complete").await?;
    assert_eq!(foreground_count.load(Ordering::Acquire), 0);
    Ok(())
}

#[tokio::test]
#[cfg(all(unix, feature = "tui"))]
async fn foreground_pipe_session_counts_for_background_hint() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let foreground_count = Arc::new(AtomicUsize::new(0));
    manager.set_foreground_pty_counter(Arc::clone(&foreground_count));

    manager
        .create_pipe_session(
            "foreground-pipe".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;
    assert_eq!(foreground_count.load(Ordering::Relaxed), 1);

    manager.close_session("foreground-pipe").await?;
    assert_eq!(foreground_count.load(Ordering::Relaxed), 0);
    Ok(())
}

/// Regression: an already-exited session must close under a bounded wait
/// and always drop the foreground counter. An unbounded reader join here
/// used to freeze ForceTerminateOrClose and lock the composer.
#[tokio::test]
#[cfg(unix)]
async fn force_terminate_or_close_exited_pty_releases_foreground_count() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let foreground_count = Arc::new(AtomicUsize::new(0));
    manager.set_foreground_pty_counter(Arc::clone(&foreground_count));
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "pty-exit-close".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 101".to_string()],
            workspace_root,
            size,
            HashMap::new(),
            None,
        )
        .await?;
    assert_eq!(foreground_count.load(Ordering::Relaxed), 1);

    timeout(Duration::from_secs(2), async {
        loop {
            if manager.is_session_completed("pty-exit-close").await.ok().flatten().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("pty session should exit");

    let closed = timeout(Duration::from_secs(8), manager.force_terminate_or_close("pty-exit-close"))
        .await
        .expect("force_terminate_or_close must return within the close timeout bound")?;
    assert!(closed, "exited session should report already_exited=true");
    assert_eq!(foreground_count.load(Ordering::Relaxed), 0);
    Ok(())
}

/// ForceCancel must stop and detach foreground sessions and leave
/// background ones alone, so the foreground counter cannot stay elevated.
#[tokio::test]
#[cfg(unix)]
async fn force_cancel_foreground_sessions_skips_background() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let foreground_count = Arc::new(AtomicUsize::new(0));
    manager.set_foreground_pty_counter(Arc::clone(&foreground_count));

    manager
        .create_pipe_session(
            "force-cancel-fg".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;
    manager
        .create_pipe_session_with_sandbox_and_background(
            "force-cancel-bg".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            workspace_root,
            HashMap::new(),
            false,
            true,
        )
        .await?;
    assert_eq!(foreground_count.load(Ordering::Relaxed), 1);

    let (stopped, closed, failed) = manager.force_cancel_foreground_sessions().await;
    assert_eq!(failed, 0);
    assert_eq!(closed, 0);
    assert_eq!(stopped, 1);
    assert!(
        manager.session_record("force-cancel-bg").await.is_ok(),
        "background session must remain after force-cancel"
    );
    assert!(manager.session_record("force-cancel-fg").await.is_err());
    assert_eq!(foreground_count.load(Ordering::Relaxed), 0);
    manager.close_session("force-cancel-bg").await?;
    Ok(())
}

/// Regression: force_terminate on a live PTY child must return within the
/// bounded reap budget and observe an exit status via try_wait (the child
/// is reaped, not abandoned).
#[tokio::test]
#[cfg(unix)]
async fn force_terminate_reaps_live_pty_child() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "pty-live-reap".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            workspace_root,
            size,
            HashMap::new(),
            None,
        )
        .await?;

    timeout(Duration::from_secs(8), manager.force_terminate_session("pty-live-reap"))
        .await
        .expect("force_terminate_session must return within the bounded reap budget")?;

    // try_wait after kill must observe the child exited (i.e. it was reaped),
    // not merely that the terminate call returned.
    let exit = timeout(Duration::from_secs(2), manager.is_session_completed("pty-live-reap"))
        .await
        .expect("completion poll must not hang")?;
    assert!(exit.is_some(), "force_terminate must reap the child to an exit status, got {exit:?}");

    timeout(Duration::from_secs(8), manager.close_session("pty-live-reap"))
        .await
        .expect("close_session must return after force-terminate")?;
    Ok(())
}

/// Regression: `PtyCloseMode::Immediate` close must not wait out the
/// graceful SIGTERM window. The child traps TERM, so a Graceful close
/// would burn the full 500 ms grace before SIGKILL; an Immediate close
/// SIGKILLs the group right away and still reaps the child.
#[tokio::test]
async fn immediate_close_skips_grace_window_and_reaps_live_pty_child() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let foreground_count = Arc::new(AtomicUsize::new(0));
    manager.set_foreground_pty_counter(Arc::clone(&foreground_count));
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "immediate-live".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "trap \"\" TERM; sleep 30".to_string(),
            ],
            workspace_root,
            size,
            HashMap::new(),
            None,
        )
        .await?;

    let started = std::time::Instant::now();
    let metadata =
        timeout(Duration::from_secs(8), manager.close_session_with_mode("immediate-live", PtyCloseMode::Immediate))
            .await
            .expect("immediate close must return within the bounded reap budget")?;

    // A Graceful close of a TERM-trapping child takes at least the 500 ms
    // grace window; Immediate must finish well under it.
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "immediate close must skip the SIGTERM grace window, took {:?}",
        started.elapsed()
    );
    assert!(
        metadata.exit_code.is_some(),
        "immediate close must reap the child to an exit status, got {:?}",
        metadata.exit_code
    );
    assert!(manager.session_record("immediate-live").await.is_err());
    assert_eq!(foreground_count.load(Ordering::Relaxed), 0);
    Ok(())
}

/// The turn-Cancelled terminator uses Immediate PTY termination and the
/// foreground-only filter: live foreground sessions die fast while
/// user-owned background sessions survive for later continuation.
#[tokio::test]
async fn terminate_active_for_exit_kills_foreground_and_preserves_background() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "exit-active-fg".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "trap \"\" TERM; sleep 30".to_string(),
            ],
            workspace_root.clone(),
            size,
            HashMap::new(),
            None,
        )
        .await?;
    manager
        .create_pipe_session_with_sandbox_and_background(
            "exit-active-bg".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            workspace_root,
            HashMap::new(),
            false,
            true,
        )
        .await?;

    let started = std::time::Instant::now();
    timeout(Duration::from_secs(8), manager.terminate_active_sessions_for_exit_async())
        .await
        .expect("terminate_active_for_exit must return within the bounded close budget")?;

    assert!(
        started.elapsed() < Duration::from_millis(400),
        "foreground Immediate close must skip the SIGTERM grace window, took {:?}",
        started.elapsed()
    );
    assert!(manager.session_record("exit-active-fg").await.is_err());
    assert!(
        manager.session_record("exit-active-bg").await.is_ok(),
        "background session must remain after the active-only terminator"
    );
    manager.close_session("exit-active-bg").await?;
    Ok(())
}

#[tokio::test]
#[cfg(all(unix, feature = "tui"))]
async fn foreground_watcher_promotes_requested_pty_without_wait_polling() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let active_pty_sessions = Arc::new(AtomicUsize::new(0));
    manager.set_foreground_pty_counter(Arc::clone(&active_pty_sessions));
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "foreground-pty".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root,
            size,
            HashMap::new(),
            None,
        )
        .await?;
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 1);
    assert_eq!(manager.request_foreground_background(), Some(BackgroundShortcutResult::Requested));

    timeout(Duration::from_secs(2), async {
        loop {
            if manager
                .snapshot_session("foreground-pty")
                .await
                .map(|snapshot| snapshot.background)
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("foreground watcher should consume a pending Ctrl+B request");

    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    assert_eq!(manager.active_background_processes(), 1);
    manager.close_session("foreground-pty").await?;
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test]
#[cfg(all(unix, feature = "tui"))]
async fn closing_exited_pty_kills_descendants_that_keep_the_pty_open() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let pty_manager = pty_sessions.manager().clone();
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    pty_manager.create_session(
        "pty-descendant".to_string(),
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "/bin/sleep 5 & exit 0".to_string(),
        ],
        workspace_root,
        size,
    )?;

    timeout(Duration::from_secs(2), async {
        loop {
            if pty_manager.is_session_completed("pty-descendant")?.is_some() {
                break Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;

    let close_manager = pty_manager.clone();
    timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || close_manager.close_session("pty-descendant")),
    )
    .await
    .expect("closing an exited PTY must not wait for inherited descriptors")??;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn in_progress_exec_sessions_filters_exited_orders_and_caps() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    // One long-running session, then a newer one.
    manager
        .create_pipe_session(
            "run-old".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(10)).await;
    manager
        .create_pipe_session(
            "run-new".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;
    // One quick session that exits on its own.
    manager
        .create_pipe_session(
            "run-quick".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "true".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;

    // Wait for the quick session to exit.
    for _ in 0..50 {
        if manager.is_session_completed("run-quick").await?.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let in_progress = manager.in_progress_exec_sessions(4).await;
    assert_eq!(in_progress.len(), 2, "exited sessions must be filtered: {in_progress:?}");
    assert_eq!(in_progress[0].id.as_str(), "run-new", "newest session must be listed first: {in_progress:?}");
    assert_eq!(in_progress[1].id.as_str(), "run-old");
    assert!(in_progress.iter().all(|session| session.exit_code.is_none()));

    // Cap is honored, keeping the newest.
    let capped = manager.in_progress_exec_sessions(1).await;
    assert_eq!(capped.len(), 1);
    assert_eq!(capped[0].id.as_str(), "run-new");
    // Cap of 0 returns nothing.
    assert!(manager.in_progress_exec_sessions(0).await.is_empty());

    manager.close_session("run-old").await?;
    manager.close_session("run-new").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn pipe_session_drain_clears_so_old_output_does_not_reappear() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "drain-clear".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf hello".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("drain-clear")
        .await?
        .expect("pipe sessions should expose activity receiver");

    timeout(Duration::from_secs(2), activity_rx.changed()).await??;
    let drained = manager
        .read_session_output("drain-clear", true)
        .await?
        .expect("should drain hello");
    assert!(drained.contains("hello"));

    let stale = manager.read_session_output("drain-clear", true).await?;
    assert!(stale.is_none(), "drained output must not reappear: {stale:?}");

    manager.close_session("drain-clear").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn pipe_session_returns_new_output_after_drain() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "drain-resume".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "printf one; sleep 1; printf two".to_string(),
            ],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("drain-resume")
        .await?
        .expect("pipe sessions should expose activity receiver");

    timeout(Duration::from_secs(2), activity_rx.changed()).await??;
    let _first = manager.read_session_output("drain-resume", true).await?;

    timeout(Duration::from_secs(3), activity_rx.changed()).await??;
    let second = manager
        .read_session_output("drain-resume", true)
        .await?
        .expect("should drain post-drain output");
    assert!(second.contains("two"), "output produced after a drain must still be returned: {second:?}");

    manager.close_session("drain-resume").await?;
    Ok(())
}

#[tokio::test]
async fn pipe_output_buffer_peek_is_idempotent_and_non_consuming() {
    let buffer = super::PipeOutputBuffer::default();
    buffer.append("hello", 5).await;

    let first = buffer.peek_pending().await;
    let second = buffer.peek_pending().await;
    assert_eq!(first, second);
    assert_eq!(first, Some("hello".to_string()));
}

#[tokio::test]
async fn pipe_output_buffer_drain_returns_exactly_once() {
    let buffer = super::PipeOutputBuffer::default();
    buffer.append("hello", 5).await;

    let first = buffer.drain_pending().await;
    let second = buffer.drain_pending().await;
    assert_eq!(first, Some("hello".to_string()));
    assert_eq!(second, None);
}

#[tokio::test]
async fn pipe_output_buffer_drain_clears_internal_pending_length() {
    let buffer = super::PipeOutputBuffer::default();
    buffer.append("hello", 5).await;

    buffer.drain_pending().await;
    let peek: Option<String> = buffer.peek_pending().await;
    assert!(peek.is_none(), "buffer must be empty after drain: {peek:?}");
}

#[tokio::test]
async fn pipe_output_buffer_append_after_drain_returns_only_fresh_output() {
    let buffer = super::PipeOutputBuffer::default();
    buffer.append("first", 5).await;
    buffer.drain_pending().await;

    buffer.append("second", 6).await;
    let output = buffer.peek_pending().await;
    assert_eq!(output, Some("second".to_string()));
}

#[tokio::test]
async fn pipe_output_buffer_bounds_preview_and_tracks_total_bytes() {
    let buffer = super::PipeOutputBuffer::default();
    let chunk = "x".repeat(super::PIPE_OUTPUT_HEAD_BYTES * 4);
    buffer.append(&chunk, chunk.len()).await;

    let preview = buffer.peek_pending().await.expect("bounded preview");
    assert!(preview.len() <= super::PIPE_OUTPUT_HEAD_BYTES * 3);
    let (total_bytes, truncated) = buffer.stats().await;
    assert_eq!(total_bytes, chunk.len() as u64);
    assert!(truncated);
}

/// Close must not deadlock or panic when a reader still holds
/// `output_read_lock` past `EXEC_SESSION_OUTPUT_READ_LOCK_TIMEOUT`.
/// Post-close reads must fail with a normal error, not a panic.
#[tokio::test]
async fn close_proceeds_cleanly_when_output_read_lock_is_held() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "lock-race".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf hello".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let record = manager.session_record("lock-race").await?;
    let _held = record.output_read_lock.lock().await;

    // Close waits at most EXEC_SESSION_OUTPUT_READ_LOCK_TIMEOUT (1s) for
    // the lock, then proceeds; bound the whole call so a hang fails fast.
    let closed = timeout(Duration::from_secs(5), manager.close_session("lock-race")).await;
    let session = closed
        .expect("close_session must finish while the read lock is held")
        .expect("close_session must succeed after the lock acquire timeout");
    assert_eq!(session.id.as_str(), "lock-race");

    drop(_held);

    let after = manager.read_session_output("lock-race", true).await;
    assert!(after.is_err(), "post-close read must be a clean error, got {after:?}");
    Ok(())
}
