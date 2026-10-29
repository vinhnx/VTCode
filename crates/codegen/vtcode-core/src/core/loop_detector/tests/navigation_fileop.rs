use super::*;

#[test]
fn test_navigation_loop_detection() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let list_args = serde_json::json!({"path": "src"});
    let grep_args = serde_json::json!({"pattern": "fn", "path": "src/main.rs"});
    let read_args = serde_json::json!({"path": "src/main.rs"});

    // Sequence: A, B, C (where A=LIST, B=GREP, C=READ)
    // 3 calls below the warning threshold of 4.
    let sequence = [
        (LEGACY_LIST_FILES, &list_args),
        (LEGACY_GREP_FILE, &grep_args),
        (tools::READ_FILE, &read_args),
    ];

    for (i, (tool, args)) in sequence.iter().enumerate() {
        let res = detector.record_call(tool, args);
        assert!(res.is_none(), "Call {} ({}) should not have triggered a warning", i + 1, tool);
    }

    // 4th call (any read-only) should trigger navigation loop warning (streak hits 4)
    let warning = detector.record_call(LEGACY_GREP_FILE, &grep_args);
    assert!(warning.is_some(), "4th call should have triggered a navigation loop warning");
    assert!(warning.unwrap().contains("Navigation loop detected"));

    // A mutating call should reset the streak
    let write_args = serde_json::json!({"path": "src/new.rs", "content": "test"});
    assert!(detector.record_call(tools::WRITE_FILE, &write_args).is_none());

    // Subsequent read calls should start from 0; single call should be fine
    assert!(detector.record_call(LEGACY_LIST_FILES, &list_args).is_none());
}
#[test]
fn repeated_code_search_tool_with_different_queries_is_not_an_oscillation() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let queries = ["Companion", "smoke test", "evaluator", "VT Code"];

    for query in queries {
        let warning = detector.record_call(tools::CODE_SEARCH, &json!({"query": query}));
        assert!(
            !warning
                .as_deref()
                .is_some_and(|message| message.contains("Oscillating tool pattern detected")),
            "one search tool cannot form a tool-name oscillation: {warning:?}"
        );
    }
    assert!(detector.detect_patterns().is_none());
}
#[test]
fn alternating_distinct_read_tools_still_detect_an_oscillation() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let sequence = [
        (LEGACY_GREP_FILE, json!({"pattern": "Companion", "path": "docs/"})),
        (tools::READ_FILE, json!({"path": "docs/development/vtcode-companion.md"})),
        (LEGACY_GREP_FILE, json!({"pattern": "smoke", "path": "docs/"})),
        (tools::READ_FILE, json!({"path": "docs/development/vtcode-companion-submission.md"})),
    ];

    for (tool, args) in &sequence {
        let _ = detector.record_call(tool, args);
    }
    let warning = detector.detect_patterns();
    assert!(
        warning
            .as_deref()
            .is_some_and(|message| message.contains("Oscillating tool pattern detected")),
        "alternating distinct tools with changing arguments remains detectable: {warning:?}"
    );
}
#[test]
fn write_edit_repeat_on_same_path_is_not_identical_loop() {
    // Regression: two consecutive write_file calls to the same path with
    // DIFFERENT content must not trigger the identical-call HARD STOP.
    // The normalized hash nulls `content`, so without the write-tool
    // exemption every edit would hash identically and halt the task.
    let mut detector = LoopDetector::with_max_repeated_calls(2);
    let first = json!({"path": "src/main.rs", "content": "first draft"});
    let second = json!({"path": "src/main.rs", "content": "corrected"});

    assert!(detector.record_call(tools::WRITE_FILE, &first).is_none());
    let warning = detector.record_call(tools::WRITE_FILE, &second);
    assert!(warning.is_none(), "distinct edits to same path must not loop-stop: {warning:?}");
    assert!(!detector.is_hard_limit_exceeded(tools::WRITE_FILE));
}
#[test]
fn apply_patch_repeat_on_same_file_is_not_identical_loop() {
    let mut detector = LoopDetector::with_max_repeated_calls(2);
    let first = json!({"path": "src/main.rs", "diff": "old A", "mode": "edit"});
    let second = json!({"path": "src/main.rs", "diff": "old B", "mode": "edit"});

    assert!(detector.record_call(tools::APPLY_PATCH, &first).is_none());
    assert!(detector.record_call(tools::APPLY_PATCH, &second).is_none());
    assert!(!detector.is_hard_limit_exceeded(tools::APPLY_PATCH));
}
#[test]
fn exec_command_verification_read_uses_cmd_key() {
    // Regression: exec_command's schema uses the `cmd` key (not `command`).
    // A repeated `cat file` via exec_command must be detected as an
    // identical read command; a repeated `cargo check` must be exempt.
    let mut detector = LoopDetector::with_max_repeated_calls(2);
    let cat = json!({"cmd": "cat src/main.rs"});

    assert!(detector.record_call(tools::EXEC_COMMAND, &cat).is_none());
    let warning = detector.record_call(tools::EXEC_COMMAND, &cat);
    assert!(warning.is_some(), "repeated `cat file` via exec_command.cmd should hit the identical limit");

    let mut detector = LoopDetector::with_max_repeated_calls(2);
    let build = json!({"cmd": "cargo check"});
    assert!(detector.record_call(tools::EXEC_COMMAND, &build).is_none());
    assert!(
        detector.record_call(tools::EXEC_COMMAND, &build).is_none(),
        "repeated `cargo check` must be exempt from identical-call limits"
    );
}
#[test]
fn exec_command_without_read_target_breaks_navigation_streak() {
    // Regression: `exec_command` (production shell tool) must reset the
    // readonly streak like other mutating tools, otherwise read -> build ->
    // read -> build hits the navigation HARD STOP.
    let mut detector = LoopDetector::with_max_repeated_calls(100);

    // 4 consecutive reads of DIFFERENT files => navigation warning fires at 4
    for i in 0..4 {
        let read_args = json!({"path": format!("src/file_{i}.rs")});
        let _ = detector.record_call(tools::READ_FILE, &read_args);
    }
    // A `cargo check` (no read target) breaks the streak
    assert!(
        detector
            .record_call(tools::EXEC_COMMAND, &json!({"cmd": "cargo check"}))
            .is_none()
    );
    // Reads after the break must not immediately hard-stop (streak restarted at 0)
    let read_args = json!({"path": "src/file_4.rs"});
    let warning = detector.record_call(tools::READ_FILE, &read_args);
    assert!(warning.is_none(), "read after mutating exec_command must not hit navigation stop: {warning:?}");
}
#[test]
fn reset_clears_window_and_cache_order_state() {
    let mut detector = LoopDetector::with_max_repeated_calls(2);
    let read = json!({"path": "src/main.rs"});
    for _ in 0..4 {
        let _ = detector.record_call(tools::READ_FILE, &read);
    }
    detector.reset();
    assert_eq!(detector.norm_cache_order.len(), 0);
    assert_eq!(detector.action_hash_window.len(), 0);
    assert_eq!(detector.tool_counts.len(), 0);
}
#[test]
fn test_checkpoint_324_pattern_detected() {
    // Simulates the exact failure from turn_324 checkpoint:
    // Agent reads Cargo.lock with start_line/end_line (ignored), retries with
    // different values, interleaves grep calls. With lowered navigation
    // thresholds (streak >= 4 warns, >= 7 hard-stops), the navigation loop
    // warning fires at call 4 and the repetitive-read-target hard stop
    // fires at call 6.
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let read_tool = format!("{}::read", tools::UNIFIED_FILE);

    // Call 1: read Cargo.toml (different target)
    let r = detector.record_call(&read_tool, &json!({"path": "Cargo.toml"}));
    assert!(r.is_none());

    // Call 2: read Cargo.lock
    let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock"}));
    assert!(r.is_none());

    // Call 3: read Cargo.lock (streak=2)
    let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock"}));
    assert!(r.is_none());

    // Call 4: grep Cargo.lock (read-only, does NOT break streak, streak=4)
    // Navigation loop warning fires at streak 4 with the lowered threshold.
    let r = detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "aws-lc", "path": "Cargo.lock"}));
    assert!(r.is_some(), "Navigation loop warning should fire at streak 4");
    let msg = r.unwrap();
    assert!(msg.contains("Navigation loop detected"));

    // Call 5: read Cargo.lock with start_line (streak=5)
    // Cooldown suppresses the navigation warning; no repetitive-read warning yet.
    let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock", "start_line": 550, "end_line": 590}));
    assert!(r.is_none());

    // Call 6: read Cargo.lock with different start_line (streak=6, variants=3)
    // Repetitive-read-target HARD STOP fires: same_target_streak >= 4 && variants <= 3
    let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock", "start_line": 4400, "end_line": 4420}));
    assert!(r.is_some(), "HARD STOP should fire at call 6");
    let msg = r.unwrap();
    assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop, got: {msg}");
    assert!(msg.contains("Cargo.lock"));
    assert!(msg.contains("exact continuation range"));
    assert!(detector.is_hard_limit_exceeded(&read_tool));
}
// --- file_operation read-action tests ---

