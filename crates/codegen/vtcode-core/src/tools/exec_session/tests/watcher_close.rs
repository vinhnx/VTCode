use super::*;

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
