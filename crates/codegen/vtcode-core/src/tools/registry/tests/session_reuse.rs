use super::*;

#[tokio::test]
async fn active_exec_continuations_bypass_identical_call_loop_detection() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    registry.execution_history.set_loop_detection_limits(5, 2);

    let initial = registry
        .execute_harness_command_session(long_running_exec_args(false, 10))
        .await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();
    let continue_args = json!({
        "session_id": session_id,
        "chars": "",
        "yield_time_ms": 10,
    });

    let first = registry
        .execute_public_tool_ref_prevalidated(tools::WRITE_STDIN, &continue_args)
        .await?;
    assert_ne!(first.get("loop_detected"), Some(&json!(true)));

    let second = registry
        .execute_public_tool_ref_prevalidated(tools::WRITE_STDIN, &continue_args)
        .await?;
    assert_ne!(second.get("loop_detected"), Some(&json!(true)));

    let third = registry
        .execute_public_tool_ref_prevalidated(tools::WRITE_STDIN, &continue_args)
        .await?;
    assert_ne!(third.get("loop_detected"), Some(&json!(true)));
    assert!(
        third.get("exit_code").is_some() || third.get("next_continue_args").is_some(),
        "continuation should either remain active or complete cleanly"
    );

    Ok(())
}
#[tokio::test]
async fn command_session_accepts_compact_session_alias_for_poll() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = registry
        .execute_harness_command_session(long_running_exec_args(true, 10))
        .await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();

    let response = registry
        .execute_harness_command_session(json!({
            "s": session_id,
            "yield_time_ms": 10
        }))
        .await?;

    assert!(response.get("output").is_some());
    assert!(response.get("exit_code").is_some() || response.get("next_continue_args").is_some());

    Ok(())
}
#[tokio::test]
async fn command_session_inspect_accepts_compact_session_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;

    let initial = registry
        .execute_harness_command_session(long_running_exec_args(true, 10))
        .await?;
    let session_id = initial["session_id"]
        .as_str()
        .expect("partial run should expose session_id")
        .to_string();

    let response = registry
        .execute_harness_command_session(json!({
            "action": "inspect",
            "s": session_id,
            "head_lines": 1,
            "tail_lines": 0
        }))
        .await?;

    assert_eq!(response["content_type"], "exec_inspect");
    assert!(response["output"].is_string());
    assert!(response.get("session_id").is_some());

    Ok(())
}
#[tokio::test]
async fn command_session_inspect_rejects_missing_session_without_waiting() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        registry.execute_harness_command_session(json!({
            "action": "inspect",
            "session_id": "run-missing",
            "yield_time_ms": 5000,
        })),
    )
    .await
    .expect("missing session must fail before the five-second yield");
    let source = result.expect_err("missing session must not look like successful empty output");
    let error = ToolExecutionError::from_anyhow(tools::UNIFIED_EXEC, &source, 0, false, false, None);
    assert!(error.is_exec_session_not_found());
    Ok(())
}
#[tokio::test]
#[cfg(unix)]
async fn command_session_inspect_distinguishes_empty_and_closed_sessions() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry
        .exec_sessions
        .create_pipe_session(
            "run-empty".into(),
            vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
            temp_dir.path().to_path_buf(),
            Default::default(),
        )
        .await?;
    let args = json!({"action": "inspect", "s": "run-empty", "yield_time_ms": 0});
    let empty = registry.execute_harness_command_session(args.clone()).await?;
    assert_eq!(empty["success"], true);
    assert_eq!(empty["output"], "");
    registry.close_exec_session("run-empty").await?;
    let source = registry
        .execute_harness_command_session(args)
        .await
        .expect_err("closed session is unavailable");
    let error = ToolExecutionError::from_anyhow(tools::UNIFIED_EXEC, &source, 0, false, false, None);
    assert!(error.is_exec_session_not_found());
    Ok(())
}
#[tokio::test]
#[cfg(unix)]
async fn command_session_wait_returns_partial_output_when_session_closes_mid_wait() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    // The fixture emits output three times over ~600ms, so the wait loop's
    // 50ms drain cycle provably buffers it long before the fixture closes
    // the session — the same concurrent-removal actor as a TUI force-cancel
    // closing a foreground session under an in-flight wait.
    registry
        .exec_sessions
        .create_pipe_session(
            "run-mid-wait-close".into(),
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "for i in 1 2 3; do echo mid-wait-output; sleep 0.2; done; sleep 30".into(),
            ],
            temp_dir.path().to_path_buf(),
            Default::default(),
        )
        .await?;

    let (close_side, wait_side) = tokio::join!(
        async {
            // 2s leaves a 30x+ margin over the fixture's last emission on a
            // loaded CI runner while keeping the test fast (wait deadline
            // is 30s, so the close always lands mid-wait).
            tokio::time::sleep(Duration::from_millis(2000)).await;
            registry.close_exec_session("run-mid-wait-close").await?;
            anyhow::Ok(())
        },
        registry.execute_harness_command_session(json!({
            "action": "wait",
            "s": "run-mid-wait-close",
            "wait_timeout_seconds": 30,
        }))
    );
    close_side.expect("close side must succeed");

    // join! completes only when both sides settle: the wait either observes
    // the mid-wait close or runs out its own deadline.
    let response = wait_side.expect("a session closed mid-wait must return its buffered output, not an error");
    let output = response["output"].as_str().unwrap_or_default();
    assert!(output.contains("mid-wait-output"), "output buffered before the close must survive: {response}");
    assert!(
        output.contains("closed while waiting"),
        "the response must disclose that the capture is partial: {response}"
    );

    Ok(())
}
#[tokio::test]
async fn mutating_tools_clear_recent_read_reuse_history() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    registry.execution_history.set_loop_detection_limits(5, 2);

    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "original")?;

    let read_args = json!({
        "path": test_file.to_string_lossy(),
        "max_bytes": 1000,
    });
    let write_args = json!({
        "path": test_file.to_string_lossy(),
        "content": "modified",
        "mode": "overwrite",
    });

    let first = registry.execute_tool_ref(tools::READ_FILE, &read_args).await?;
    assert_eq!(first["content"], "original");

    let second = registry.execute_tool(tools::READ_FILE, read_args.clone()).await?;
    assert_eq!(second["content"], "original");

    let write_result = registry.execute_tool(tools::WRITE_FILE, write_args).await?;
    assert_eq!(write_result["success"], json!(true));

    let after_write = registry.execute_tool(tools::READ_FILE, read_args).await?;
    assert_eq!(after_write["content"], "modified");
    assert_ne!(after_write.get("reused_recent_result"), Some(&json!(true)));
    assert_ne!(after_write.get("loop_detected"), Some(&json!(true)));

    Ok(())
}
#[tokio::test]
async fn read_only_command_session_results_are_fast_reused() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "hello")?;

    let cat_args = json!({
        "cmd": format!("cat {}", test_file.to_string_lossy()),
    });

    let first = registry.execute_tool_ref(tools::EXEC_COMMAND, &cat_args).await?;
    assert!(first.get("reused_recent_result").is_none(), "first call should not be reused");

    let second = registry.execute_tool_ref(tools::EXEC_COMMAND, &cat_args).await?;
    assert_eq!(
        second.get("reused_recent_result"),
        Some(&json!(true)),
        "second identical read-only exec call should reuse the first result"
    );

    Ok(())
}
#[tokio::test]
async fn web_fetch_structured_errors_are_not_reused_as_successful_results() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;

    let args = json!({
        "url": "http://example.com",
    });

    let first = registry.execute_tool_ref(tools::WEB_FETCH, &args).await?;
    assert!(
        first
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error.contains("Only HTTPS URLs are allowed")),
        "first web_fetch call should return the structured tool error"
    );

    let second = registry.execute_tool_ref(tools::WEB_FETCH, &args).await?;
    assert!(
        second.get("reused_recent_result").is_none(),
        "failed web_fetch output must not be cached as a successful read-only result"
    );
    assert!(
        second.get("loop_detected").is_none(),
        "failed web_fetch output must not count toward identical successful-call loops"
    );

    Ok(())
}
