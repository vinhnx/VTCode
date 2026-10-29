use super::*;

#[test]
fn recent_records_and_failures_respect_count_and_chronology() {
    let history = ToolExecutionHistory::new(10);
    for index in 0..5 {
        let record = if index % 2 == 0 {
            ToolExecutionRecord::success(
                format!("tool_{index}"),
                format!("tool_{index}"),
                false,
                None,
                json!({}),
                json!("ok"),
                make_snapshot(),
                None,
                None,
                None,
                None,
                false,
            )
        } else {
            ToolExecutionRecord::failure(
                format!("tool_{index}"),
                format!("tool_{index}"),
                false,
                None,
                json!({}),
                format!("failed {index}"),
                make_snapshot(),
                None,
                None,
                None,
                None,
                false,
            )
        };
        history.add_record(record);
    }

    // Constructors agree with the record outcome on both sides.
    let recent = history.get_recent_records(10);
    assert_eq!(recent.len(), 5);
    for record in &recent {
        assert_eq!(record.success, record.result.is_ok());
    }
    // Chronological order: earliest first.
    assert_eq!(recent.first().expect("first").tool_name, "tool_0");
    assert_eq!(recent.last().expect("last").tool_name, "tool_4");

    // count=0 boundary yields nothing without panic.
    assert!(history.get_recent_records(0).is_empty());
    assert!(history.get_recent_failures(0).is_empty());

    // More failures exist than requested: the newest wins, oldest first.
    assert_eq!(recent.len(), 5);
    let failures = history.get_recent_failures(1);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].tool_name, "tool_3");

    let failures = history.get_recent_failures(10);
    assert_eq!(failures.iter().map(|r| r.tool_name.as_str()).collect::<Vec<_>>(), vec!["tool_1", "tool_3"]);
}
#[test]
fn finds_recent_spooled_result() {
    let args = json!({"command": "git diff"});
    let temp = tempdir().unwrap();
    let history = ToolExecutionHistory::with_workspace_root(10, temp.path().to_path_buf());
    let spool_path = ".vtcode/context/tool_outputs/spooled-output.txt";
    let full_path = temp.path().join(spool_path);
    std::fs::create_dir_all(full_path.parent().expect("spool parent")).unwrap();
    std::fs::write(&full_path, "diff output").unwrap();
    let result = json!({
        "spool_path": spool_path,
        "spool_state": "completed",
        "spooled_bytes": 11,
        "spool_sha256": vtcode_commons::utils::calculate_sha256(b"diff output"),
        "success": true
    });

    history.add_record(ToolExecutionRecord::success(
        "run_pty_cmd".to_string(),
        "run_pty_cmd".to_string(),
        false,
        None,
        args.clone(),
        result.clone(),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_spooled_result("run_pty_cmd", &args, Duration::from_secs(60));
    assert_eq!(found, Some(result));

    std::fs::write(&full_path, "changed-out").unwrap();
    assert!(
        history
            .find_recent_spooled_result("run_pty_cmd", &args, Duration::from_secs(60))
            .is_none()
    );
}
#[test]
fn task_telemetry_snapshot_counts_tool_surface_metrics() {
    let history = ToolExecutionHistory::new(10);
    let task = "repo_task_1";
    let command_args = json!({
        "cmd": "rg ToolTaskTelemetrySnapshot vtcode-core/src",
        "sandbox_permissions": "require_escalated",
    });
    let spool_path = "/tmp/vtcode-spool-1.txt";

    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_EXEC.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        command_args.clone(),
        json!({"spool_path": spool_path}),
        make_task_snapshot(task),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_EXEC.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        command_args,
        json!({"status": "ok"}),
        make_task_snapshot(task),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_EXEC.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"spool_path": spool_path, "query": "warning"}),
        json!({"spool_path": spool_path, "matches": []}),
        make_task_snapshot(task),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        json!({"query": "ToolRegistry", "result_types": ["definition"]}),
        json!({"query": "ToolRegistry", "filters": {"path": ".", "file_types": [], "result_types": ["definition"], "max_results": 20}, "results": [], "returned": 0, "truncated": false, "hints": []}),
        make_task_snapshot(task),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::failure(
        tools::UNIFIED_FILE.to_string(),
        "file_operation".to_string(),
        false,
        None,
        json!({"input": "*** Begin Patch\n*** End Patch\n"}),
        "invalid patch".to_string(),
        make_task_snapshot(task),
        None,
        None,
        None,
        None,
        false,
    ));

    let snapshot = history.task_telemetry_snapshot(Some(task), Some(false));
    assert_eq!(snapshot.total_tool_calls, 5);
    assert_eq!(snapshot.repeated_equivalent_calls, 1);
    assert_eq!(snapshot.failed_tool_calls, 1);
    assert_eq!(snapshot.spooled_outputs, 1);
    assert_eq!(snapshot.fallback_calls, 0);
    assert_eq!(snapshot.read_after_spool_calls, 1);
    assert_eq!(snapshot.command_approval_prompts, 2);
    assert_eq!(snapshot.task_completed_successfully, Some(false));
    assert_eq!(snapshot.calls_by_tool.get(tools::EXEC_COMMAND), Some(&3));
    assert_eq!(snapshot.calls_by_tool.get(tools::CODE_SEARCH), Some(&1));
    assert_eq!(snapshot.calls_by_tool.get("file_operation"), Some(&1));
    assert!(!snapshot.calls_by_tool.keys().any(|label| label.contains("unified_")));

    let json = snapshot.to_json();
    assert_eq!(json["total_tool_calls"], 5);
    assert_eq!(json["task_completed_successfully"], false);
}
#[test]
fn ignores_non_spooled_or_stale_results() {
    let history = ToolExecutionHistory::new(10);
    let args = json!({"path": "README.md"});

    let mut record = ToolExecutionRecord::success(
        "read_file".to_string(),
        "read_file".to_string(),
        false,
        None,
        args.clone(),
        json!({"content": "small"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    );
    record.timestamp = SystemTime::UNIX_EPOCH;
    history.add_record(record);

    let found = history.find_recent_spooled_result("read_file", &args, Duration::from_secs(60));
    assert!(found.is_none());
}
#[test]
fn ignores_spooled_result_when_spool_file_is_missing() {
    let history = ToolExecutionHistory::new(10);
    let args = json!({"command": "cargo clippy"});
    let missing_spool_path = tempdir().unwrap().path().join("missing_spool.txt");
    let result = json!({
        "spool_path": missing_spool_path,
        "success": true
    });

    history.add_record(ToolExecutionRecord::success(
        "run_pty_cmd".to_string(),
        "run_pty_cmd".to_string(),
        false,
        None,
        args.clone(),
        result,
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_spooled_result("run_pty_cmd", &args, Duration::from_secs(60));
    assert!(found.is_none());
}
#[test]
fn find_recent_successful_result_skips_missing_spool_file() {
    let history = ToolExecutionHistory::new(10);
    let args = json!({"command": "cargo clippy"});
    let missing_spool_path = tempdir().unwrap().path().join("missing_spool.txt");
    let result = json!({
        "spool_path": missing_spool_path,
        "success": true
    });

    history.add_record(ToolExecutionRecord::success(
        "run_pty_cmd".to_string(),
        "run_pty_cmd".to_string(),
        false,
        None,
        args.clone(),
        result,
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_successful_result("run_pty_cmd", &args, Duration::from_secs(60));
    assert!(found.is_none());
}
