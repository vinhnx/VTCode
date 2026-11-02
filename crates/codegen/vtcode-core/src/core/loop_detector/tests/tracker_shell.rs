use super::*;

#[test]
fn distinct_tracker_items_and_states_do_not_exhaust_the_default_tool_limit() {
    let mut detector = LoopDetector::new();
    assert!(
        detector
            .record_call(tools::TASK_TRACKER, &serde_json::json!({"action":"list"}))
            .is_none()
    );
    for index in 1..=8 {
        for status in ["in_progress", "completed"] {
            let warning = detector.record_call(
                tools::TASK_TRACKER,
                &serde_json::json!({"action":"update","index":index,"status":status}),
            );
            assert!(warning.is_none(), "item {index} {status}: {warning:?}");
            assert!(!detector.is_hard_limit_exceeded(tools::TASK_TRACKER));
        }
    }
    assert_ne!(
        hash_normalized_args(tools::TASK_TRACKER, &serde_json::json!({"action":"list"})),
        hash_normalized_args(tools::TASK_TRACKER, &serde_json::json!({"action":"create"})),
    );
}
#[test]
fn repeated_tracker_calls_still_stop_and_explicit_limits_remain_aggregate() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    for count in 1..=6 {
        detector.record_call(tools::TASK_TRACKER, &serde_json::json!({"action":"list"}));
        assert_eq!(detector.is_hard_limit_exceeded(tools::TASK_TRACKER), count == 6);
    }
    let mut detector = LoopDetector::new();
    detector.set_tool_limit(tools::TASK_TRACKER, 2);
    for index in 1..=4 {
        detector.record_call(
            tools::TASK_TRACKER,
            &serde_json::json!({"action":"update","index":index,"status":"completed"}),
        );
        assert_eq!(detector.is_hard_limit_exceeded(tools::TASK_TRACKER), index == 4);
    }
}
#[test]
fn distinct_shell_reads_do_not_become_repetitions_of_the_tool_name() {
    let mut detector = LoopDetector::new();
    for offset in 1..=24 {
        let warning = detector.record_call(
            tools::EXEC_COMMAND,
            &serde_json::json!({
                "cmd":format!("sed -n '{offset}p' src/cli/companion.rs")
            }),
        );
        assert!(warning.is_none(), "distinct read {offset}: {warning:?}");
        assert!(!detector.is_hard_limit_exceeded(tools::EXEC_COMMAND));
    }
}
#[test]
fn shell_repeat_hard_stop_tracks_the_action_and_resets_explicitly() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    for offset in 1..=18 {
        detector.record_call(tools::EXEC_COMMAND, &serde_json::json!({"cmd":format!("sed -n '{offset}p' fixture")}));
    }
    for count in 1..16 {
        detector.record_call(tools::EXEC_COMMAND, &serde_json::json!({"cmd":"cargo check --locked"}));
        assert!(!detector.is_hard_limit_exceeded(tools::EXEC_COMMAND), "repeat {count}");
    }
    let warning = detector.record_call(tools::EXEC_COMMAND, &serde_json::json!({"cmd":"cargo check --locked"}));
    assert!(warning.unwrap().starts_with(HARD_STOP_PREFIX));
    assert!(detector.is_hard_limit_exceeded(tools::EXEC_COMMAND));
    detector.reset_tool(tools::EXEC_COMMAND);
    assert!(!detector.is_hard_limit_exceeded(tools::EXEC_COMMAND));
    assert!(
        detector
            .record_call(tools::EXEC_COMMAND, &serde_json::json!({"cmd":"cargo check --locked"}))
            .is_none()
    );
}
#[test]
fn explicit_shell_limits_still_bound_distinct_actions() {
    let mut detector = LoopDetector::new();
    detector.set_tool_limit(tools::EXEC_COMMAND, 2);
    for offset in 1..=4 {
        detector.record_call(tools::EXEC_COMMAND, &serde_json::json!({"cmd":format!("sed -n '{offset}p' fixture")}));
        assert_eq!(detector.is_hard_limit_exceeded(tools::EXEC_COMMAND), offset == 4);
    }
    detector.reset();
    assert!(!detector.is_hard_limit_exceeded(tools::EXEC_COMMAND));
}
#[test]
fn matrix_actions_are_distinct_but_repeated_status_still_hits_the_limit() {
    let mut detector = LoopDetector::with_max_repeated_calls(2);
    for action in ["create", "start", "status"] {
        detector.record_call(tools::MATRIX, &serde_json::json!({"action":action,"matrix_id":"work"}));
        assert!(!detector.is_hard_limit_exceeded(tools::MATRIX));
    }
    assert!(
        detector
            .record_call(tools::MATRIX, &serde_json::json!({"action":"status","matrix_id":"work"}))
            .is_some()
    );
    assert!(detector.is_hard_limit_exceeded(tools::MATRIX));
}
