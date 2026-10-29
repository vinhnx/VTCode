use super::*;

#[tokio::test]
async fn low_signal_search_churn_schedules_recovery_and_progress_only_recovery_text_completes() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_loop_limit(tool_names::CODE_SEARCH, 2);
    let seeded_args = json!({"query":"Result","path":"src"});
    assert!(backing.record_tool_call(tool_names::CODE_SEARCH, &seeded_args).is_none());
    let _ = backing.record_tool_call(tool_names::CODE_SEARCH, &seeded_args);
    let warning = backing.record_tool_call(tool_names::CODE_SEARCH, &seeded_args);
    assert!(warning.is_some());
    assert!(backing.is_hard_limit_exceeded(tool_names::CODE_SEARCH));
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results": []}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let same_args = json!({"query":"Result","path":"src"});
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

    let balancer_outcome = super::super::handle_turn_balancer(&mut ctx, 4, &mut tracker, 4, 3).await;
    assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert!(ctx.consume_recovery_pass());

    let recovery_outcome = ctx
        .handle_text_response("Let me try a narrower search next.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("recovery response should be handled");

    assert!(matches!(recovery_outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!ctx.is_recovery_active());
    assert!(backing.is_hard_limit_exceeded(tool_names::CODE_SEARCH));
}
#[tokio::test]
async fn early_low_signal_search_churn_schedules_recovery_before_turn_window() {
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results": []}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let same_args = json!({"query":"Result","path":"src"});
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

    let balancer_outcome = super::super::handle_turn_balancer(&mut ctx, 3, &mut tracker, 20, 3).await;
    assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert_eq!(tracker.consecutive_navigations, 0);
    assert!(ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Repeated low-signal navigation calls reached the per-turn fast-path cap")
    }));
    assert_recovery_expires_at_fresh_turn(ctx.working_history);
}
#[tokio::test]
async fn early_balancer_recovery_is_plan_aware_in_planning_workflow() {
    let mut backing = TestTurnProcessingBacking::new(20).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results": []}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let same_args = json!({"query":"Result","path":"src"});
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

    let balancer_outcome = super::super::handle_turn_balancer(&mut ctx, 3, &mut tracker, 20, 3).await;
    assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert!(tracker.planning_low_signal_synthesis_triggered);
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("<proposed_plan>") })
    );
}
#[tokio::test]
async fn early_balancer_recovery_stays_generic_outside_planning_workflow() {
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results": []}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let same_args = json!({"query":"Other","path":"src"});
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
    update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

    let balancer_outcome = super::super::handle_turn_balancer(&mut ctx, 3, &mut tracker, 20, 3).await;
    assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert!(!tracker.planning_low_signal_synthesis_triggered);
    assert!(ctx.working_history.iter().any(|message| {
        let text = message.content.as_text();
        text.contains("summarize only from collected evidence") && !text.contains("<proposed_plan>")
    }));
}
#[tokio::test]
async fn early_balancer_trips_on_same_root_listings_below_low_signal_limit() {
    // With a lenient repeat limit (5), three same-binary same-root
    // listings stay below the low-signal cap but still trip recovery
    // through the coarse-listing branch once consecutive navigations
    // reach the limit. The armed reason names the looped family.
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let hit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results": [{"path": "src/main.rs"}]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for command in ["ls src", "ls -1 src", "ls src/"] {
        update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &json!({"cmd":command}));
    }
    for query in ["TurnLoop", "LoopTracker"] {
        update_repetition_tracker(&mut tracker, &hit, tool_names::CODE_SEARCH, &json!({"query":query,"path":"src"}));
    }
    assert!(tracker.max_low_signal_count() < 5);

    let balancer_outcome = super::super::handle_turn_balancer(&mut ctx, 5, &mut tracker, 20, 5).await;
    assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    let reason = ctx.recovery_reason().unwrap_or_default();
    assert!(reason.contains("Top churn: exec::inspection::ls::src ×3."));
}
#[tokio::test]
async fn diverse_root_listings_stay_below_early_recovery() {
    // Scans of distinct trees are legitimate exploration: three
    // same-binary listings of different roots must not schedule the early
    // recovery pass, because the coarse family is scoped by search root.
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for command in ["ls src", "ls crates", "ls tests"] {
        update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &json!({"cmd":command}));
    }
    assert_eq!(tracker.max_coarse_listing_count(), 1);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 20, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
}
#[tokio::test]
async fn balancer_does_not_reclaim_an_already_pending_recovery() {
    // When the blocked-tool fuse armed a recovery earlier in the same
    // batch, the balancer condition can still trip on the same tracker
    // state. It must not announce a second "scheduling" pass or overwrite
    // the armed reason — `activate_recovery` reports the no-op and the
    // messaging stays silent.
    let mut backing = TestTurnProcessingBacking::new(120).await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("blocked-tool recovery");
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for command in ["ls src", "ls -1 src", "ls src/"] {
        update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &json!({"cmd":command}));
    }

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert_eq!(ctx.recovery_reason(), Some("blocked-tool recovery"));
    assert!(
        !ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("summarize only from collected evidence"))
    );
    assert!(!ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Repeated low-signal navigation calls reached")
    }));
}
#[tokio::test]
async fn retained_search_results_converge_before_the_loop_budget() {
    let mut backing = TestTurnProcessingBacking::new(32).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let hit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results":[{"result_type":"text","path":"README.md","line":10,"snippet":"## Overview"}]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for step in 0..4 {
        update_repetition_tracker(
            &mut tracker,
            &hit,
            tool_names::CODE_SEARCH,
            &json!({"query":format!("query{step}"),"path":"README.md"}),
        );
        let outcome = super::super::handle_turn_balancer(&mut ctx, step + 1, &mut tracker, 60, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert_eq!(ctx.is_recovery_active(), step == 3);
    }
    assert!(ctx.recovery_reason().unwrap().contains("low-signal"));
    assert!(!tracker.verification_is_pending());
}
