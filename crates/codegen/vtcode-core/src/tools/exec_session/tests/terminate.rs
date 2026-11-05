use super::*;

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

#[tokio::test]
async fn force_terminate_pipe_waits_for_the_child_exit() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    manager
        .create_pipe_session(
            "force-pipe-confirm".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;
    manager.force_terminate_session("force-pipe-confirm").await?;
    timeout(Duration::from_secs(1), async {
        loop {
            if manager.is_session_completed("force-pipe-confirm").await?.is_some() {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    manager.close_session("force-pipe-confirm").await?;
    Ok(())
}

#[tokio::test]
async fn force_cancel_closes_an_already_exited_pipe() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    manager
        .create_pipe_session(
            "force-pipe-exited".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 31".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;
    timeout(Duration::from_secs(2), async {
        loop {
            if manager.is_session_completed("force-pipe-exited").await? == Some(31) {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    assert_eq!(manager.force_cancel_foreground_sessions().await, (0, 1, 0));
    assert!(manager.session_record("force-pipe-exited").await.is_err());
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
