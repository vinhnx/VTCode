use super::*;

#[test]
fn test_immediate_repetition_detection() {
    let mut detector = LoopDetector::with_max_repeated_calls(3);
    let args = json!({"path": "src/"});

    // First two calls - no warning
    assert!(detector.record_call(LEGACY_GREP_FILE, &args).is_none());
    assert!(detector.record_call(LEGACY_GREP_FILE, &args).is_none());

    // Third identical call - hard stop
    let warning = detector.record_call(LEGACY_GREP_FILE, &args);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains(HARD_STOP_PREFIX));
}
#[test]
fn test_command_tools_skip_identical_hard_stop() {
    let mut detector = LoopDetector::new();
    let args = json!({"command": "cargo test"});

    assert!(detector.record_call(tools::RUN_PTY_CMD, &args).is_none());
    assert!(detector.record_call(tools::RUN_PTY_CMD, &args).is_none());
    assert!(detector.record_call(tools::RUN_PTY_CMD, &args).is_none());
}
#[test]
fn test_exec_command_alias_skips_identical_hard_stop() {
    let mut detector = LoopDetector::new();
    let args = json!({"cmd": "cargo test"});

    assert!(detector.record_call(tools::EXEC_COMMAND, &args).is_none());
    assert!(detector.record_call(tools::EXEC_COMMAND, &args).is_none());
    assert!(detector.record_call(tools::EXEC_COMMAND, &args).is_none());
}
#[test]
fn test_root_path_normalization() {
    let mut detector = LoopDetector::with_max_repeated_calls(3);

    // All these should be treated as identical
    let paths = [
        json!({"path": "."}),
        json!({"path": ""}),
        json!({"path": "././"}),
        json!({"path": "//"}),
        json!({}),
    ];

    for path in &paths[..2] {
        assert!(detector.record_call(LEGACY_LIST_FILES, path).is_none());
    }

    // Third call with any root variation should trigger
    let warning = detector.record_call(LEGACY_LIST_FILES, &paths[2]);
    assert!(warning.is_some());

    // Further root-only variations should continue to warn
    for path in &paths[3..] {
        assert!(detector.record_call(LEGACY_LIST_FILES, path).is_some());
    }
}
#[test]
fn test_detects_repeated_calls() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let tool_name = "test_repeated_tool";
    detector.set_tool_limit(tool_name, MAX_READONLY_TOOL_CALLS);
    let args = json!({"path": "/src"});

    // Repetition heuristics (pattern detection and soft limits) should warn eventually.
    let mut saw_warning = false;
    for _ in 0..MAX_READONLY_TOOL_CALLS {
        if detector.record_call(tool_name, &args).is_some() {
            saw_warning = true;
        }
    }
    assert!(saw_warning);
    assert_eq!(detector.get_call_count(tool_name), MAX_READONLY_TOOL_CALLS);
}
#[test]
fn test_hard_limit_enforcement() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let tool_name = "test_hard_limit_tool";
    detector.set_tool_limit(tool_name, 2);
    let args = json!({"pattern": "test"});

    // Hard limit is 2x configured soft limit.
    let hard_limit = 2 * HARD_LIMIT_MULTIPLIER;
    let mut saw_warning = false;
    for i in 0..hard_limit {
        let result = detector.record_call(tool_name, &args);
        if result.is_some() {
            saw_warning = true;
        }
        if i >= hard_limit - 1 {
            assert!(result.is_some());
        }
    }

    assert!(saw_warning);
    assert!(detector.is_hard_limit_exceeded(tool_name));
}
#[test]
fn test_different_tools_no_warning() {
    let mut detector = LoopDetector::new();

    detector.record_call(LEGACY_LIST_FILES, &json!({"path": "/src"}));
    detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "test"}));
    detector.record_call(tools::READ_FILE, &json!({"path": "main.rs"}));

    assert_eq!(detector.tool_counts.len(), 3);
}
#[test]
fn test_non_root_paths_distinct() {
    let mut detector = LoopDetector::new();

    // These should be treated as different calls
    detector.record_call(LEGACY_LIST_FILES, &json!({"path": "src"}));
    detector.record_call(LEGACY_LIST_FILES, &json!({"path": "docs"}));
    detector.record_call(LEGACY_LIST_FILES, &json!({"path": "tests"}));

    // Count for each should be 1
    assert_eq!(detector.tool_counts.get(LEGACY_LIST_FILES).copied().unwrap_or(0), 3);
}
#[test]
fn test_identical_calls_trigger_hard_limit() {
    let mut detector = LoopDetector::with_max_repeated_calls(3);
    let args = json!({"path": "."});

    assert!(detector.record_call(tools::READ_FILE, &args).is_none());
    assert!(detector.record_call(tools::READ_FILE, &args).is_none());

    let warning = detector.record_call(tools::READ_FILE, &args);
    assert!(warning.is_some());
    assert!(detector.is_hard_limit_exceeded(tools::READ_FILE));
}
#[test]
fn test_normalize_args_removes_pagination() {
    let args = json!({"path": "src", "page": 1, "per_page": 10});
    let normalized = normalize_args_for_detection(LEGACY_LIST_FILES, &args);

    assert!(normalized.get("page").is_none());
    assert!(normalized.get("per_page").is_none());
    assert_eq!(normalized.get("path").and_then(|v| v.as_str()), Some("src"));
}
#[test]
fn test_reset_tool_clears_specific_tool() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let args = json!({"path": "src"});

    // Record calls for multiple tools
    detector.record_call(LEGACY_LIST_FILES, &args);
    detector.record_call(LEGACY_LIST_FILES, &args);
    detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "test"}));

    assert_eq!(detector.get_call_count(LEGACY_LIST_FILES), 2);
    assert_eq!(detector.get_call_count(LEGACY_GREP_FILE), 1);

    // Reset only list_files
    detector.reset_tool(LEGACY_LIST_FILES);

    assert_eq!(detector.get_call_count(LEGACY_LIST_FILES), 0);
    assert_eq!(detector.get_call_count(LEGACY_GREP_FILE), 1);
}
#[test]
fn test_suggest_alternative_for_list_files() {
    let detector = LoopDetector::new();
    let suggestion = detector.suggest_alternative(LEGACY_LIST_FILES);

    assert!(suggestion.is_some());
    let msg = suggestion.unwrap();
    assert!(msg.contains("code_search"));
    assert!(msg.contains("file_types"));
    assert!(msg.contains("subdirectories"));
}
#[test]
fn test_suggest_alternative_for_grep_file() {
    let detector = LoopDetector::new();
    let suggestion = detector.suggest_alternative(LEGACY_GREP_FILE);

    assert!(suggestion.is_some());
    let msg = suggestion.unwrap();
    assert!(msg.contains("exec_command.cmd"));
    assert!(msg.contains("definition or usage"));
    assert!(msg.contains("pattern"));
}
#[test]
fn code_search_loop_fingerprint_uses_only_normalised_search_identity() {
    let first = json!({
        "query": " Widget ",
        "path": " src ",
        "file_types": [".rs", "python", "rust"],
        "result_types": ["path", "definition", "path"],
        "max_results": 5
    });
    let second = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust", ".py"],
        "result_types": ["definition", "path"],
        "max_results": 100
    });

    assert_eq!(hash_normalized_args(tools::CODE_SEARCH, &first), hash_normalized_args(tools::CODE_SEARCH, &second));

    for changed in [
        json!({"query": "Other", "path": "src", "file_types": ["rust", "python"], "result_types": ["definition", "path"]}),
        json!({"query": "Widget", "path": "tests", "file_types": ["rust", "python"], "result_types": ["definition", "path"]}),
        json!({"query": "Widget", "path": "src", "file_types": ["rust"], "result_types": ["definition", "path"]}),
        json!({"query": "Widget", "path": "src", "file_types": ["rust", "python"], "result_types": ["usage", "path"]}),
    ] {
        assert_ne!(
            hash_normalized_args(tools::CODE_SEARCH, &first),
            hash_normalized_args(tools::CODE_SEARCH, &changed)
        );
    }
}
#[test]
fn test_suggest_alternative_for_unknown_tool() {
    let detector = LoopDetector::new();
    let suggestion = detector.suggest_alternative("unknown_tool");

    assert!(suggestion.is_some());
    let msg = suggestion.unwrap();
    assert!(msg.contains("root cause analysis"));
}
#[test]
fn test_faster_detection_with_lower_limit() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    detector.set_tool_limit(LEGACY_LIST_FILES, 3);
    let args = json!({"path": "src"});

    // First call - no warning
    assert!(detector.record_call(LEGACY_LIST_FILES, &args).is_none());

    // Second call - no warning
    assert!(detector.record_call(LEGACY_LIST_FILES, &args).is_none());

    // Third call - should trigger warning (soft limit = 3)
    let warning = detector.record_call(LEGACY_LIST_FILES, &args);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("Loop detected"));
}
#[test]
fn test_file_operation_action_suffix_uses_action_specific_limit() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let tool_key = format!("{}::read", tools::UNIFIED_FILE);

    for idx in 0..(MAX_WRITE_TOOL_CALLS * HARD_LIMIT_MULTIPLIER) {
        let args = json!({"path": "src/main.rs", "offset_lines": idx + 1, "limit": 1});
        let _ = detector.record_call(&tool_key, &args);
    }

    // Read action should not use write limits.
    assert!(!detector.is_hard_limit_exceeded(&tool_key));
}
#[test]
fn test_file_operation_write_suffix_uses_write_limit() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let tool_key = format!("{}::write", tools::UNIFIED_FILE);

    for idx in 0..(MAX_WRITE_TOOL_CALLS * HARD_LIMIT_MULTIPLIER) {
        let args = json!({"path": format!("src/file_{idx}.rs"), "content": "x"});
        let _ = detector.record_call(&tool_key, &args);
    }

    assert!(detector.is_hard_limit_exceeded(&tool_key));
}
#[test]
fn test_command_session_action_suffix_skips_identical_limit() {
    let mut detector = LoopDetector::with_max_repeated_calls(3);
    let tool_key = format!("{}::run", tools::UNIFIED_EXEC);
    let args = json!({"command": "cargo check"});

    assert!(detector.record_call(&tool_key, &args).is_none());
    assert!(detector.record_call(&tool_key, &args).is_none());
    assert!(detector.record_call(&tool_key, &args).is_none());
}
