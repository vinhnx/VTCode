use super::*;

#[tokio::test]
async fn executes_prevalidated_tool_path() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let args = json!({"input": "value"});
    let response = registry.execute_tool_ref_prevalidated(CUSTOM_TOOL_NAME, &args).await?;
    assert!(response["success"].as_bool().unwrap_or(false));

    Ok(())
}
#[tokio::test]
async fn harness_exec_reuses_public_output_normalization() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let response = registry
        .execute_harness_command_session(json!({
            "action": "run",
            "command": "printf vtcode",
            "tty": false,
            "yield_time_ms": 1000
        }))
        .await?;

    assert_eq!(response["output"].as_str(), Some("vtcode"));
    assert!(response.get("stdout").is_none());

    Ok(())
}
#[tokio::test]
async fn harness_terminal_runs_retain_completed_sessions_until_close() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    let active_pty_sessions = Arc::new(AtomicUsize::new(0));
    registry.set_active_pty_sessions(Arc::clone(&active_pty_sessions));

    let response = registry
        .execute_harness_command_session_terminal_run(json!({
            "action": "run",
            "command": ["/bin/sh", "-lc", "printf vtcode-terminal"],
            "tty": true,
            "yield_time_ms": 200,
        }))
        .await?;

    let session_id = response["session_id"]
        .as_str()
        .expect("terminal run should expose session_id")
        .to_string();
    assert_eq!(response["exit_code"], 0);
    assert_eq!(response["output"].as_str(), Some("vtcode-terminal"));
    // A completed command releases its foreground-PTY count immediately; the
    // retention contract applies to the session record, not the live count.
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    assert_eq!(registry.harness_exec_session_completed(&session_id).await?, Some(0));

    registry.close_harness_exec_session(&session_id).await?;
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    registry.harness_exec_session_completed(&session_id).await.unwrap_err();

    Ok(())
}
#[tokio::test]
async fn background_terminal_runs_do_not_mark_the_ui_busy() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    let active_pty_sessions = Arc::new(AtomicUsize::new(0));
    registry.set_active_pty_sessions(Arc::clone(&active_pty_sessions));

    let response = registry
        .execute_harness_command_session_terminal_run(json!({
            "action": "run",
            "command": ["/bin/sh", "-lc", "sleep 5"],
            "tty": true,
            "background": true,
            "yield_time_ms": 200,
        }))
        .await?;
    let session_id = response["session_id"]
        .as_str()
        .expect("background terminal run should expose session_id");

    assert_eq!(response["background"], true);
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);

    registry.close_harness_exec_session(session_id).await?;
    assert_eq!(active_pty_sessions.load(Ordering::Relaxed), 0);
    Ok(())
}
#[tokio::test]
async fn background_exec_returns_reusable_session_metadata_and_drains_on_wait() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = tokio::time::timeout(
        Duration::from_secs(3),
        registry.execute_harness_command_session(json!({
            "action": "run",
            "command": "printf first; sleep 0.4; printf second",
            "tty": false,
            "background": true,
            "yield_time_ms": 250,
        })),
    )
    .await??;
    let session_id = initial["session_id"]
        .as_str()
        .expect("background run should expose session_id")
        .to_string();

    assert_eq!(initial["background"], true);
    assert_eq!(initial["lifecycle_state"], "running");
    assert!(initial["child_pid"].as_u64().is_some_and(|pid| pid > 0));
    assert_eq!(initial["next_wait_args"]["session_id"], session_id);
    assert_eq!(initial["next_continue_args"]["session_id"], session_id);
    assert!(initial["output"].as_str().unwrap_or_default().contains("first"));

    let completed = tokio::time::timeout(
        Duration::from_secs(5),
        registry.execute_harness_command_session(json!({
            "action": "wait",
            "session_id": session_id,
            "wait_timeout_seconds": 3,
        })),
    )
    .await??;
    assert_eq!(completed["background"], true);
    assert_eq!(completed["exit_code"], 0);
    assert!(completed["output"].as_str().unwrap_or_default().contains("second"));

    registry
        .execute_harness_command_session(json!({
            "action": "close",
            "session_id": session_id,
        }))
        .await?;
    Ok(())
}
#[tokio::test]
async fn prevalidated_exec_mode_settles_noninteractive_run() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let response = registry
        .execute_public_tool_ref_prevalidated_with_mode(
            tools::EXEC_COMMAND,
            &delayed_exec_args(false, 50),
            ExecSettlementMode::SettleNonInteractive,
        )
        .await?;

    let output = response["output"].as_str().expect("settled exec output should be text");
    assert!(output.contains("first"));
    assert!(output.contains("second"));
    assert_eq!(response["exit_code"], 0);
    assert!(response.get("next_continue_args").is_none());

    Ok(())
}
#[tokio::test]
async fn prevalidated_exec_mode_settles_pipe_poll_until_exit() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = registry.execute_harness_command_session(delayed_exec_args(false, 50)).await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();
    let initial_output = initial["output"].as_str().unwrap_or_default().to_string();
    if initial.get("next_continue_args").is_none() {
        assert_eq!(initial["exit_code"], 0);
        assert!(initial_output.contains("first"));
        assert!(initial_output.contains("second"));
        return Ok(());
    }
    assert!(initial.get("exit_code").is_none());

    let response = registry
        .execute_public_tool_ref_prevalidated_with_mode(
            tools::WRITE_STDIN,
            &json!({
                "session_id": session_id,
                "chars": "",
                "yield_time_ms": 50,
            }),
            ExecSettlementMode::SettleNonInteractive,
        )
        .await?;

    assert_eq!(response["exit_code"], 0);
    let settled_output = response["output"].as_str().expect("settled poll output should be text");
    assert!(initial_output.contains("second") || settled_output.contains("second"));
    assert!(response.get("next_continue_args").is_none());

    Ok(())
}
#[tokio::test]
async fn prevalidated_exec_mode_keeps_interactive_runs_manual() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let response = registry
        .execute_public_tool_ref_prevalidated_with_mode(
            tools::EXEC_COMMAND,
            &long_running_exec_args(true, 50),
            ExecSettlementMode::SettleNonInteractive,
        )
        .await?;

    assert!(response.get("next_continue_args").is_some());
    assert!(response.get("exit_code").is_none());

    let session_id = response["session_id"]
        .as_str()
        .expect("interactive run should expose session_id")
        .to_string();
    registry
        .execute_harness_command_session(json!({
            "action": "close",
            "session_id": session_id,
        }))
        .await?;

    Ok(())
}
#[tokio::test]
async fn command_session_run_preserves_requested_session_id_for_follow_up_calls() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let mut run_args = long_running_exec_args(true, 10);
    run_args
        .as_object_mut()
        .expect("run args should be an object")
        .insert("session_id".to_string(), json!("check_sh"));

    let initial = registry.execute_harness_command_session(run_args).await?;
    assert_eq!(initial["session_id"], "check_sh");
    assert_eq!(initial["next_continue_args"], json!({ "session_id": "check_sh" }));

    let response = registry
        .execute_harness_command_session(json!({
            "action": "poll",
            "session_id": "check_sh",
            "yield_time_ms": 10,
        }))
        .await?;

    assert!(response.get("output").is_some());
    assert!(response.get("exit_code").is_some() || response.get("next_continue_args").is_some());

    if response.get("exit_code").is_none() {
        registry
            .execute_harness_command_session(json!({
                "action": "close",
                "session_id": "check_sh",
            }))
            .await?;
    }

    Ok(())
}
