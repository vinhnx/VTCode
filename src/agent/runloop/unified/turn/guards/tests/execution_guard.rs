use super::*;

#[tokio::test]
async fn execution_total_low_signal_guard_trips_at_threshold() {
    // Diverse empty searches (a new query each time) never trip the
    // per-family fast-path; the execution-mode total guard converges the
    // turn at 12 low-signal outcomes without productive work.
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results":[]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD - 1) {
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::CODE_SEARCH,
            &json!({"query": format!("q{step}"), "path": "src"}),
        );
    }
    let outcome = super::super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 40, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());

    update_repetition_tracker(
        &mut tracker,
        &miss,
        tool_names::CODE_SEARCH,
        &json!({"query": "q-final", "path": "src"}),
    );
    let outcome = super::super::handle_turn_balancer(&mut ctx, 13, &mut tracker, 40, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    let reason = ctx.recovery_reason().unwrap_or_default();
    assert!(reason.contains("Diverse low-signal navigation reached 12"));
    assert_recovery_expires_at_fresh_turn(ctx.working_history);
}
#[tokio::test]
async fn execution_total_low_signal_guard_fires_once_per_turn() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    tracker.execution_total_low_signal_triggered = true;
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results":[]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD + 3) {
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::CODE_SEARCH,
            &json!({"query": format!("r{step}"), "path": "src"}),
        );
    }

    let outcome = super::super::handle_turn_balancer(&mut ctx, 16, &mut tracker, 40, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
}
#[tokio::test]
async fn execution_total_low_signal_window_resets_on_mutation() {
    // A productive mutation resets the low-signal window, so churn
    // interrupted by real work must not accumulate across the reset.
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results":[]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD - 1) {
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::CODE_SEARCH,
            &json!({"query": format!("a{step}"), "path": "src"}),
        );
    }
    update_repetition_tracker(
        &mut tracker,
        &ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({}),
            stdout: None,
            modified_files: vec!["src/main.rs".to_string()],
            command_success: true,
        }),
        tool_names::EDIT_FILE,
        &json!({"path":"src/main.rs","old_string":"a","new_string":"b"}),
    );
    assert_eq!(tracker.total_low_signal_navigations, 0);

    for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD - 1) {
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::CODE_SEARCH,
            &json!({"query": format!("b{step}"), "path": "src"}),
        );
    }
    let outcome = super::super::handle_turn_balancer(&mut ctx, 24, &mut tracker, 40, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
}
