use super::*;

#[tokio::test]
async fn tool_free_recovery_tolerates_intro_prose_around_valid_plan() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.activate_tool_free_recovery_for_test("per-file read cap");
    let mut ctx = backing.turn_processing_context();
    assert!(ctx.consume_recovery_pass());

    let outcome = ctx
        .handle_text_response(
            "Here is the plan from the gathered evidence.".to_string(),
            Vec::new(),
            None,
            Some(RECOVERY_VALID_PLAN.to_string()),
            false,
        )
        .await
        .expect("recovery response should be handled");

    // The old gate ended Blocked with "prose outside the required block".
    // Prose-tolerant recovery must persist the plan instead.
    assert!(
        !matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
        "intro prose around a valid plan must not end Blocked"
    );
}
#[tokio::test]
async fn tool_free_invalid_plan_uses_two_corrections_then_awaits_input() {
    use vtcode_core::exec::events::planning::PlanningOutcome;
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.activate_tool_free_recovery_for_test("per-file read cap");
    let mut ctx = backing.turn_processing_context();

    // Model the production provider boundary: each response consumes one
    // admitted synthesis/correction request, rather than merely changing a
    // turn-local validation counter.
    for attempt in 1..=3 {
        assert!(
            ctx.admit_planning_recovery_request("test-model")
                .await
                .expect("request admission")
        );
        assert!(ctx.consume_recovery_pass());
        let outcome = ctx
            .handle_text_response(String::new(), Vec::new(), None, Some(RECOVERY_INVALID_PLAN.to_string()), false)
            .await
            .expect("invalid draft should be handled");
        assert_eq!(ctx.plan_session.recovery.requests_used, attempt);
        if attempt < 3 {
            assert!(
                matches!(outcome, TurnHandlerOutcome::Continue),
                "request {attempt} should schedule the remaining correction"
            );
            assert!(ctx.recovery_is_tool_free(), "corrections cannot reopen research");
        } else {
            assert!(
                matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
                "the third failed request must await input"
            );
            assert_eq!(ctx.plan_session.recovery.outcome, PlanningOutcome::AwaitingInput);
            assert!(ctx.plan_session.recovery.input_prompt_shown);
        }
    }
    assert!(
        !ctx.admit_planning_recovery_request("test-model")
            .await
            .expect("exhausted admission")
    );
    assert_eq!(ctx.plan_session.recovery.requests_used, 3);
    assert!(!persisted_plan_is_ready(&ctx.tool_registry.planning_workflow_state()).await);
}
#[tokio::test]
async fn tool_free_invalid_plan_repair_preserves_draft_feedback_and_accepts_corrected_plan() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.activate_tool_free_recovery_for_test("preview budget exhausted");
    let events_path = backing.enable_harness_emitter();
    let mut ctx = backing.turn_processing_context();
    assert!(
        ctx.admit_planning_recovery_request("test-model")
            .await
            .expect("request admission")
    );
    assert!(ctx.consume_recovery_pass());

    let first = ctx
        .handle_text_response(
            String::new(),
            Vec::new(),
            None,
            Some(RECOVERY_INVALID_VERIFICATION_PLAN.to_string()),
            false,
        )
        .await
        .expect("invalid verification fixture should be handled");
    assert!(
        matches!(first, TurnHandlerOutcome::Continue),
        "the first invalid recovery draft must schedule bounded repair"
    );
    assert!(ctx.recovery_is_tool_free(), "the repair pass must keep tools disabled");

    let rejected_draft = ctx
        .working_history
        .iter()
        .find(|message| {
            message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains("Update the preview guard")
        })
        .expect("the rejected non-streaming draft must remain in assistant history");
    assert_eq!(rejected_draft.phase, Some(uni::AssistantPhase::Commentary));

    let feedback = ctx
        .working_history
        .iter()
        .find(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains("proposed plan was rejected")
        })
        .expect("validator-owned feedback must be retained for the repair pass")
        .content
        .as_text();
    assert!(feedback.contains("2 of 5 implementation step(s)"), "feedback should count both invalid steps");
    assert!(feedback.contains("step 1: verification item 1"));
    assert!(feedback.contains("step 5: verification item 1"));
    assert!(feedback.contains(CANONICAL_STEP_FORMAT), "the canonical step contract must remain available");
    assert!(feedback.contains("Repair the listed defects"));
    assert!(
        !feedback.contains("Valid examples:"),
        "repair should target the defects without repeating an example wall"
    );
    for (name, text) in [
        ("denied-interview-retry", DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE),
        ("pseudo-tool-reprompt", PLAN_PSEUDO_TOOL_CALL_REPROMPT_DIRECTIVE),
    ] {
        assert!(text.contains("sed -n") && text.contains("grep -n"), "{name} missing inspection examples: {text}");
        assert!(text.contains("git diff --check"), "{name} missing invalid VCS example: {text}");
    }
    assert!(
        !feedback.contains("1. Update the preview guard -> files:"),
        "validator-owned feedback must not echo the rejected draft"
    );

    assert!(
        ctx.admit_planning_recovery_request("test-model")
            .await
            .expect("request admission")
    );
    assert!(ctx.consume_recovery_pass());
    let repaired = ctx
        .handle_text_response(
            String::new(),
            Vec::new(),
            None,
            Some(RECOVERY_CORRECTED_VERIFICATION_PLAN.to_string()),
            false,
        )
        .await
        .expect("corrected recovery plan should be handled");
    assert!(
        matches!(
            repaired,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
        ),
        "the corrected draft must wait for separate human approval without scheduling implementation"
    );
    assert_eq!(
        ctx.plan_session.recovery.outcome,
        vtcode_core::exec::events::planning::PlanningOutcome::AwaitingApproval,
    );
    assert!(ctx.tool_registry.is_planning_active(), "saving a repaired draft cannot authorize implementation");
    assert!(
        ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains("Update the preview guard")
        }),
        "the rejected draft must remain available after the corrected plan is saved"
    );
    let plan_file = ctx
        .tool_registry
        .planning_workflow_state()
        .get_plan_file()
        .await
        .expect("the corrected plan should publish a plan file");
    let persisted_plan = std::fs::read_to_string(plan_file).expect("read the corrected plan");
    assert!(
        persisted_plan.contains("with concrete verification"),
        "the corrected fixture, rather than the rejected draft, must be persisted"
    );
    let events = tokio::fs::read_to_string(events_path).await.expect("approval event log");
    assert_eq!(events.matches("\"type\":\"plan.approval.requested\"").count(), 1);
    for event in events.lines().map(|line| {
        serde_json::from_str::<vtcode_core::exec::events::VersionedThreadEvent>(line)
            .expect("versioned event")
            .into_event()
    }) {
        if let vtcode_core::exec::events::ThreadEvent::PlanApprovalResolved(event) = event {
            assert!(!event.automatic, "recovery cannot authorize implementation");
            assert_eq!(
                event.decision,
                vtcode_core::exec::events::PlanApprovalDecision::Cancel,
                "disconnected approval UI may dismiss, but cannot approve"
            );
        }
    }
}
#[tokio::test]
async fn tool_free_unwrapped_plan_is_repaired_instead_of_blocking_immediately() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.activate_tool_free_recovery_for_test("per-file read cap");
    let mut ctx = backing.turn_processing_context();
    assert!(ctx.consume_recovery_pass());

    let outcome = ctx
        .handle_text_response(RECOVERY_INVALID_PLAN.to_string(), Vec::new(), None, None, false)
        .await
        .expect("unwrapped recovery draft should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "an unwrapped invalid draft should use the bounded repair path"
    );
    assert!(
        ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("Repair the listed defects")
                && message.content.as_text().contains(CANONICAL_STEP_FORMAT)
        }),
        "the repair directive should be retained for the next synthesis pass"
    );
}
#[tokio::test]
async fn untagged_planlike_text_schedules_bounded_repair_instead_of_generic_hint() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(UNTAGGED_PLANLIKE_TEXT.to_string(), Vec::new(), None, None, false)
        .await
        .expect("untagged plan-like text should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "untagged plan-like text must schedule bounded repair, not end the turn"
    );
    assert!(
        ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("Repair the listed defects")
                && message.content.as_text().contains(CANONICAL_STEP_FORMAT)
        }),
        "the repair directive should teach the canonical step contract"
    );
}
#[tokio::test]
async fn untagged_planlike_text_with_exhausted_budget_rejects_with_specific_reasons() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.mark_plan_validation_repair_used();
    ctx.plan_session.mark_plan_validation_repair_used();

    let outcome = ctx
        .handle_text_response(UNTAGGED_PLANLIKE_TEXT.to_string(), Vec::new(), None, None, false)
        .await
        .expect("untagged plan-like text should be handled");

    assert_terminal_plan_rejection_is_blocked(&outcome);
    assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
}
#[tokio::test]
async fn untagged_valid_plan_promotes_to_approval_handoff() {
    // A tagless draft that already validates must not end with the generic
    // planless hint; it promotes to a plan candidate and reaches the same
    // persist/approval handoff as a tagged draft.
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(EXECUTION_REVISION_PLAN.to_string(), Vec::new(), None, None, false)
        .await
        .expect("valid untagged plan should be handled");

    assert!(
        matches!(
            outcome,
            TurnHandlerOutcome::BreakWithPolicy {
                result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                ..
            }
        ),
        "valid untagged plan must reach the approval handoff"
    );
    assert!(
        !ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains("no approval-ready plan was produced")
        }),
        "a promoted plan must not store the planless hint"
    );
}
