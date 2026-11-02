use super::*;

#[test]
fn test_repetitive_read_target_with_small_variations_triggers_hard_stop() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let tool_key = format!("{}::read", tools::UNIFIED_FILE);
    let mut saw_hard_stop = false;

    for offset in [1, 2, 1, 2, 1, 2, 1, 2] {
        let args = json!({"path": "crates/codegen/vtcode-core/src/a2a/server.rs", "offset_lines": offset, "limit": 20});
        if let Some(warning) = detector.record_call(&tool_key, &args)
            && warning.contains(HARD_STOP_PREFIX)
        {
            saw_hard_stop = true;
        }
    }

    assert!(saw_hard_stop);
    assert!(detector.is_hard_limit_exceeded(&tool_key));
}
#[test]
fn test_repetitive_read_target_with_many_ranges_is_not_hard_stopped() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let tool_key = format!("{}::read", tools::UNIFIED_FILE);

    for offset in 1..=MAX_SIMILAR_READ_TARGET_CALLS {
        let args =
            json!({"path": "crates/codegen/vtcode-core/src/a2a/server.rs", "offset_lines": offset * 40, "limit": 40});
        if let Some(warning) = detector.record_call(&tool_key, &args) {
            assert!(!warning.contains(HARD_STOP_PREFIX));
        }
    }

    assert!(!detector.is_hard_limit_exceeded(&tool_key));
}
#[test]
fn test_repetitive_read_target_grep_calls_do_not_break_streak() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let read_tool = format!("{}::read", tools::UNIFIED_FILE);

    // Interleave grep calls between read_file calls on the same target.
    // With the new logic, grep (read-only) does not break the streak.
    // Use distinct offsets so variants stay above the threshold.
    // Limit iterations so the navigation streak stays below the hard stop (7).
    for offset in 1..=(MAX_SIMILAR_READ_TARGET_CALLS - 1) {
        let _ = detector.record_call(
            &read_tool,
            &json!({"path": "crates/codegen/vtcode-core/src/a2a/server.rs", "offset_lines": offset * 40, "limit": 20}),
        );
        let _ = detector.record_call(
            LEGACY_GREP_FILE,
            &json!({"pattern": "handle_loop_detection", "path": "crates/codegen/vtcode-core/src"}),
        );
    }

    // Variants exceed the threshold, so no repetitive-read-target hard stop.
    // But navigation hard stop may have fired (streak >= 7) -- check specifically
    // for the repetitive-read-target condition instead.
    assert!(!detector.is_hard_limit_exceeded(&read_tool));
}
#[test]
fn test_repetitive_read_target_same_params_with_grep_between_triggers_hard_stop() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let read_tool = format!("{}::read", tools::UNIFIED_FILE);

    // Same offset repeated, with grep calls between reads.
    // Grep doesn't break the streak, so the hard stop fires.
    for _ in 0..MAX_SIMILAR_READ_TARGET_CALLS + 2 {
        let _ = detector.record_call(&read_tool, &json!({"path": "Cargo.lock", "offset_lines": 1, "limit": 2000}));
        let _ = detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "aws-lc", "path": "Cargo.lock"}));
    }

    assert!(detector.is_hard_limit_exceeded(&read_tool));
}
#[test]
fn test_read_file_alias_cycling_triggers_identical_detection() {
    // Simulates the exact failure from the transcript: LLM cycles through
    // offset_lines, max_lines, chunk_lines, line_start/line_end, encoding
    // for the same file. Normalization should collapse them to identical hashes.
    let mut detector = LoopDetector::with_max_repeated_calls(3);

    let call1 = json!({"path": "docs/README.md", "max_lines": 200});
    let call2 = json!({"path": "docs/README.md", "offset_lines": 1, "limit": 200});
    let call3 = json!({"path": "docs/README.md", "chunk_lines": 200});

    // After normalization, all three should have: {path: "docs/README.md", offset: 1, limit: 200}
    let n1 = normalize_args_for_detection(tools::READ_FILE, &call1);
    let n2 = normalize_args_for_detection(tools::READ_FILE, &call2);
    let n3 = normalize_args_for_detection(tools::READ_FILE, &call3);

    // Verify aliases are normalized
    assert!(n1.get("max_lines").is_none(), "max_lines should be removed");
    assert!(n2.get("offset_lines").is_none(), "offset_lines should be removed");
    assert!(n3.get("chunk_lines").is_none(), "chunk_lines should be removed");
    assert_eq!(n1.get("limit"), n2.get("limit"));
    assert_eq!(n2.get("limit"), n3.get("limit"));

    // All three should trigger identical-call detection by call 3
    assert!(detector.record_call(tools::READ_FILE, &call1).is_none());
    assert!(detector.record_call(tools::READ_FILE, &call2).is_none());

    let warning = detector.record_call(tools::READ_FILE, &call3);
    assert!(warning.is_some(), "Third aliased call should be detected");
    assert!(warning.unwrap().contains(HARD_STOP_PREFIX));
}
#[test]
fn test_read_file_encoding_and_action_are_stripped() {
    let with_encoding = json!({"path": "foo.rs", "encoding": "utf-8", "offset_lines": 1, "max_lines": 200});
    let without_encoding = json!({"path": "foo.rs", "offset_lines": 1, "max_lines": 200});

    let n1 = normalize_args_for_detection(tools::READ_FILE, &with_encoding);
    let n2 = normalize_args_for_detection(tools::READ_FILE, &without_encoding);

    assert!(n1.get("encoding").is_none());
    assert_eq!(n1, n2);
}
#[test]
fn test_line_start_line_end_normalized_to_offset_limit() {
    let args = json!({"path": "foo.rs", "line_start": 1, "line_end": 200});
    let normalized = normalize_args_for_detection(tools::READ_FILE, &args);

    assert!(normalized.get("line_start").is_none());
    assert!(normalized.get("line_end").is_none());
    assert_eq!(normalized.get("offset").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(normalized.get("limit").and_then(|v| v.as_u64()), Some(200));
}
#[test]
fn test_start_line_end_line_normalized_to_offset_limit() {
    let args = json!({"path": "Cargo.lock", "start_line": 550, "end_line": 590});
    let normalized = normalize_args_for_detection(tools::READ_FILE, &args);

    assert!(normalized.get("start_line").is_none());
    assert!(normalized.get("end_line").is_none());
    assert_eq!(normalized.get("offset").and_then(|v| v.as_u64()), Some(550));
    assert_eq!(normalized.get("limit").and_then(|v| v.as_u64()), Some(41));
}
