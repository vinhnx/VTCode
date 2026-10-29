use super::*;

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
