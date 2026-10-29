use super::*;

#[tokio::test]
async fn spawn_honors_model_override_when_user_explicitly_requests_it() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            model: Some(models::openai::GPT_5_6_LUNA.to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let effective_model = wait_for_effective_model(&controller, &spawned.id)
        .await
        .expect("effective model");
    assert_eq!(effective_model, models::openai::GPT_5_6_LUNA);
    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_background_subprocess_rejects_non_background_agent() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let err = controller
        .spawn_background_subprocess(SpawnBackgroundSubprocessRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnBackgroundSubprocessRequest::default()
        })
        .await
        .expect_err("non-background agent should be rejected");

    assert!(err.to_string().contains("background: true"));
    assert!(err.to_string().contains("Use spawn_agent instead"));
}
#[tokio::test]
async fn spawn_background_subprocess_returns_active_record_when_settings_match() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: record_id.clone(),
                agent_name: spec.name.clone(),
                display_label: subagent_display_label(&spec),
                description: spec.description.clone(),
                source: spec.source.label(),
                color: spec.color.clone(),
                session_id: "session-background-demo".to_string(),
                exec_session_id: "exec-session-background-demo".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: Some(42),
                prompt: "Report readiness once.".to_string(),
                summary: Some("ready".to_string()),
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: Some(4),
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entry = controller
        .spawn_background_subprocess(SpawnBackgroundSubprocessRequest {
            agent_type: Some("background-demo".to_string()),
            ..SpawnBackgroundSubprocessRequest::default()
        })
        .await
        .expect("matching active record should be returned");

    assert_eq!(entry.id, record_id);
    assert_eq!(entry.status, BackgroundSubprocessStatus::Running);
    assert_eq!(entry.pid, Some(42));
}
#[tokio::test]
async fn spawn_background_subprocess_rejects_conflicting_active_record_settings() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id,
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: background_record_id(spec.name.as_str()),
                agent_name: spec.name.clone(),
                display_label: subagent_display_label(&spec),
                description: spec.description.clone(),
                source: spec.source.label(),
                color: spec.color.clone(),
                session_id: "session-background-demo".to_string(),
                exec_session_id: "exec-session-background-demo".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: Some(42),
                prompt: "Report readiness once.".to_string(),
                summary: Some("ready".to_string()),
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: Some(4),
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let err = controller
        .spawn_background_subprocess(SpawnBackgroundSubprocessRequest {
            agent_type: Some("background-demo".to_string()),
            message: Some("Run a different task.".to_string()),
            ..SpawnBackgroundSubprocessRequest::default()
        })
        .await
        .expect_err("conflicting active record should be rejected");

    assert!(err.to_string().contains("different prompt"));
    assert!(err.to_string().contains("Stop or restart"));
}
#[tokio::test]
async fn wait_for_background_returns_stopped_record_immediately() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Stopped, false, ""),
        );
    }

    let entry = controller
        .wait_for_background(std::slice::from_ref(&record_id), Some(50))
        .await
        .expect("wait")
        .expect("stopped record should complete immediately");
    assert_eq!(entry.id, record_id);
    assert_eq!(entry.status, BackgroundSubprocessStatus::Stopped);
}
#[tokio::test]
async fn wait_for_background_times_out_on_running_record() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(
                &spec,
                &record_id,
                BackgroundSubprocessStatus::Running,
                true,
                "exec-session-missing",
            ),
        );
    }

    let entry = controller
        .wait_for_background(std::slice::from_ref(&record_id), Some(50))
        .await
        .expect("wait");
    assert!(entry.is_none(), "running record should time out, not hallucinate completion");
}
#[tokio::test]
async fn wait_for_background_is_fail_closed_for_unknown_and_empty_targets() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    assert!(
        controller
            .wait_for_background(&["background-missing".to_string()], Some(10))
            .await
            .expect("wait")
            .is_none()
    );
    assert!(controller.wait_for_background(&[], Some(10)).await.expect("wait").is_none());
}
#[tokio::test]
#[cfg(unix)]
async fn managed_background_completion_persists_before_delivery() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-clean-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.1; exit 0".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(!event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(event.exit_code, Some(0));
    let status = controller
        .background_status_entries()
        .await
        .into_iter()
        .find(|entry| entry.id == record_id)
        .expect("terminal record remains visible without refresh");
    assert_eq!(status.status, BackgroundSubprocessStatus::Stopped);

    let persisted = load_background_state(temp.path()).await.expect("persisted state");
    let persisted_record = persisted
        .records
        .iter()
        .find(|record| record.id == record_id)
        .expect("record persisted");
    let persisted_json = serde_json::to_value(persisted_record).expect("persisted record serializes");
    assert_eq!(persisted_json["status"], "stopped");
    assert!(
        persisted_json["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("successfully"))
    );

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}
#[tokio::test]
#[cfg(unix)]
async fn managed_background_graceful_stop_delivers_one_terminal_completion() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-graceful-stop-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let stopped = controller.graceful_stop_background(&record_id).await.expect("graceful stop");
    assert_eq!(stopped.status, BackgroundSubprocessStatus::Stopped);
    assert!(!stopped.desired_enabled);

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("stop completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert!(event.error.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), completions.recv())
            .await
            .is_err()
    );

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}
#[tokio::test]
#[cfg(unix)]
async fn managed_background_force_cancel_delivers_one_terminal_completion() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-force-cancel-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let stopped = controller.force_cancel_background(&record_id).await.expect("force cancel");
    assert_eq!(stopped.status, BackgroundSubprocessStatus::Stopped);
    assert!(!stopped.desired_enabled);

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("force-cancel completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(event.exit_code, None);
    assert!(event.error.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), completions.recv())
            .await
            .is_err()
    );

    controller.signal_shutdown().await;
}
#[tokio::test]
#[cfg(unix)]
async fn parent_subscription_replays_completion_published_before_receiver() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-completed-before-parent-subscribe";

    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }
    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.1; exit 0".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let stopped = controller
                .background_status_entries()
                .await
                .into_iter()
                .any(|entry| entry.id == record_id && entry.status == BackgroundSubprocessStatus::Stopped);
            if stopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("monitor should persist terminal state before subscription");

    let mut completions = controller.subscribe_parent_background_completions();
    let event = tokio::time::timeout(Duration::from_secs(1), completions.recv())
        .await
        .expect("parent subscription should replay completion")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(!event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(event.exit_code, Some(0));

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}
#[tokio::test]
async fn background_completion_monitor_shutdown_cancels_and_joins_task() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    assert!(controller.background_completion_monitor.lock().await.is_some());
    controller.signal_shutdown().await;
    assert!(controller.background_completion_shutdown.is_cancelled());
    assert!(controller.background_completion_monitor.lock().await.is_none());
}
#[tokio::test]
async fn background_completion_monitor_is_cancelled_when_last_owner_drops() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let shutdown = controller.background_completion_shutdown.clone();
    let monitor_slot = Arc::clone(&controller.background_completion_monitor);
    let controller_clone = controller.clone();

    drop(controller);
    assert!(!shutdown.is_cancelled(), "a live clone must keep the monitor running");

    drop(controller_clone);
    assert!(shutdown.is_cancelled(), "dropping the final owner must cancel the monitor");
    assert!(monitor_slot.lock().await.is_none(), "the monitor handle must be reclaimed");
}
#[tokio::test]
#[cfg(unix)]
async fn managed_background_nonzero_completion_is_delivered_as_error() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = false;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-error-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.1; exit 9".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert_eq!(event.status, BackgroundSubprocessStatus::Error);
    assert_eq!(event.exit_code, Some(9));
    assert!(event.error.as_deref().is_some_and(|error| error.contains('9')));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), completions.recv())
            .await
            .is_err()
    );

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}
#[tokio::test]
#[cfg(unix)]
async fn wait_for_background_races_completion_notification() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = false;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-wait-notification";

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.2; exit 0".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let started = std::time::Instant::now();
    let entry = controller
        .wait_for_background(std::slice::from_ref(&record_id), Some(2_000))
        .await
        .expect("wait");
    assert_eq!(entry.expect("completion").status, BackgroundSubprocessStatus::Stopped);
    assert!(started.elapsed() < Duration::from_secs(2));

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}
/// The unified `agent wait` surface must return a settled target in either
/// scope promptly, instead of blocking out a still-running target in the
/// other. A running delegated child plus an already-stopped background
/// subprocess must resolve on the background entry well before the child's
/// (5s) timeout would expire.
#[tokio::test]
async fn agent_wait_returns_settled_background_entry_without_waiting_out_delegated_child() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = Arc::new(
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
            .await
            .expect("controller"),
    );

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let background_id = background_record_id(spec.name.as_str());
    let delegated_spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .expect("default");
    {
        let mut state = controller.state.write().await;
        state.children.insert(
            "delegated-running".to_string(),
            test_child_record(
                "delegated-running",
                "session-delegated",
                "parent-session",
                &delegated_spec,
                SubagentStatus::Running,
                1,
                None,
            ),
        );
        state.background_children.insert(
            background_id.clone(),
            test_background_record(&spec, &background_id, BackgroundSubprocessStatus::Stopped, false, ""),
        );
    }

    let registry = crate::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.set_subagent_controller(Arc::clone(&controller));

    let started = std::time::Instant::now();
    let response = registry
        .agent_executor(serde_json::json!({
            "action": "wait",
            "ids": ["delegated-running", background_id],
            "timeout_ms": 5_000
        }))
        .await
        .expect("agent wait");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "settled background target must not wait out the delegated child: {elapsed:?}"
    );
    assert_eq!(response["completed"], serde_json::json!(true));
    assert_eq!(response["entry"]["id"], serde_json::json!(background_id));
    assert_eq!(response["entry"]["status"], serde_json::json!("stopped"));
}
