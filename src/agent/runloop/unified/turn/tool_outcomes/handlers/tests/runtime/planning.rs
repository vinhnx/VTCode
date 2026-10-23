use super::*;

#[tokio::test]
async fn planning_mode_allows_request_user_input_blocked_through_to_failure() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let args = json!({"questions": [{"id": "q1", "header": "Q1", "question": "Test?"}]});

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "interview_blocked", tool_names::REQUEST_USER_INPUT, &args);

    assert!(
        outcome.is_none(),
        "Blocked request_user_input should flow through to handle_failure, not fast-exit from guard"
    );
    assert!(
        !ctx.plan_session.is_interview_denied(),
        "Interview denial is handled by handle_failure in execution_result.rs, not the guard"
    );
    assert_eq!(ctx.harness_state.consecutive_blocked_tool_calls, 1, "blocked call should be recorded");
}

#[tokio::test]
async fn permanent_interview_denial_switches_to_tool_free_plan_recovery() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    backing
        .tool_registry
        .set_tool_policy(tool_names::REQUEST_USER_INPUT, ToolPolicy::Deny)
        .await
        .expect("deny interview tool");

    let mut ctx = backing.turn_processing_context();
    let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
        "interview_denied".to_string(),
        tool_names::REQUEST_USER_INPUT.to_string(),
        json!({"questions": [{"id": "q1", "header": "Q1", "question": "Test?"}]}).to_string(),
    ));
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_tool_calls(&mut outcome_ctx, &[call])
        .await
        .expect("denied interview should be handled as a tool response");

    assert!(outcome.is_none());
    assert!(outcome_ctx.ctx.plan_session.is_interview_denied());
    assert!(outcome_ctx.ctx.harness_state.is_recovery_active());
    assert!(outcome_ctx.ctx.harness_state.recovery_is_tool_free());
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("synthesize exactly one completed `<proposed_plan>`")
    }));
}

#[tokio::test]
async fn planning_mode_non_planning_tool_blocked_calls_schedule_recovery() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path": "src/main.rs"});
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);

    for idx in 0..max_streak {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "non-planning blocked call {idx} should stay under cap");
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(ctx.harness_state.blocked_tool_recovery_pending());
}

#[tokio::test]
async fn blocked_batch_drains_responses_and_schedules_tool_free_recovery() {
    let mut backing = TestContextBacking::new(4).await;
    backing
        .tool_registry
        .set_tool_policy(tool_names::READ_FILE, ToolPolicy::Deny)
        .await
        .expect("deny read_file");
    let mut ctx = backing.turn_processing_context();
    ctx.full_auto = true;
    let args_for_call = |index: usize| json!({"path": format!("src/main_{index}.rs")});
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let tool_calls = (0..=max_streak)
        .map(|index| {
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                format!("blocked_batch_{index}"),
                tool_names::READ_FILE.to_string(),
                args_for_call(index).to_string(),
            ))
        })
        .collect::<Vec<_>>();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_tool_calls(&mut outcome_ctx, &tool_calls)
        .await
        .expect("blocked batch should schedule recovery");

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(outcome_ctx.ctx.harness_state.is_recovery_active());
    assert!(outcome_ctx.ctx.harness_state.recovery_is_tool_free());
    assert_eq!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .count(),
        tool_calls.len(),
        "every assistant tool call needs a response before recovery"
    );
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message.content.as_text().contains("tools are disabled for one bounded pass")
    }));
}
