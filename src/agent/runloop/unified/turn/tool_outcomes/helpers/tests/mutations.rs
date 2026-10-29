use super::*;

#[test]
fn dynamic_readonly_inspections_do_not_arm_verification_gate() {
    let mut tracker = LoopTracker::new();
    // Session-vtcode-20261015T142625Z: dynamic `for` loops over grep results
    // with `test -f` exited 0 with no writes but were counted as blind edits,
    // bumping completion generation and looping harness auto-verification.
    for command in [
        "for p in $(grep -o '](\\./[^)#]*' README.md | sed 's/](//' | sort -u); do test -f \"$p\" || echo \"missing: $p\"; done",
        "rg -n 'vinhnx/(VTCode|vtcode)' README.md",
        "grep -n '^## ' README.md",
        "git status --short",
        "rg -n \"MD013|line_length\" docs/development/*.md | head",
    ] {
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"exit_code": 0}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &outcome, "exec_command", &json!({"cmd": command}));
    }
    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.verification_is_pending());
}
#[test]
fn execution_tool_resets_mutation_counter() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    // Two mutations
    update_repetition_tracker(&mut tracker, &success, "edit_file", &json!({"path":"a","old_str":"x","new_str":"y"}));
    update_repetition_tracker(&mut tracker, &success, "edit_file", &json!({"path":"b","old_str":"x","new_str":"y"}));
    assert_eq!(tracker.consecutive_mutations, 2);

    // Execution tool resets
    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::UNIFIED_EXEC,
        &json!({"action":"run","command":"cargo check"}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}
#[test]
fn reads_increment_navigation_counter() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(&mut tracker, &success, tools::READ_FILE, &json!({"path":"src/main.rs"}));
    assert_eq!(tracker.consecutive_navigations, 1);
    assert_eq!(tracker.consecutive_mutations, 0);

    update_repetition_tracker(&mut tracker, &success, tools::GREP_FILE, &json!({"pattern":"foo","path":"src/"}));
    assert_eq!(tracker.consecutive_navigations, 2);
}
#[test]
fn mutation_resets_navigation_counter() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    // Several reads
    for _ in 0..5 {
        update_repetition_tracker(&mut tracker, &success, tools::READ_FILE, &json!({"path":"src/main.rs"}));
    }
    assert_eq!(tracker.consecutive_navigations, 5);

    // A mutation resets navigation counter
    update_repetition_tracker(
        &mut tracker,
        &success,
        "edit_file",
        &json!({"path":"src/lib.rs","old_str":"a","new_str":"b"}),
    );
    assert_eq!(tracker.consecutive_navigations, 0);
    assert_eq!(tracker.consecutive_mutations, 1);
}
#[test]
fn task_tracker_does_not_increment_mutations_in_planning() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::TASK_TRACKER,
        &json!({"action":"create","items":["step"]}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}
#[test]
fn task_tracker_does_not_increment_mutations() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::TASK_TRACKER,
        &json!({"action":"create","items":["step"]}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}
#[test]
fn plan_file_write_does_not_increment_mutations() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":".vtcode/plans/my-plan.md","content":"text"}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}
#[test]
fn non_plan_file_write_still_increments_mutations() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"text"}),
    );
    assert_eq!(tracker.consecutive_mutations, 1);
    assert_eq!(tracker.consecutive_navigations, 0);
}
#[test]
fn argument_error_detection_includes_required_update_fields() {
    assert!(check_is_argument_error("Tool execution failed: 'index' is required for 'update' (1-indexed)"));
}
