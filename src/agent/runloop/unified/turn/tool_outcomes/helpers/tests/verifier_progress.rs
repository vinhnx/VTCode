use super::*;

#[test]
fn verifier_wait_window_grows_and_resets_with_the_session() {
    let mut tracker = LoopTracker::new();

    // The window doubles up to the cap, so a long verifier needs a handful of
    // polls instead of one per second.
    let windows: Vec<u64> = (0..6).map(|_| tracker.next_verifier_wait_timeout_secs()).collect();
    assert_eq!(windows, vec![1, 2, 4, 8, 8, 8]);
    assert!(windows.windows(2).all(|pair| pair[1] >= pair[0]), "the wait window must never shrink: {windows:?}");

    // A new verifier session starts a fresh window.
    tracker.reset_verifier_wait_polls();
    assert_eq!(tracker.next_verifier_wait_timeout_secs(), 1);
}
#[test]
fn unrelated_successful_checks_cannot_refill_required_verifier_progress() {
    let mut tracker = LoopTracker::new();
    for attempt in 1..=BLIND_EDITING_THRESHOLD {
        tracker.mark_verification_complete();
        assert_eq!(
            tracker.record_required_verifier_progress(Some("cargo clippy".into())),
            attempt == BLIND_EDITING_THRESHOLD
        );
    }
    assert!(
        !tracker.record_required_verifier_progress(Some("cargo check".into())),
        "advancing to another check starts a new window"
    );
    tracker.record_successful_mutation();
    assert!(
        !tracker.record_required_verifier_progress(Some("cargo check".into())),
        "real edits start a new verification generation"
    );
}
#[test]
fn tracker_observations_cannot_reset_navigation_or_verification_progress() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 1));
    let output = json!({"status":"ok", "checklist":{"items":[{"index":3,"status":"in_progress"}]}});
    let observed = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output,
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for budget in [1000, 2000] {
        update_repetition_tracker(
            &mut tracker,
            &observed,
            tools::TASK_TRACKER,
            &json!({"action":"list","max_output_tokens":budget}),
        );
    }
    assert!(tracker.tracker_observations_stalled());
    assert_eq!(tracker.consecutive_navigations, 2);
    assert_eq!(tracker.verification_snapshot(), (true, 0));
    let progress = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"status":"updated"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(
        &mut tracker,
        &progress,
        tools::TASK_TRACKER,
        &json!({"action":"update","index":3,"status":"completed"}),
    );
    assert!(!tracker.tracker_observations_stalled());
    assert_eq!(tracker.verification_snapshot(), (true, 0));
}
#[test]
fn rejected_verifier_does_not_grant_repair_edits() {
    for kind in [
        vtcode_core::tools::registry::ToolErrorType::InvalidParameters,
        vtcode_core::tools::registry::ToolErrorType::PermissionDenied,
        vtcode_core::tools::registry::ToolErrorType::PolicyViolation,
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::new(
                tools::EXEC_COMMAND,
                kind,
                "verifier rejected",
            ),
        });
        assert!(!update_repetition_tracker(
            &mut tracker,
            &outcome,
            tools::EXEC_COMMAND,
            &json!({"cmd":"cargo check --locked"})
        ));
        assert!(tracker.verification_is_pending());

        assert!(!tracker.take_verification_result_lost_notice());
        assert!(!tracker.take_verifier_failed_notice());
    }
}
#[test]
fn running_verifier_requires_its_own_terminal_session_result() {
    for exit_code in [0, 1] {
        let mut tracker = LoopTracker::new();
        tracker.mark_verification_pending();
        let running = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"session_id": "run-verifier", "lifecycle_state": "running"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &running, tools::EXEC_COMMAND, &json!({"cmd": "cargo check --locked"}));
        assert!(tracker.verification_is_pending());
        assert_eq!(tracker.pending_verifier_session_id.as_deref(), Some("run-verifier"));

        assert!(!tracker.take_verifier_failed_notice(), "running is not failed");

        let terminal = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"exit_code": exit_code}),
            stdout: None,
            modified_files: vec![],
            command_success: exit_code == 0,
        });
        update_repetition_tracker(
            &mut tracker,
            &terminal,
            tools::WRITE_STDIN,
            &json!({"session_id": "run-unrelated", "action": "wait"}),
        );
        assert!(tracker.verification_is_pending(), "unrelated completion cannot verify changes");
        assert!(!tracker.take_verifier_failed_notice(), "unrelated completion is not the verifier verdict");
        update_repetition_tracker(
            &mut tracker,
            &terminal,
            tools::WRITE_STDIN,
            &json!({"session_id": " run-verifier ", "action": "wait"}),
        );
        assert_eq!(tracker.verification_is_pending(), exit_code != 0);
        assert!(tracker.pending_verifier_session_id.is_none());
        assert_eq!(tracker.take_verifier_failed_notice(), exit_code != 0);
        assert!(!tracker.take_verifier_failed_notice(), "terminal feedback is consumed once");
    }
}
#[test]
fn session_cleanup_remains_available_while_verification_is_pending() {
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    for action in ["terminate", "close"] {
        let args = json!({"session_id": "run-cleanup", "action": action});

        assert!(vtcode_core::tools::tool_intent::is_turn_budget_exempt_call(tools::WRITE_STDIN, &args));
    }
}
