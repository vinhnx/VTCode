use super::*;

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
