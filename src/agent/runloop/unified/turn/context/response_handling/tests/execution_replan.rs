use super::*;

#[tokio::test]
async fn approved_execution_replan_continues_without_second_confirmation() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.persist_approved_plan_for_test(EXECUTION_REVISION_PLAN).await;
    backing.set_approved_plan_execution_for_test(true);
    backing.set_skip_confirmations_for_test(false);
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(
            "Replanning: the approved paths no longer exist.".to_string(),
            Vec::new(),
            None,
            Some(EXECUTION_REVISION_PLAN.to_string()),
            false,
        )
        .await
        .expect("execution replan should route through the approval handoff");

    assert!(
        matches!(
            outcome,
            TurnHandlerOutcome::BreakWithPolicy {
                result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                target,
            } if !target.skip_confirmations,
        ),
        "the revised plan must schedule the continuation turn while keeping the session confirmation policy"
    );
}
#[tokio::test]
async fn full_auto_execution_replan_continues_without_second_confirmation() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.persist_approved_plan_for_test(EXECUTION_REVISION_PLAN).await;
    backing.set_skip_confirmations_for_test(true);
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(
            "Replanning: the approved paths no longer exist.".to_string(),
            Vec::new(),
            None,
            Some(EXECUTION_REVISION_PLAN.to_string()),
            false,
        )
        .await
        .expect("full-auto replan should continue automatically");

    assert!(
        matches!(
            outcome,
            TurnHandlerOutcome::BreakWithPolicy {
                result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                target,
            } if target.skip_confirmations,
        ),
        "a policy-automatic replan must schedule the continuation turn"
    );
}
#[tokio::test]
async fn execution_mode_invalid_unapproved_plan_gets_tool_free_repair_and_reaches_approval() {
    // session-vtcode-20260924T133543Z_155288-07964: Build mode produced
    // a useful README plan with bold labels instead of required headings.
    const INVALID_README_PLAN: &str = "**Goal:** Improve README density.\n\n1. Fix the broken link in `README.md`.\n\n**Verification:** Check links.\n";
    const REPAIRED_README_PLAN: &str = "## Summary\nImprove README density.\n\n## Implementation Steps\n1. Fix the broken link -> files: [README.md] -> verify: [rg -n 'Loop engineering' README.md]\n\n## Test Cases and Validation\n- Confirm the link with rg -n 'Loop engineering' README.md.\n\n## Assumptions and Defaults\n- Keep unrelated README sections as they are.\n";

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();

    let first = ctx
        .handle_text_response(String::new(), Vec::new(), None, Some(INVALID_README_PLAN.to_string()), false)
        .await
        .expect("invalid unapproved plan should schedule repair");
    assert!(matches!(first, TurnHandlerOutcome::Continue));
    assert!(ctx.recovery_is_tool_free(), "repair must not expose edit tools before approval");
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message.content.as_text().contains("missing required section(s): Summary")
            && message.content.as_text().contains(CANONICAL_STEP_FORMAT)
    }));
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant && message.content.as_text().contains(INVALID_README_PLAN)
    }));

    let validation = validate_plan_content(REPAIRED_README_PLAN);
    assert!(validation.is_ready(), "corrected fixture must validate: {:?}", validation.reasons());
    assert!(ctx.consume_recovery_pass());
    let repaired = ctx
        .handle_text_response(String::new(), Vec::new(), None, Some(REPAIRED_README_PLAN.to_string()), false)
        .await
        .expect("corrected plan should reach approval");
    let outcome_kind = match &repaired {
        TurnHandlerOutcome::Continue => "continue".to_string(),
        TurnHandlerOutcome::Break(result) => format!("break: {result:?}"),
        TurnHandlerOutcome::BreakWithPolicy { result, .. } => format!("break with policy: {result:?}"),
        TurnHandlerOutcome::SwitchPrimaryAgent(_) => "switch primary agent".to_string(),
        TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { .. } => "switch primary agent with policy".to_string(),
    };
    assert!(
        matches!(
            repaired,
            TurnHandlerOutcome::BreakWithPolicy {
                result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                ..
            }
        ),
        "a corrected Build-mode plan should reach the approval handoff, got {outcome_kind}"
    );
}
#[tokio::test]
async fn execution_mode_invalid_unapproved_plan_repair_is_bounded() {
    const INVALID_PLAN: &str = "**Goal:** Improve README.md.\n\n1. Fix a link in README.md.\n";
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();

    let first = ctx
        .handle_text_response(String::new(), Vec::new(), None, Some(INVALID_PLAN.to_string()), false)
        .await
        .expect("first invalid plan should schedule repair");
    assert!(matches!(first, TurnHandlerOutcome::Continue));

    assert!(ctx.consume_recovery_pass());
    let second = ctx
        .handle_text_response(String::new(), Vec::new(), None, Some(INVALID_PLAN.to_string()), false)
        .await
        .expect("a failed repair should end with feedback");
    assert_terminal_plan_rejection_is_blocked(&second);
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text().contains(EXECUTION_PLAN_REJECTION_NOTICE)
    }));
    // Terminal rejection must leave the validator contract in history so a
    // later `continue` can repair the draft (session-vtcode-20260924T133543Z).
    let final_text = last_final_answer_text(&ctx);
    assert_history_carries_validator_feedback(&final_text);
    assert!(
        final_text.contains("missing required section"),
        "history must name the missing sections, got: {final_text}"
    );
}
#[tokio::test]
async fn execution_mode_replan_rejection_surfaces_feedback_without_repair_directive() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_approved_plan_execution_for_test(true);
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(
            "Replanning: the approved plan is stale.".to_string(),
            Vec::new(),
            None,
            Some("not a valid plan artifact".to_string()),
            false,
        )
        .await
        .expect("invalid replan should end the turn with feedback");

    assert_terminal_plan_rejection_is_blocked(&outcome);
    assert!(
        !ctx.working_history
            .iter()
            .any(|message| message.role == uni::MessageRole::System
                && message.content.as_text().contains("Planning recovery")),
        "execution-mode rejection must not push planning repair directives"
    );
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains("not a valid plan artifact")),
        "the rejected draft stays attached to history for inspection"
    );
    assert!(
        ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().contains(EXECUTION_PLAN_REJECTION_NOTICE)
        }),
        "a rejected revision must publish a harness-visible final response"
    );
    assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
    assert!(
        ctx.harness_state.final_response_rendered(),
        "a rejected revision must count as a rendered final response"
    );
}
#[tokio::test]
async fn plan_only_execution_rejection_still_publishes_final_response() {
    use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails, VersionedThreadEvent};

    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_approved_plan_execution_for_test(true);
    let events_path = backing.enable_harness_emitter();
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(String::new(), Vec::new(), None, Some("not a valid plan artifact".to_string()), false)
        .await
        .expect("plan-only invalid replan should end the turn with feedback");

    assert_terminal_plan_rejection_is_blocked(&outcome);
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text().contains(EXECUTION_PLAN_REJECTION_NOTICE)
    }));
    assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
    assert!(!ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("The turn stopped before a final assistant response")
    }));
    assert!(
        ctx.harness_state.final_response_event_emitted(),
        "a plan-only rejection must publish the final assistant event"
    );

    // Host runtimes never see renderer lines: the rejected draft must also
    // travel as canonical thread events, like the planning-mode path.
    let events = tokio::fs::read_to_string(&events_path).await.expect("rejection event log");
    let rejected = events
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned event")
                .into_event()
        })
        .find_map(|event| match event {
            ThreadEvent::ItemCompleted(completed) if completed.item.id.ends_with("-rejected-plan") => {
                Some(completed.item)
            }
            _ => None,
        })
        .expect("execution rejection must publish the rejected draft as a plan item");
    let ThreadItemDetails::Plan(plan) = &rejected.details else {
        panic!("rejected item must carry plan details");
    };
    assert!(plan.text.contains("not a valid plan artifact"));

    // Execution mode has no recovery cycle: a one-shot pending flag rearms
    // the repair budget on the next explicit user turn (turn_prep).
    assert!(
        ctx.plan_session.take_execution_plan_rejection_pending(),
        "terminal execution rejection must arm the rearm flag"
    );
    assert!(!ctx.plan_session.take_execution_plan_rejection_pending(), "the rearm flag must fire once");
}
