use super::*;

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
