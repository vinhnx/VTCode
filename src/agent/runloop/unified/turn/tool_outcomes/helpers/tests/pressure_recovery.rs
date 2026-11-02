use super::*;

#[test]
fn repetition_tracker_counts_failures() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            "edit_file".to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "boom".to_string(),
        ),
    });

    update_repetition_tracker(&mut tracker, &outcome, "edit_file", &json!({"path":"src/main.rs"}));

    assert_eq!(tracker.max_count_filtered(|_| false), 1);
}
#[test]
fn failed_file_mutations_do_not_trigger_verification_pressure() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            "apply_patch".to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "invalid patch path".to_string(),
        ),
    });

    update_repetition_tracker(
        &mut tracker,
        &outcome,
        tools::APPLY_PATCH,
        &json!({"input":"*** Begin Patch\n*** Update File: /absolute/path\n*** End Patch"}),
    );

    assert_eq!(tracker.consecutive_mutations, 0);
}
#[test]
fn no_op_write_does_not_trigger_verification_pressure() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({
            "success": true,
            "path": "README.md",
            "diff_preview": {
                "content": "",
                "truncated": false,
                "omitted_line_count": 0,
                "skipped": false,
                "is_empty": true
            },
            "diff": [{
                "path": "README.md",
                "content": "",
                "truncated": false,
                "omitted_line_count": 0,
                "skipped": false,
                "is_empty": true
            }]
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &outcome,
        tools::WRITE_FILE,
        &json!({"path":"README.md","content":"same\n","mode":"overwrite"}),
    );

    assert_eq!(tracker.consecutive_mutations, 0);
}
#[test]
fn skipped_write_does_not_trigger_verification_pressure() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({
            "success": true,
            "skipped": true,
            "reason": "File already exists"
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &outcome,
        tools::WRITE_FILE,
        &json!({"path":"README.md","content":"same\n","mode":"skip_if_exists"}),
    );

    assert_eq!(tracker.consecutive_mutations, 0);
}
#[test]
fn logged_compound_inspections_do_not_trigger_anti_blind_pressure() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for command in [
        "cat README.md && printf '\\n--- git status ---\\n' && git status --short",
        "wc -l README.md; rg -n '^#' README.md",
        "git diff --stat; find docs -maxdepth 2 -type f | sort | head -40",
    ] {
        update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd":command}));
    }

    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.consecutive_navigations, 3);
}
#[test]
fn awk_range_inspections_do_not_trigger_anti_blind_pressure() {
    // Regression for session-vtcode-20260921T023834Z: six consecutive
    // read-only `awk` page reads tripped the blind-editing gate because
    // `awk` was missing from the read-only allow-list.
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for _ in 0..BLIND_EDITING_THRESHOLD {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tools::EXEC_COMMAND,
            &json!({"cmd": "awk 'NR>=297 && NR<=312' README.md"}),
        );
    }

    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.verification_is_pending());
}
#[test]
fn awk_write_primitives_still_trigger_anti_blind_pressure() {
    // Asymmetric counterpart: real writes (including gawk `@` indirect
    // calls) must still count as mutations so the fix cannot
    // over-correct into a fail-open.
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for _ in 0..BLIND_EDITING_THRESHOLD {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tools::EXEC_COMMAND,
            &json!({"cmd": "awk -v f=system 'BEGIN{@f(\"id\")}' README.md"}),
        );
    }

    assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
    assert!(!tracker.verification_is_pending(), "mutation debt belongs to the registry");
}
#[cfg(unix)]
#[test]
fn logged_compound_inspection_with_unix_stderr_suppression_does_not_trigger_pressure() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let command = r###"git diff --stat; find docs -maxdepth 2 -type f | sort | head -40; rg -n "vtcode init|vtcode models|full-auto|run-debug|cargo install" docs/user-guide docs/installation docs/development 2>/dev/null | head -50"###;
    update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd":command}));

    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 1);
}
#[test]
fn only_a_completed_verification_clears_pending_mutation_pressure() {
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let edit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for _ in 0..BLIND_EDITING_THRESHOLD {
        update_repetition_tracker(&mut tracker, &edit, tools::EDIT_FILE, &json!({"path":"src/lib.rs"}));
    }
    assert!(tracker.verification_is_pending());

    let failed_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::EXEC_COMMAND.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "check could not start".to_string(),
        ),
    });
    update_repetition_tracker(&mut tracker, &failed_check, tools::EXEC_COMMAND, &json!({"cmd":"cargo nextest run"}));
    assert!(tracker.verification_is_pending());

    update_repetition_tracker(&mut tracker, &edit, tools::EXEC_COMMAND, &json!({"cmd":"cargo nextest run"}));
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.consecutive_mutations, 0);
}
#[test]
fn carried_verification_checkpoint_clears_after_successful_check() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    let successful_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &successful_check,
        tools::EXEC_COMMAND,
        &json!({"cmd":"cargo check --locked"}),
    );

    assert!(!tracker.verification_is_pending());
}
#[test]
fn verification_snapshot_round_trips_through_session_state() {
    let tracker = LoopTracker::with_verification_snapshot((true, 0));
    assert_eq!(tracker.verification_snapshot(), (true, 0));
    assert_eq!(LoopTracker::new().verification_snapshot(), (false, 0));
}
#[test]
fn repetition_tracker_ignores_cancellations() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled);

    update_repetition_tracker(&mut tracker, &outcome, "edit_file", &json!({"path":"src/main.rs"}));

    assert_eq!(tracker.max_count_filtered(|_| false), 0);
}
#[test]
fn reset_after_balancer_recovery_preserves_anti_blind_state() {
    let mut tracker = LoopTracker::new();
    tracker.record("code_search:{\"query\":\"Widget\"}".to_string());
    tracker.record("code_search:{\"query\":\"Widget\"}".to_string());
    tracker.consecutive_mutations = 2;
    tracker.verification_pending = true;
    tracker.verification_warning_emitted = true;
    tracker.verification_block_notice_emitted = true;
    tracker.verification_result_lost_notice_pending = true;
    tracker.piped_verification_notice_pending = true;
    tracker.consecutive_navigations = 4;
    tracker.consecutive_low_signal_navigations = 3;
    tracker.total_low_signal_navigations = 7;
    tracker.record_low_signal("code_search::Widget::src".to_string());
    tracker.navigation_loop_recoveries = 3;

    tracker.reset_after_balancer_recovery();

    assert_eq!(tracker.max_count_filtered(|_| false), 0);
    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.consecutive_mutations, 2);
    assert!(tracker.verification_pending);
    assert!(tracker.verification_is_pending());

    assert!(tracker.verification_warning_emitted);
    assert!(tracker.verification_block_notice_emitted);
    assert!(tracker.verification_result_lost_notice_pending);
    assert!(tracker.piped_verification_notice_pending);
    assert_eq!(tracker.consecutive_navigations, 0);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
    assert_eq!(tracker.total_low_signal_navigations, 0);
    assert_eq!(tracker.navigation_loop_recoveries, 3);
}
#[test]
fn balancer_recovery_preserves_mutation_counts_without_arming_verification() {
    let mut tracker = LoopTracker::new();
    for _ in 0..(BLIND_EDITING_THRESHOLD - 1) {
        tracker.record_successful_mutation();
    }
    assert!(!tracker.verification_is_pending());

    tracker.reset_after_balancer_recovery();
    tracker.record_successful_mutation();

    assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
    assert!(!tracker.verification_is_pending(), "the registry owns completion debt");
}
