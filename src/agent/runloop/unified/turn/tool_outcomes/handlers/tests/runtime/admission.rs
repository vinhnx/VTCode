use super::*;

#[tokio::test]
async fn blocked_tool_call_guard_emits_tool_and_system_messages() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    let mut outcome = None;
    for idx in 0..=max_streak {
        outcome = enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
    }

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(ctx.harness_state.blocked_tool_recovery_pending());
    flush_blocked_tool_recovery(&mut ctx);
    assert!(!ctx.harness_state.blocked_tool_recovery_pending());
    assert!(ctx.harness_state.recovery_is_tool_free());
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("blocked_streak"))
    );
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message
                .content
                .as_text()
                .contains("A bounded recovery response will run without more tool calls")
    }));

    // UI synchronization: guard trips must clear any inline loading placeholder
    // so the bottom-line doesn't remain stuck. Input status left/right should
    // be None after the guard fires.
    assert!(ctx.input_status_state.left.is_none() && ctx.input_status_state.right.is_none());
}

#[tokio::test]
async fn blocked_tool_call_guard_allows_configured_consecutive_cap() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    for idx in 0..max_streak {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "blocked call {idx} should stay under cap");
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
}

#[tokio::test]
async fn blocked_tool_call_guard_caps_non_consecutive_total_churn() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let limits = blocked_tool_call_limits(&ctx);
    let args = json!({"path":"src/main.rs"});

    for idx in 0..limits.total_cap {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "blocked total {idx} should stay under cap");
        ctx.reset_blocked_tool_call_streak();
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_total_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    flush_blocked_tool_recovery(&mut ctx);
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("blocked_total"))
    );
    assert!(ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains(&format!("{} total blocked calls", limits.total_cap))
    }));
}

#[tokio::test]
async fn blocked_tool_call_guard_allows_four_times_total_churn_in_planning_mode() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let limits = blocked_tool_call_limits(&ctx);
    let args = json!({"path":"src/main.rs"});

    assert_eq!(limits.total_cap, limits.consecutive_cap * 4);
    for idx in 0..limits.total_cap {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "blocked total {idx} should stay under planning cap");
        ctx.reset_blocked_tool_call_streak();
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_total_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    flush_blocked_tool_recovery(&mut ctx);
    assert!(ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains(&format!("{} total blocked calls", limits.total_cap))
    }));
}

#[tokio::test]
async fn denied_execution_result_uses_planning_total_fuse() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let limits = blocked_tool_call_limits(&ctx);
    let args = json!({"path": "src/main.rs"});
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    for index in 0..limits.total_cap {
        let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: ToolExecutionError::policy_violation("read_file", "denied for test"),
        });
        let outcome = handle_tool_execution_result(
            &mut outcome_ctx,
            format!("denied_{index}"),
            tool_names::READ_FILE,
            &args,
            &pipeline_outcome,
            Instant::now(),
        )
        .await
        .expect("denied execution result should be handled");
        assert!(outcome.is_none(), "planning denial {index} should stay under total fuse");
        outcome_ctx.ctx.reset_blocked_tool_call_streak();
    }

    let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: ToolExecutionError::policy_violation("read_file", "denied for test"),
    });
    let outcome = handle_tool_execution_result(
        &mut outcome_ctx,
        "denied_over_cap".to_string(),
        tool_names::READ_FILE,
        &args,
        &pipeline_outcome,
        Instant::now(),
    )
    .await
    .expect("total-fuse denial should be handled");

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(outcome_ctx.ctx.harness_state.blocked_tool_recovery_pending());
}

#[tokio::test]
async fn blocked_tool_call_guard_short_circuits_to_recovery_when_active() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path":"src/main.rs"});
    ctx.activate_recovery("loop detector");

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_recovery", tool_names::READ_FILE, &args);

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
}
