use super::*;

fn record_distinct_inspection(tracker: &mut LoopTracker, index: usize) {
    // Complete/spooled reads and changing ranges must share the execution
    // progress window even when positioned-evidence deduplication cannot apply.
    let command = if index.is_multiple_of(2) {
        format!("sed -n '{},{}p' src/card.rs", index + 1, index + 8)
    } else {
        format!("cat .vtcode/context/tool_outputs/result-{index}.txt | head -n 40")
    };
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"output": format!("evidence-{index}"), "exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(tracker, &outcome, tool_names::EXEC_COMMAND, &json!({"cmd": command}));
}

#[tokio::test]
async fn approved_execution_bounds_distinct_inspections_and_recurrent_stall() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_approved_plan_execution_for_test(true);
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    for index in 0..NAVIGATION_LOOP_THRESHOLD - 1 {
        record_distinct_inspection(&mut tracker, index);
    }
    super::super::handle_turn_balancer(&mut ctx, 14, &mut tracker, 3).await;
    assert!(!ctx.is_recovery_active(), "bounded missing-evidence research stays available");
    record_distinct_inspection(&mut tracker, NAVIGATION_LOOP_THRESHOLD - 1);
    assert_eq!(tracker.repeated_navigation_count(), 0, "the calls deliberately have distinct signatures");
    let outcome = super::super::handle_turn_balancer(&mut ctx, 15, &mut tracker, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert_eq!(tracker.navigation_loop_recoveries, 1);

    for index in 30..30 + NAVIGATION_LOOP_THRESHOLD {
        record_distinct_inspection(&mut tracker, index);
    }
    let outcome = super::super::handle_turn_balancer(&mut ctx, 30, &mut tracker, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
}

#[tokio::test]
async fn diverse_research_without_approved_execution_remains_available() {
    for planning in [false, true] {
        let mut backing = TestTurnProcessingBacking::new().await;
        if planning {
            backing.activate_planning_for_test();
        }
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        for index in 0..NAVIGATION_LOOP_THRESHOLD + 2 {
            record_distinct_inspection(&mut tracker, index);
        }
        let outcome = super::super::handle_turn_balancer(&mut ctx, 17, &mut tracker, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active(), "planning={planning}");
    }
}

#[tokio::test]
async fn approved_execution_edit_resets_inspection_window() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_approved_plan_execution_for_test(true);
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    for index in 0..NAVIGATION_LOOP_THRESHOLD - 1 {
        record_distinct_inspection(&mut tracker, index);
    }
    update_repetition_tracker(
        &mut tracker,
        &ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({}),
            stdout: None,
            modified_files: vec!["src/card.rs".to_string()],
            command_success: true,
        }),
        tool_names::EDIT_FILE,
        &json!({"path": "src/card.rs", "old_string": "gap_1", "new_string": "gap_3"}),
    );
    record_distinct_inspection(&mut tracker, 40);
    assert_eq!(tracker.consecutive_navigations, 1);
    super::super::handle_turn_balancer(&mut ctx, 16, &mut tracker, 3).await;
    assert!(!ctx.is_recovery_active());
}
