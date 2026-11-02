use super::*;

#[tokio::test]
async fn planning_empty_search_pipelines_schedule_bounded_synthesis() {
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let empty = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"exit_code": 0, "output": "", "total_output_bytes": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for index in 0..PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD {
        let command = format!("rg --files --hidden | rg 'optional-lint-{index}' | head -10");
        update_repetition_tracker(&mut tracker, &empty, tool_names::EXEC_COMMAND, &json!({"cmd": command}));
        if index + 1 < PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD {
            let outcome = super::super::handle_turn_balancer(&mut ctx, index as usize + 1, &mut tracker, 120, 3).await;
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
            assert!(!ctx.is_recovery_active());
        }
    }
    assert_eq!(tracker.max_low_signal_count(), 1);
    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert!(tracker.planning_low_signal_synthesis_triggered);
    assert!(ctx.working_history.iter().any(|message| {
        let text = message.content.as_text();
        text.contains("<proposed_plan>") && text.contains("Action -> files")
    }));
}
#[tokio::test]
async fn planning_listing_triplet_stays_below_early_recovery() {
    // Planning owns dedicated convergence guards (6/10 low-signal, 12-step
    // nav synthesis), so three successful same-base listings are legitimate
    // exploration and must not trip the generic fast-path. The fifth does.
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout": "src\nsrc-tauri"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for command in ["ls src/", "ls src", "ls -1 src"] {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": command, "command": command, "action": "run"}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 3);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active(), "planning triplet must not schedule early recovery");

    for command in ["ls -R src", "ls -lh src"] {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": command, "command": command, "action": "run"}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 5);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 8, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert!(
        ctx.working_history.iter().any(|message| {
            let text = message.content.as_text();
            text.contains("<proposed_plan>") && text.contains("Action -> files")
        }),
        "planning listing recovery must instruct plan-format synthesis"
    );
}
#[tokio::test]
async fn pending_verification_preserves_navigation_loop_recovery() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    tracker.consecutive_navigations = NAVIGATION_LOOP_THRESHOLD;

    let balancer_outcome = super::super::handle_turn_balancer(&mut ctx, 1, &mut tracker, 8, 3).await;
    assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
    assert_eq!(tracker.consecutive_navigations, 0);
    assert!(ctx.is_recovery_active());
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("Navigation loop detected") })
    );
    assert_recovery_expires_at_fresh_turn(ctx.working_history);
    assert!(ctx.consume_recovery_pass());

    let recovery_outcome = ctx
        .handle_text_response(
            "I'll inspect one more file and then summarize.".to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("recovery response should be handled");

    assert!(matches!(recovery_outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!ctx.is_recovery_active());
}
