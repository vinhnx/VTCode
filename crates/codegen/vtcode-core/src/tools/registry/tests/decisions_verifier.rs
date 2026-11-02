use super::*;

#[tokio::test]
async fn public_decision_requires_canonical_task_and_validates_even_without_evidence() -> Result<()> {
    let workspace = TempDir::new()?;
    let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let args = json!({"summary":"Reuse existing parser","rationale":"Preserve validated input handling"});
    assert!(registry.record_decision_executor(args.clone()).await.is_err());
    registry.set_harness_task(Some("task-test".into()));
    assert!(registry.record_decision_executor(args.clone()).await.is_err());
    let called = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&called);
    registry.set_decision_evidence_validator(Arc::new(move |task, ids| {
        assert_eq!(task, "task-test");
        assert!(ids.is_empty());
        observed.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    assert_eq!(registry.record_decision_executor(args.clone()).await?["recorded"], true);
    assert_eq!(called.load(Ordering::SeqCst), 1);
    let mut budgeted_args = args.clone();
    budgeted_args["max_output_tokens"] = json!(100);
    assert_eq!(registry.execute_tool(tools::RECORD_DECISION, budgeted_args.clone()).await?["recorded"], true);
    assert_eq!(called.load(Ordering::SeqCst), 2);
    budgeted_args["max_output_tokens"] = json!(0);
    assert!(registry.execute_tool(tools::RECORD_DECISION, budgeted_args).await.is_err());
    assert_eq!(called.load(Ordering::SeqCst), 2);
    registry.set_tool_policy(tools::RECORD_DECISION, ToolPolicy::Deny).await?;
    assert!(registry.execute_tool(tools::RECORD_DECISION, args.clone()).await.is_err());
    assert_eq!(called.load(Ordering::SeqCst), 2);
    registry.set_tool_policy(tools::RECORD_DECISION, ToolPolicy::Allow).await?;
    registry.set_harness_task(Some("next-task".into()));
    let observed = Arc::clone(&called);
    registry.set_decision_evidence_validator(Arc::new(move |task, _| {
        assert_eq!(task, "next-task");
        observed.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { anyhow::bail!("canonical persistence failed") })
    }));
    let failed = registry.execute_tool(tools::RECORD_DECISION, args).await?;
    assert!(failed.get("error").is_some(), "{failed}");
    assert_eq!(called.load(Ordering::SeqCst), 3);
    Ok(())
}
#[tokio::test]
async fn rejected_exec_policy_is_not_a_lost_execution_result() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    let mut commands = registry.commands_config();
    commands.deny_list.push("rustc".to_string());
    registry.apply_commands_config(&commands);
    let response = registry
        .execute_tool(tools::EXEC_COMMAND, json!({"cmd": "rustc --version"}))
        .await?;
    assert_eq!(response["error"]["error_type"], "PolicyViolation");
    assert!(response.get("exit_code").is_none());
    assert!(registry.in_progress_exec_sessions(32).await.is_empty());
    Ok(())
}
#[cfg(unix)]
#[tokio::test]
async fn filtered_verifier_reports_checker_and_filter_failures_without_masking() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    fs::create_dir(temp_dir.path().join("scripts"))?;
    for (checker_exit, filter, expected_exit, raw_alias) in [
        (7, "grep README", 7, false),
        (0, "grep README", 0, false),
        (0, "grep missing-pattern", 1, false),
        (0, "grep -E '['", 2, false),
        (7, "grep README", 7, true),
        (0, "grep README", 0, true),
    ] {
        fs::write(
            temp_dir.path().join("scripts/check_markdown.py"),
            format!(
                "import sys\nprint('README: asymmetric evidence')\nprint('unrelated evidence')\nsys.exit({checker_exit})\n"
            ),
        )?;
        let command = format!("python3 scripts/check_markdown.py 2>&1 | {filter}");
        let mut args = json!({"cmd":command, "yield_time_ms":1000});
        if raw_alias {
            args["raw_command"] = json!(command);
        }
        let result = registry.execute_tool(tools::EXEC_COMMAND, args).await?;
        assert_eq!(result["exit_code"], expected_exit, "{result}");
        let output = result["output"].as_str().unwrap();
        assert!(!output.contains("unrelated evidence"), "{output}");
        assert_eq!(output.contains("README: asymmetric evidence"), filter == "grep README", "{output}");
    }
    let mut commands = registry.commands_config();
    commands.deny_list.push("python3".into());
    registry.apply_commands_config(&commands);
    let denied = registry
        .execute_tool(tools::EXEC_COMMAND, json!({"cmd":"python3 scripts/check_markdown.py | grep README"}))
        .await?;
    assert_eq!(denied["error"]["error_type"], "PolicyViolation");
    assert!(denied.get("exit_code").is_none());
    Ok(())
}
#[cfg(unix)]
#[tokio::test]
async fn public_pipe_stdin_defaults_to_eof_and_opt_in_accepts_input() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    let eof = registry
        .execute_tool(
            tools::EXEC_COMMAND,
            json!({
                "cmd": "if read value; then printf 'unexpected:%s' \"$value\"; else printf 'stdin-eof'; fi",
                "shell": "/bin/sh", "yield_time_ms": 1000,
            }),
        )
        .await?;
    assert_eq!(eof["exit_code"], 0);
    assert!(eof["output"].as_str().unwrap().contains("stdin-eof"));

    let active = registry
        .execute_tool(
            tools::EXEC_COMMAND,
            json!({
                "cmd": "read value; printf 'received:%s' \"$value\"",
                "shell": "/bin/sh", "stdin": true, "yield_time_ms": 250,
            }),
        )
        .await?;
    assert!(active.get("exit_code").is_none());
    let session_id = active["session_id"].as_str().unwrap();
    let written = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "chars": "hello\n", "yield_time_ms": 1000,
            }),
        )
        .await?;
    assert_eq!(written["exit_code"], 0);
    assert!(written["output"].as_str().unwrap().contains("received:hello"));
    Ok(())
}