#[test]
fn read_target_extracted_for_file_operation_read_action() {
    let args = json!({"action": "read", "path": "src/lib.rs"});
    let target = read_target_for_tool_call(tools::UNIFIED_FILE, &args);
    assert_eq!(target.as_deref(), Some("src/lib.rs"));
}
#[test]
fn read_target_none_for_file_operation_write_action() {
    let args = json!({"action": "write", "path": "src/lib.rs", "content": "fn main() {}"});
    let target = read_target_for_tool_call(tools::UNIFIED_FILE, &args);
    assert!(target.is_none());
}
#[test]
fn read_target_extracted_for_file_operation_with_read_suffix() {
    let args = json!({"path": "src/main.rs"});
    let read_tool = format!("{}::read", tools::UNIFIED_FILE);
    let target = read_target_for_tool_call(&read_tool, &args);
    assert_eq!(target.as_deref(), Some("src/main.rs"));
}
#[test]
fn repetitive_read_target_fires_for_file_operation_read_action() {
    let mut detector = LoopDetector::new();

    // Use only 2 different offsets so variants stays <= MAX_SIMILAR_READ_TARGET_VARIANTS.
    // With 4 calls to the same file, same_target_streak >= MAX_SIMILAR_READ_TARGET_CALLS
    // and variants <= MAX_SIMILAR_READ_TARGET_VARIANTS => HARD STOP fires.
    let offsets = [0, 100, 0, 100];
    for (i, offset) in offsets.iter().enumerate() {
        let result = detector
            .record_call(tools::UNIFIED_FILE, &json!({"action": "read", "path": "src/lib.rs", "offset": offset}));
        if i < offsets.len() - 1 {
            assert!(result.is_none(), "call {i} should not trigger");
        } else {
            assert!(result.is_some(), "call {i} should trigger HARD STOP");
            let msg = result.unwrap();
            assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop: {msg}");
            assert!(msg.contains("src/lib.rs"));
        }
    }
}
