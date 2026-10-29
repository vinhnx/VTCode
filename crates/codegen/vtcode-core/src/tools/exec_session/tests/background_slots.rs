use super::*;

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
