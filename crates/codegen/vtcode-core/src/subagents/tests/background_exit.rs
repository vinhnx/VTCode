use super::*;

#[tokio::test]
#[cfg(unix)]
async fn background_clean_exit_zero_becomes_stopped_without_restart() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let workspace = controller.config.workspace_root.clone();
    controller
        .config
        .exec_sessions
        .create_pipe_session_with_sandbox_and_background(
            "exec-clean-exit".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()],
            workspace,
            Default::default(),
            false,
            true,
        )
        .await
        .expect("clean exec session");
    wait_for_exec_exit(&controller.config.exec_sessions, "exec-clean-exit").await;

    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            "background-clean".to_string(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: "background-clean".to_string(),
                agent_name: "demo".to_string(),
                display_label: "demo".to_string(),
                description: "demo".to_string(),
                source: "test".to_string(),
                color: None,
                session_id: "session-clean".to_string(),
                exec_session_id: "exec-clean-exit".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: None,
                prompt: "demo".to_string(),
                summary: None,
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: None,
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entries = controller.refresh_background_processes().await.expect("refresh");
    let entry = entries.iter().find(|e| e.id == "background-clean").expect("clean entry");
    // Clean `exit 0` must surface as Stopped (matching `exited (0)`), not Error,
    // and must not consume the restart budget.
    assert_eq!(entry.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(entry.exit_code, Some(0));
    assert!(!entry.termination_requested);
    assert!(entry.error.is_none());
    assert!(!entry.desired_enabled);
    let state = controller.state.read().await;
    let record = state.background_children.get("background-clean").expect("record");
    assert_eq!(record.restart_attempts, 0);
}
#[tokio::test]
#[cfg(unix)]
async fn background_nonzero_exit_becomes_error_when_restore_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = false;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let workspace = controller.config.workspace_root.clone();
    controller
        .config
        .exec_sessions
        .create_pipe_session_with_sandbox_and_background(
            "exec-failing-exit".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 1".to_string()],
            workspace,
            Default::default(),
            false,
            true,
        )
        .await
        .expect("failing exec session");
    wait_for_exec_exit(&controller.config.exec_sessions, "exec-failing-exit").await;

    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            "background-failing".to_string(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: "background-failing".to_string(),
                agent_name: "demo".to_string(),
                display_label: "demo".to_string(),
                description: "demo".to_string(),
                source: "test".to_string(),
                color: None,
                session_id: "session-failing".to_string(),
                exec_session_id: "exec-failing-exit".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: None,
                prompt: "demo".to_string(),
                summary: None,
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: None,
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entries = controller.refresh_background_processes().await.expect("refresh");
    let entry = entries.iter().find(|e| e.id == "background-failing").expect("failing entry");
    // Asymmetric oracle vs exit 0: non-zero must stay Error with diagnostic,
    // never collapse to a clean Stopped.
    assert_eq!(entry.status, BackgroundSubprocessStatus::Error);
    assert_eq!(entry.exit_code, Some(1));
    assert!(!entry.termination_requested);
    assert!(entry.error.as_deref().is_some_and(|e| e.contains('1')));
    assert!(entry.desired_enabled);
}
#[tokio::test]
#[cfg(unix)]
async fn background_graceful_stop_stays_stopped_while_process_drains() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let workspace = controller.config.workspace_root.clone();
    controller
        .config
        .exec_sessions
        .create_pipe_session_with_sandbox_and_background(
            "exec-long-running".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace,
            Default::default(),
            false,
            true,
        )
        .await
        .expect("long exec session");

    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            "background-stopping".to_string(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: "background-stopping".to_string(),
                agent_name: "demo".to_string(),
                display_label: "demo".to_string(),
                description: "demo".to_string(),
                source: "test".to_string(),
                color: None,
                session_id: "session-stopping".to_string(),
                exec_session_id: "exec-long-running".to_string(),
                // Graceful stop sets this optimistically before SIGTERM drains.
                desired_enabled: false,
                status: BackgroundSubprocessStatus::Stopped,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: Some(created_at),
                pid: None,
                prompt: "demo".to_string(),
                summary: None,
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: None,
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entries = controller.refresh_background_processes().await.expect("refresh");
    // Must not resurrect to Running while the process still drains; TUI stays
    // non-interfering and the `Exited` arm finalizes later.
    let entry = entries.iter().find(|e| e.id == "background-stopping").expect("stopping entry");
    assert_eq!(entry.status, BackgroundSubprocessStatus::Stopped);
    let state = controller.state.read().await;
    let record = state.background_children.get("background-stopping").expect("record");
    assert_eq!(record.status, BackgroundSubprocessStatus::Stopped);
    assert!(!record.desired_enabled);

    controller.config.exec_sessions.close_session("exec-long-running").await.ok();
}
