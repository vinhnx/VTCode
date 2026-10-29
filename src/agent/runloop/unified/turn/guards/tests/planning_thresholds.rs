use super::*;

#[tokio::test]
async fn planning_recovery_triggers_at_exact_consecutive_low_signal_threshold() {
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
    tracker.total_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
    assert_eq!(tracker.total_low_signal_navigations, 0);
    assert!(ctx.working_history.iter().any(|message| {
        message.content.as_text().contains("adaptive synthesis threshold")
            || message.content.as_text().contains("synthesize the plan")
            || message.content.as_text().contains("<proposed_plan>")
    }));
    assert_recovery_expires_at_fresh_turn(ctx.working_history);
}
#[tokio::test]
async fn unchanged_tracker_polls_get_one_correction_then_terminal_handoff() {
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    for round in 0..2 {
        let status = if round == 0 {
            ToolExecutionStatus::Success {
                output: json!({"status":"ok", "checklist":{"items":[{"index":3,"status":"in_progress"}]}}),
                stdout: None,
                modified_files: vec![],
                command_success: true,
            }
        } else {
            ToolExecutionStatus::Failure {
                error: vtcode_core::tools::registry::ToolExecutionError::new(
                    tool_names::TASK_TRACKER,
                    vtcode_core::tools::registry::ToolErrorType::PolicyViolation,
                    "identical tracker polls blocked",
                ),
            }
        };
        let outcome = ToolPipelineOutcome::from_status(status);
        for _ in 0..2 {
            update_repetition_tracker(&mut tracker, &outcome, tool_names::TASK_TRACKER, &json!({"action":"list"}));
        }
        let result = super::super::handle_turn_balancer(&mut ctx, 2 + round * 2, &mut tracker, 20, 3).await;
        if round == 0 {
            assert!(matches!(result, TurnHandlerOutcome::Continue));
            assert!(!ctx.is_recovery_active(), "correction must retain usable tools");
            assert!(tracker.tracker_observation_warning_emitted);
        } else {
            let TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) = result else {
                panic!("unchanged polls must not schedule another correction");
            };
            assert!(!crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(Some(&reason)));
        }
    }
}
#[tokio::test]
async fn planning_recovery_triggers_at_exact_total_low_signal_threshold() {
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    tracker.consecutive_low_signal_navigations = 1;
    tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;

    let outcome = super::super::handle_turn_balancer(&mut ctx, 10, &mut tracker, 120, 3).await;

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
    assert_eq!(tracker.total_low_signal_navigations, 0);
}
#[tokio::test]
async fn adaptive_low_signal_threshold_does_not_change_non_planning_turns() {
    let mut backing = TestTurnProcessingBacking::new(120).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
    tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;

    let outcome = super::super::handle_turn_balancer(&mut ctx, 1, &mut tracker, 120, 20).await;

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
}
#[tokio::test]
async fn adaptive_low_signal_synthesis_triggers_only_once_per_turn() {
    let mut backing = TestTurnProcessingBacking::new(120).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
    tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;

    let first = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(first, TurnHandlerOutcome::Continue));
    assert!(tracker.planning_low_signal_synthesis_triggered);
    assert!(
        ctx.working_history.iter().any(|message| {
            let text = message.content.as_text();
            text.contains("<proposed_plan>") && text.contains("Action -> files")
        }),
        "low-signal recovery must instruct plan-format synthesis"
    );

    tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
    tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;
    let second = super::super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 120, usize::MAX).await;
    assert!(matches!(second, TurnHandlerOutcome::Continue));
    assert_eq!(tracker.consecutive_low_signal_navigations, PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD);
    assert_eq!(tracker.total_low_signal_navigations, PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD);
}
