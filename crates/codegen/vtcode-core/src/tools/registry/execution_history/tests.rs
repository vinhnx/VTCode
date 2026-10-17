use super::*;
use serde_json::json;
use tempfile::tempdir;

fn make_snapshot() -> HarnessContextSnapshot {
    HarnessContextSnapshot::new("session_test".to_string(), None)
}

fn make_task_snapshot(task_id: &str) -> HarnessContextSnapshot {
    HarnessContextSnapshot::new("session_test".to_string(), Some(task_id.to_string()))
}

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

#[test]
fn len_tracks_records_and_clear() {
    let history = ToolExecutionHistory::new(10);
    assert_eq!(history.len(), 0);
    assert!(history.is_empty());

    history.add_record(ToolExecutionRecord::success(
        "read_file".to_string(),
        "read_file".to_string(),
        false,
        None,
        json!({"path": "README.md"}),
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    assert_eq!(history.len(), 1);
    assert!(!history.is_empty());

    history.clear();
    assert_eq!(history.len(), 0);
    assert!(history.is_empty());
}

#[test]
fn invalidate_all_reads_drops_read_records_only() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        "read_file".to_string(),
        "read_file".to_string(),
        false,
        None,
        json!({"path": "src/main.rs"}),
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        "code_search".to_string(),
        "code_search".to_string(),
        false,
        None,
        json!({"query": "fn main"}),
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    history.invalidate_all_reads();
    assert_eq!(history.len(), 1, "code_search record must survive");
}

#[test]
fn finds_recent_read_file_spool_progress() {
    let history = ToolExecutionHistory::new(10);
    let args = json!({"path": ".vtcode/context/tool_outputs/command_session_123.txt"});
    let result = json!({
        "success": true,
        "spool_chunked": true,
        "has_more": true,
        "next_read_args": {
            "path": ".vtcode/context/tool_outputs/command_session_123.txt",
            "offset": 41,
            "limit": 40
        }
    });

    history.add_record(ToolExecutionRecord::success(
        "read_file".to_string(),
        "read_file".to_string(),
        false,
        None,
        args,
        result,
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_read_file_spool_progress(
        ".vtcode/context/tool_outputs/command_session_123.txt",
        Duration::from_secs(60),
    );
    assert_eq!(found, Some((41, 40)));
}

#[test]
fn finds_recent_file_operation_read_spool_progress() {
    let history = ToolExecutionHistory::new(10);
    let args = json!({
        "action": "read",
        "path": ".vtcode/context/tool_outputs/command_session_456.txt"
    });
    let result = json!({
        "success": true,
        "spool_chunked": true,
        "has_more": true,
        "next_read_args": {
            "path": ".vtcode/context/tool_outputs/command_session_456.txt",
            "offset": 81,
            "limit": 40
        }
    });

    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        args,
        result,
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_read_file_spool_progress(
        ".vtcode/context/tool_outputs/command_session_456.txt",
        Duration::from_secs(60),
    );
    assert_eq!(found, Some((81, 40)));
}

#[test]
fn matches_read_file_alias_name_and_abs_relative_spool_path() {
    let history = ToolExecutionHistory::new(10);
    let rel_path = ".vtcode/context/tool_outputs/command_session_789.txt";
    let abs_path = env::current_dir().unwrap().join(rel_path);
    let args = json!({
        "path": abs_path,
        "offset": 1,
        "limit": 40
    });
    let result = json!({
        "success": true,
        "spool_chunked": true,
        "has_more": true,
        "next_read_args": {
            "path": rel_path,
            "offset": 41,
            "limit": 40
        }
    });

    history.add_record(ToolExecutionRecord::success(
        "Read file".to_string(),
        "Read file".to_string(),
        false,
        None,
        args,
        result,
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_read_file_spool_progress(rel_path, Duration::from_secs(60));
    assert_eq!(found, Some((41, 40)));
}

#[test]
fn matches_prefixed_read_file_tool_name() {
    let history = ToolExecutionHistory::new(10);
    let path = ".vtcode/context/tool_outputs/command_session_prefixed.txt";
    let args = json!({ "path": path });
    let result = json!({
        "success": true,
        "spool_chunked": true,
        "has_more": true,
        "next_read_args": {
            "path": path,
            "offset": 121,
            "limit": 40
        }
    });

    history.add_record(ToolExecutionRecord::success(
        "repo_browser.read_file".to_string(),
        "repo_browser.read_file".to_string(),
        false,
        None,
        args,
        result,
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_read_file_spool_progress(path, Duration::from_secs(60));
    assert_eq!(found, Some((121, 40)));
}

#[test]
fn ignores_read_file_spool_progress_without_canonical_args() {
    let history = ToolExecutionHistory::new(10);
    let path = ".vtcode/context/tool_outputs/command_session_legacy.txt";
    let args = json!({"path": path});
    let result = json!({
        "success": true,
        "spool_chunked": true,
        "has_more": true,
        "next_offset": 33,
        "chunk_limit": 32
    });

    history.add_record(ToolExecutionRecord::success(
        "read_file".to_string(),
        "read_file".to_string(),
        false,
        None,
        args,
        result,
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let found = history.find_recent_read_file_spool_progress(path, Duration::from_secs(60));
    assert_eq!(found, None);
}

#[test]
fn readonly_file_operation_calls_use_lower_identical_limit() {
    let history = ToolExecutionHistory::new(10);
    history.set_loop_detection_limits(5, 2);

    let args = json!({
        "action": "read",
        "path": "crates/codegen/vtcode-core/src/core/agent/runner/tests.rs"
    });

    // The effective limit is max(base_limit, MIN_READONLY_IDENTICAL_LIMIT).
    // With MIN_READONLY_IDENTICAL_LIMIT=2, the limit matches the base.
    assert_eq!(history.loop_limit_for(tools::UNIFIED_FILE, &args), 2);
}

fn loop_test_record(tool_name: &str, args: Value) -> ToolExecutionRecord {
    ToolExecutionRecord::success(
        tool_name.to_string(),
        tool_name.to_string(),
        false,
        None,
        args,
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    )
}

#[test]
fn tracker_loop_detection_resets_after_successful_intervening_calls() {
    let list_args = json!({"action":"list"});
    let completion_args = json!({"action":"update", "items":["[x] Check README"]});
    for args in [list_args, completion_args] {
        for intervening in [
            loop_test_record(tools::TASK_TRACKER, json!({"action":"update", "index":1, "status":"in_progress"})),
            loop_test_record(tools::EXEC_COMMAND, json!({"cmd":"cargo check --locked"})),
            loop_test_record(tools::READ_FILE, json!({"path":"README.md"})),
        ] {
            let history = ToolExecutionHistory::new(20);
            history.set_loop_detection_limits(8, 2);
            for _ in 0..2 {
                history.add_record(loop_test_record(tools::TASK_TRACKER, args.clone()));
            }
            assert!(history.detect_loop(tools::TASK_TRACKER, &args).detected);
            history.add_record(intervening);
            assert_eq!(history.detect_loop(tools::TASK_TRACKER, &args).repeat_count, 0);

            history.add_record(loop_test_record(tools::TASK_TRACKER, args.clone()));
            let after_one = history.detect_loop(tools::TASK_TRACKER, &args);
            assert!(!after_one.detected);
            assert_eq!(after_one.repeat_count, 1);
            history.add_record(loop_test_record(tools::TASK_TRACKER, args.clone()));
            let after_two = history.detect_loop(tools::TASK_TRACKER, &args);
            assert!(after_two.detected);
            assert_eq!(after_two.repeat_count, 2);
        }
    }
}

#[test]
fn tracker_loop_detection_does_not_reset_after_failed_calls() {
    let history = ToolExecutionHistory::new(20);
    history.set_loop_detection_limits(8, 2);
    let args = json!({"action":"list"});
    for _ in 0..2 {
        history.add_record(loop_test_record(tools::TASK_TRACKER, args.clone()));
    }
    for tool_name in [tools::TASK_TRACKER, tools::EXEC_COMMAND] {
        let mut failure = loop_test_record(tool_name, json!({"action":"update", "index":2, "status":"completed"}));
        failure.success = false;
        failure.result = Err("denied by policy".to_string());
        history.add_record(failure);
        let result = history.detect_loop(tools::TASK_TRACKER, &args);
        assert!(result.detected);
        assert_eq!(result.repeat_count, 2);
    }
}

#[test]
fn tracker_streak_rule_does_not_change_other_tool_loop_detection() {
    let history = ToolExecutionHistory::new(20);
    history.set_loop_detection_limits(8, 2);
    let args = json!({"query":"same search"});
    for _ in 0..2 {
        history.add_record(loop_test_record(tools::CODE_SEARCH, args.clone()));
        history.add_record(loop_test_record(tools::TASK_TRACKER, json!({"action":"list"})));
    }
    let result = history.detect_loop(tools::CODE_SEARCH, &args);
    assert!(result.detected);
    assert_eq!(result.repeat_count, 2);
}

#[test]
fn fresh_tracker_request_excludes_old_polls_without_erasing_records_or_other_limits() {
    let history = ToolExecutionHistory::new(20);
    history.set_loop_detection_limits(8, 2);
    let args = json!({"action":"list"});
    for _ in 0..2 {
        history.add_record(loop_test_record(tools::TASK_TRACKER, args.clone()));
    }
    assert!(history.detect_loop(tools::TASK_TRACKER, &args).detected);
    history.begin_tracker_request();
    assert_eq!(history.get_recent_records(20).len(), 2);
    assert_eq!(history.detect_loop(tools::TASK_TRACKER, &args).repeat_count, 0);
    for _ in 0..2 {
        history.add_record(loop_test_record(tools::TASK_TRACKER, args.clone()));
    }
    assert!(history.detect_loop(tools::TASK_TRACKER, &args).detected);
}

#[test]
fn code_search_loop_identity_normalises_query_filters_and_limit() {
    let history = ToolExecutionHistory::new(10);
    history.set_loop_detection_limits(5, 2);

    let args = json!({
        "query": "exec_only_policy",
        "path": "crates/codegen/vtcode-core/src/core/agent/runner/tests.rs",
        "file_types": ["rust"],
        "result_types": ["definition", "usage"],
        "max_results": 5
    });

    // With MIN_READONLY_IDENTICAL_LIMIT=2, two successful calls with the
    // same limit-insensitive loop identity are enough to trigger detection.
    for _ in 0..2 {
        history.add_record(ToolExecutionRecord::success(
            tools::CODE_SEARCH.to_string(),
            tools::CODE_SEARCH.to_string(),
            false,
            None,
            args.clone(),
            json!({"query": "exec_only_policy", "filters": {}, "results": [], "returned": 0, "truncated": false, "hints": []}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
    }

    let mut equivalent_args = args.clone();
    equivalent_args["max_results"] = json!(100);
    let loop_result = history.detect_loop(tools::CODE_SEARCH, &equivalent_args);
    assert!(
        loop_result.detected,
        "two loop-equivalent calls should trigger detection despite differing max_results"
    );

    // A third loop-equivalent call increases the repeat count.
    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        args.clone(),
        json!({"query": "exec_only_policy", "filters": {}, "results": [], "returned": 0, "truncated": false, "hints": []}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let loop_result = history.detect_loop(tools::CODE_SEARCH, &args);
    assert!(loop_result.detected);
    assert_eq!(loop_result.repeat_count, 3);
    assert_eq!(loop_result.tool_name, tools::CODE_SEARCH);

    for changed in ["query", "path", "file_types", "result_types"] {
        let mut changed_args = args.clone();
        changed_args[changed] = match changed {
            "query" => json!("different"),
            "path" => json!("vtcode-core/tests"),
            "file_types" => json!(["python"]),
            "result_types" => json!(["text"]),
            _ => unreachable!(),
        };
        assert!(!history.detect_loop(tools::CODE_SEARCH, &changed_args).detected);
    }
}

#[test]
fn code_search_replay_matches_omitted_path_and_default_limit() {
    let history = ToolExecutionHistory::new(10);
    let cached_args = json!({"query": "ToolRegistry"});
    let cached_result = json!({"results": ["cached default search"]});

    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        cached_args,
        cached_result.clone(),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let replayed = history.find_recent_successful_by_read_target(
        tools::CODE_SEARCH,
        &json!({"query": "ToolRegistry", "max_results": 20}),
        Duration::from_secs(60),
    );

    assert_eq!(replayed, Some(cached_result));
}

#[test]
fn code_search_replay_separates_different_effective_limits() {
    let history = ToolExecutionHistory::new(10);

    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        json!({"query": "ToolRegistry", "max_results": 1}),
        json!({"results": ["limited search"]}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let replayed = history.find_recent_successful_by_read_target(
        tools::CODE_SEARCH,
        &json!({"query": "ToolRegistry", "max_results": 100}),
        Duration::from_secs(60),
    );

    assert!(replayed.is_none());
}

#[test]
fn code_search_replay_stops_after_in_scope_mutation_but_survives_unrelated_edit() {
    let search_args = json!({"query": "Widget", "path": "src"});
    let cached_result = json!({"results": ["cached Widget"]});
    let history_with_mutation = |mutation_path: &str| {
        let history = ToolExecutionHistory::new(10);
        history.add_record(ToolExecutionRecord::success(
            tools::CODE_SEARCH.to_string(),
            tools::CODE_SEARCH.to_string(),
            false,
            None,
            search_args.clone(),
            cached_result.clone(),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
        history.add_record(ToolExecutionRecord::success(
            tools::APPLY_PATCH.to_string(),
            tools::APPLY_PATCH.to_string(),
            false,
            None,
            json!({"input": format!(
                "*** Begin Patch\n*** Update File: {mutation_path}\n@@\n-Widget\n+Gadget\n*** End Patch\n"
            )}),
            json!({"success": true}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
        history
    };

    let in_scope = history_with_mutation("src/widget.rs");
    assert!(
        in_scope
            .find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),)
            .is_none(),
        "searching src, then editing src/widget.rs, must execute fresh"
    );

    let unrelated = history_with_mutation("tests/widget.rs");
    assert_eq!(
        unrelated.find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),),
        Some(cached_result),
        "an unrelated edit may reuse the prior scoped search"
    );
}

#[test]
fn code_search_replay_stops_after_successful_pathless_command_mutation() {
    let history = ToolExecutionHistory::new(10);
    let search_args = json!({"query": "Widget", "path": "src"});
    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        search_args.clone(),
        json!({"results": ["cached Widget"]}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"cmd": "sed -i 's/Widget/Gadget/' src/widget.rs"}),
        json!({"exit_code": 0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    assert!(
        history
            .find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),)
            .is_none(),
        "a successful command mutation without explicit target metadata must invalidate search replay"
    );
}

#[test]
fn code_search_replay_stops_after_move_into_searched_scope() {
    let history = ToolExecutionHistory::new(10);
    let search_args = json!({"query": "Widget", "path": "src"});
    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        search_args.clone(),
        json!({"results": []}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        tools::MOVE_FILE.to_string(),
        tools::MOVE_FILE.to_string(),
        false,
        None,
        json!({"path": "staging/widget.rs", "destination": "src/widget.rs"}),
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    assert!(
        history
            .find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),)
            .is_none(),
        "moving a file into the searched scope must invalidate search replay"
    );
}

#[test]
fn code_search_replay_recovers_both_paths_from_base64_public_move_patch() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    let history = ToolExecutionHistory::new(10);
    let old_search = json!({"query": "Widget", "path": "src/old.rs"});
    let new_search = json!({"query": "Widget", "path": "src/new.rs"});
    for args in [&old_search, &new_search] {
        history.add_record(ToolExecutionRecord::success(
            tools::CODE_SEARCH.to_string(),
            tools::CODE_SEARCH.to_string(),
            false,
            None,
            args.clone(),
            json!({"results": ["cached"]}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
    }
    let patch =
        "*** Begin Patch\n*** Update File: src/old.rs\n*** Move to: src/new.rs\n@@\n-Widget\n+Gadget\n*** End Patch\n";
    history.add_record(ToolExecutionRecord::success(
        tools::APPLY_PATCH.to_string(),
        tools::APPLY_PATCH.to_string(),
        false,
        None,
        json!({"patch": format!("base64:{}", BASE64.encode(patch))}),
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    for args in [&old_search, &new_search] {
        assert!(
            history
                .find_recent_successful_by_read_target(tools::CODE_SEARCH, args, Duration::from_secs(60),)
                .is_none(),
            "both old and new move paths must invalidate replay: {args}"
        );
    }
}

#[test]
fn find_recent_successful_by_read_target_matches_same_path_different_offset() {
    let history = ToolExecutionHistory::new(10);

    // Record 1: read src/lib.rs with offset=0
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100}),
        json!({"content":"file content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Record 2: read src/main.rs (different file)
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
        json!({"content":"main content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Query: same path, different offset — should NOT match (issue #680:
    // a different offset means the model is asking for a different slice
    // of the file, so it needs fresh content, not a cached stub).
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":500,"limit":200}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "different offset should not match same path");

    // Query: different path, same pagination — should match record 2
    let result2 = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
        Duration::from_secs(600),
    );
    assert!(result2.is_some());
    assert_eq!(result2.unwrap(), json!({"content":"main content"}));

    // Query: non-existent path — should return None
    let result3 = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/missing.rs"}),
        Duration::from_secs(600),
    );
    assert!(result3.is_none());

    // Query: write action — should return None (not read-only)
    let result4 = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"new"}),
        Duration::from_secs(600),
    );
    assert!(result4.is_none(), "write action should not match read records");
}

#[test]
fn find_recent_successful_by_read_target_extent_matters() {
    let history = ToolExecutionHistory::new(10);

    // Record: read AGENTS.md, offset=0, limit=200
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        json!({"output":"full file content line 1\nline2\n..."}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Query: same path, same offset, larger limit → should NOT match
    // (issue #680: the model asked for more lines than the cache has)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":220}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "larger limit should not match same path");

    // Query: same path, same offset, same limit → should match (genuine repeat)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        Duration::from_secs(600),
    );
    assert!(result.is_some(), "same path and same limit should match");

    // Query: same path, same offset, smaller limit → should match (subset)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":100}),
        Duration::from_secs(600),
    );
    assert!(result.is_some(), "smaller limit is a subset of cached extent");
}

#[test]
fn find_recent_successful_by_read_target_no_limit_uses_default() {
    let history = ToolExecutionHistory::new(10);

    // Record: read AGENTS.md with no explicit limit or offset (defaults)
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md"}),
        json!({"output":"default content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Query: same path, also no explicit limit/offset → should match (both use defaults)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md"}),
        Duration::from_secs(600),
    );
    assert!(result.is_some(), "both using default offset/limit should match");

    // Query: same path, default offset but explicit limit → should NOT match
    // (one has explicit pagination, other doesn't — can't compare)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","limit":200}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "mixed default/explicit limit should not match");
}

#[test]
fn find_recent_successful_by_read_target_raw_shape_matters() {
    let history = ToolExecutionHistory::new(10);

    // Record: non-raw read can be summarized for the model, so it must not
    // satisfy a later raw=true query that asks for exact content.
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        json!({"summary":"summarized guidance","summarized_for_model":true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "non-raw summarized read should not satisfy raw=true query");

    // Record: raw=true read can satisfy the same raw=true shape.
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        json!({"output":"exact file content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        Duration::from_secs(600),
    );
    assert_eq!(result, Some(json!({"output":"exact file content"})));
}

#[test]
fn find_recent_successful_by_read_target_validates_aliases_pagination_and_encoding() {
    let history = ToolExecutionHistory::new(10);
    let cached_args = json!({
        "action": "read",
        "path": "src/lib.rs",
        "offset_lines": 1,
        "page_size_lines": 100,
        "page": 2,
        "per_page": 50,
        "encoding": "utf8"
    });
    let cached_result = json!({"content": "cached"});
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        cached_args,
        cached_result.clone(),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    assert_eq!(
        history.find_recent_successful_by_read_target(
            tools::UNIFIED_FILE,
            &json!({
                "action": "read",
                "path": "src/lib.rs",
                "offset_lines": 1,
                "page_size_lines": 50,
                "page": 2,
                "per_page": 50,
                "encoding": "utf8"
            }),
            Duration::from_secs(600),
        ),
        Some(cached_result)
    );
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::UNIFIED_FILE,
                &json!({
                    "action": "read",
                    "path": "src/lib.rs",
                    "offset_lines": 1,
                    "page_size_lines": 50,
                    "page": 3,
                    "per_page": 50,
                    "encoding": "utf8"
                }),
                Duration::from_secs(600),
            )
            .is_none()
    );
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::UNIFIED_FILE,
                &json!({
                    "action": "read",
                    "path": "src/lib.rs",
                    "offset_lines": 1,
                    "page_size_lines": 50,
                    "page": 2,
                    "per_page": 50,
                    "encoding": "base64"
                }),
                Duration::from_secs(600),
            )
            .is_none()
    );
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::UNIFIED_FILE,
                &json!({
                    "action": "read",
                    "path": "src/lib.rs",
                    "offset_lines": "invalid",
                    "page_size_lines": 50,
                    "page": 2,
                    "per_page": 50,
                    "encoding": "utf8"
                }),
                Duration::from_secs(600),
            )
            .is_none()
    );
}

#[test]
fn exec_slice_read_reuses_identical_slice_cross_turn() {
    let history = ToolExecutionHistory::new(10);

    // Session evidence (20261015T232419): `sed -n '270,300p' README.md` was
    // re-emitted 148s later and re-executed because no reuse layer accepted
    // exec tools. The recorded args carry alias materialization, the query
    // re-emits the raw provider spelling — identity must bridge both.
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '270,300p' README.md","command":"sed -n '270,300p' README.md"}),
        json!({"output":"line 270\nline 271\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let query = json!({"cmd":"sed -n '270,300p' README.md"});
    let served = history.find_recent_successful_by_read_target(tools::EXEC_COMMAND, &query, Duration::from_secs(600));
    assert!(served.is_some(), "identical slice must reuse within TTL");
}

#[test]
fn exec_slice_read_rejects_other_slices_and_transforms() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '270,300p' README.md","command":"sed -n '270,300p' README.md"}),
        json!({"output":"line 270\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Different slice: no coverage proof exists for command reads.
    let other_slice = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '280,300p' README.md"}),
        Duration::from_secs(600),
    );
    assert!(other_slice.is_none(), "different slice must not be served");

    // Piped transform differs in transform class, and must never be served
    // from a verbatim record.
    let piped = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '270,300p' README.md | awk '{print length}'"}),
        Duration::from_secs(600),
    );
    assert!(piped.is_none(), "verbatim record cannot serve a piped query");

    // Mutating command: fail closed regardless of shape.
    let mutating = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -i '270,300s/a/b/' README.md"}),
        Duration::from_secs(600),
    );
    assert!(mutating.is_none(), "mutating commands are never eligible");

    // Non-command shapes (`cat extra1 extra2`) never parse as slice reads.
    let multi = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"cat README.md"}),
        Duration::from_secs(600),
    );
    assert!(multi.is_none());
}

#[test]
fn exec_slice_read_rejects_expired_ttl_and_fresh_after_mutation() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '1,5p' README.md","command":"sed -n '1,5p' README.md"}),
        json!({"output":"top\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // TTL expired: no reuse.
    let expired = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '1,5p' README.md"}),
        Duration::from_secs(0),
    );
    assert!(expired.is_none(), "TTL still bounds exec-slice reuse");

    // A later success record for a *different* slice stays invisible to the
    // original query but does not poison it either.
    history.add_record(ToolExecutionRecord::success(
        tools::WRITE_FILE.to_string(),
        tools::WRITE_FILE.to_string(),
        false,
        None,
        json!({"path":"README.md","content":"mutated"}),
        json!({"success":true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    let after_mutation = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '1,5p' README.md"}),
        Duration::from_secs(600),
    );
    // NOTE: replay scanning skips records whose tool name differs, so the
    // write does not invalidate here — invalidation is the caller's job
    // (`invalidate_for_path`). This documents that expectation explicitly.
    assert!(after_mutation.is_some(), "mutating records of other tools are skipped, not scanned");
}

#[test]
fn mutation_invalidates_cached_exec_slice_read_of_same_path() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '1,5p' README.md","command":"sed -n '1,5p' README.md"}),
        json!({"output":"stale\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // A write to the same path must drop the exec-slice record; otherwise the
    // cross-turn cache would serve stale pre-mutation content.
    history.invalidate_for_path("README.md");
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::EXEC_COMMAND,
                &json!({"cmd":"sed -n '1,5p' README.md"}),
                Duration::from_secs(600),
            )
            .is_none(),
        "a mutation of the read path must invalidate the cached exec slice"
    );
}

#[test]
fn invalidate_all_reads_drops_exec_slice_records() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '1,5p' README.md","command":"sed -n '1,5p' README.md"}),
        json!({"output":"stale\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    // A non-slice command must survive (not a read record).
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"cargo build","command":"cargo build"}),
        json!({"output":"built\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    history.invalidate_all_reads();

    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::EXEC_COMMAND,
                &json!({"cmd":"sed -n '1,5p' README.md"}),
                Duration::from_secs(600),
            )
            .is_none(),
        "pathless mutation must drop cached exec slices too"
    );
}
