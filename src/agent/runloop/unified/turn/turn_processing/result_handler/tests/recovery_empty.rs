use super::*;

#[tokio::test]
async fn tool_free_recovery_texts_bypass_verification_accounting() {
    // Regression: with tools disabled at the API level no text could
    // verify, so counting recovery synthesis toward the verification cap
    // punished the model for obeying the recovery contract. Recovery
    // budgets govern this path instead; the generic cap still refuses
    // unverified completion (cap_ends_completed requires a clear gate).
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("rustc --version");
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    let mut turn_modified_files = BTreeSet::new();

    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("post-tool follow-up failure");
    assert!(ctx.consume_recovery_pass());
    assert!(ctx.in_tool_free_recovery_synthesis());
    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: "Synthesizing the gathered evidence into a final answer.".to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 8,
        tool_repeat_limit: 4,
    })
    .await
    .expect("recovery synthesis should be handled");

    // Clean recovery prose completes via the recovery path — never via
    // the verification-blocked handoff, and without consuming any
    // verification budget or firing the harness executor.
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(repeated_tool_attempts.verification_is_pending());
    assert_eq!(repeated_tool_attempts.verification_auto_recovery_attempts(), 0);
    assert!(!repeated_tool_attempts.auto_verification_executed);
}
#[tokio::test]
async fn refusal_blocks_turn_with_reason_even_during_recovery() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let reason = "The model declined this request (category: cyber).".to_string();
    let blocked_with_reason = {
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Refusal { reason: reason.clone() },
            response_streamed: true,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("refusal should be handled");

        matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(ref blocked) }) if *blocked == reason
        )
    };

    assert!(blocked_with_reason);
    assert!(backing.turn_refused(), "refusal must mark the turn for history rollback");
}
#[tokio::test]
async fn recovery_empty_response_emits_fallback_and_completes_turn() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::Empty { facts: NoOutputFacts::default() },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("recovery empty response should be handled");

    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
    ));
    assert!(backing.last_history_message_contains(
        "I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
    ));
}
#[tokio::test]
async fn recovery_empty_response_fallback_stays_concise_without_evidence_dump() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("tell me more".to_string()));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "first tool output".to_string()));
    ctx.working_history
        .push(uni::Message::tool_response("call_2".to_string(), "second tool output".to_string()));
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::Empty { facts: NoOutputFacts::default() },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("recovery empty response should be handled");

    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
    ));
    // Spec tui-diagnostics-cleanup S2D: user-facing fallback stays concise;
    // evidence dumps stay in tool history, not the assistant answer.
    // `activate_recovery` defaults to ToolFreeSynthesis.
    assert!(backing.last_history_message_contains(
        "I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
    ));
    assert!(!backing.last_history_message_contains("Latest user request:"));
    assert!(!backing.last_history_message_contains("Tool output 1:"));
}
#[tokio::test]
async fn recovery_empty_response_fallback_omits_spool_excerpt() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();

    ctx.working_history
        .push(uni::Message::user("summarize the failed read".to_string()));
    ctx.working_history.push(uni::Message::tool_response(
        "call_1".to_string(),
        serde_json::json!({
            "path": "src/main.rs",
            "spool_path": ".vtcode/context/tool_outputs/read_1.txt",
            "preview": "fallback-line-1\nfallback-line-2"
        })
        .to_string(),
    ));
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::Empty { facts: NoOutputFacts::default() },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("recovery empty response should be handled");

    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
    ));
    assert!(backing.last_history_message_contains(
        "I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
    ));
    assert!(!backing.last_history_message_contains("Spool excerpt:"));
    assert!(!backing.last_history_message_contains("fallback-line-1"));
}
#[tokio::test]
async fn recovery_retry_empty_response_emits_fallback_and_completes_turn() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.push_system_message("prior context");
    ctx.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
    assert!(ctx.consume_recovery_pass());

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::Empty { facts: NoOutputFacts::default() },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("recovery retry empty response should be handled");

    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: _ })
    ));
    assert!(
        backing
            .last_history_message_contains("I couldn't continue because the model returned no answer twice in a row.")
    );
}
#[tokio::test]
async fn recovery_textual_tool_markup_breaks_turn_as_blocked() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: r#"
<minimax:tool_call>
<invoke name="apply_patch">
<parameter name="action">read</parameter>
<parameter name="path">crates/codegen/vtcode-core/src/core/agent/runtime/mod.rs</parameter>
</invoke>
</minimax:tool_call>
"#
            .to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("recovery textual tool markup should be handled");

    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
        if reason == RECOVERY_CONTRACT_VIOLATION_REASON
    ));
}
#[tokio::test]
async fn empty_response_schedules_tool_enabled_retry_without_prior_tool_activity() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = {
        let mut ctx = backing.turn_processing_context();
        handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Empty { facts: NoOutputFacts::default() },
            response_streamed: true,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("empty response should schedule recovery")
    };

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!backing.recovery_is_tool_free());
    assert!(backing.last_history_message_contains("Tools remain available"));
}
#[tokio::test]
async fn empty_response_after_tool_activity_schedules_tool_free_recovery() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::assistant("Running cargo fmt now.".to_string()).with_tool_calls(vec![
                uni::ToolCall::function(
                    "call_1".to_string(),
                    "exec_command".to_string(),
                    r#"{"action":"run","command":"cargo fmt"}"#.to_string(),
                ),
            ]));
        ctx.working_history
            .push(uni::Message::tool_response("call_1".to_string(), "formatted".to_string()));

        handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Empty { facts: NoOutputFacts::default() },
            response_streamed: true,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("empty response after tool activity should schedule synthesis recovery")
    };

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(backing.recovery_is_tool_free());
}
#[tokio::test]
async fn planning_two_empty_responses_allow_one_synthesis_and_two_corrections() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    assert!(backing.recovery_is_tool_free());
    assert!(backing.last_history_message_contains("exactly one completed `<proposed_plan>` block"));
    for request in 1..=3 {
        let outcome = handle_planning_empty_with(
            &mut backing,
            NoOutputFacts::default(),
            request + 2,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
        )
        .await;
        if request < 3 {
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        } else {
            assert!(
                matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) if reason == "planning awaiting input")
            );
        }
    }
    assert_eq!(backing.turn_processing_context().plan_session.recovery.requests_used, 3);
    assert!(backing.last_history_message_contains("Planning needs your input. Saved progress is available."));
}
