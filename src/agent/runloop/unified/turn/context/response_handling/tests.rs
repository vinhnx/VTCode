use super::*;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use vtcode_core::tools::handlers::planning_workflow::CANONICAL_STEP_FORMAT;

fn last_final_answer_text(ctx: &TurnProcessingContext<'_>) -> String {
    ctx.working_history
        .iter()
        .rev()
        .find(|message| {
            message.role == uni::MessageRole::Assistant && message.phase == Some(uni::AssistantPhase::FinalAnswer)
        })
        .map(|message| message.content.as_text().into_owned())
        .expect("terminal rejection must publish a final assistant message")
}

fn assert_history_carries_validator_feedback(final_text: &str) {
    assert!(
        final_text.contains("Plan validation issues:") && final_text.contains(CANONICAL_STEP_FORMAT),
        "history must carry validator feedback and canonical step format, got: {final_text}"
    );
}

fn assert_terminal_plan_rejection_is_blocked(outcome: &TurnHandlerOutcome) {
    let TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) = outcome else {
        panic!("a rejected plan must end blocked instead of reporting completion");
    };
    assert_eq!(reason, "plan artifact rejected; revise the plan before continuing");
    assert!(!tracker_auto_continue_is_recoverable_block(Some(reason)));
}

/// Unparseable pseudo-tool-call markup: the `<tools:call>` name is empty,
/// so every textual parser rejects it, but the pseudo-marker scan still
/// sees `<tool_call`. Mirrors the raw-XML leak from turn_887/turn_888.
const BROKEN_MARKUP_RESPONSE: &str =
    "I need to inspect the workspace.\n<tool_call>\n<tools:call name=\"\">\n</tools:call>\n</tool_call>";

#[tokio::test]
async fn plan_mode_pseudo_tool_call_markup_is_stripped_and_reprompts_once() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "plan-mode pseudo-tool-call markup should re-prompt instead of ending the turn"
    );

    let assistant_texts: Vec<String> = ctx
        .working_history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().into_owned())
        .collect();
    assert!(
        assistant_texts
            .iter()
            .any(|text| text.contains("I need to inspect the workspace.")),
        "the prose part of the response should be preserved: {assistant_texts:?}"
    );
    assert!(
        assistant_texts.iter().all(|text| !text.contains("<tool_call")),
        "raw tool-call markup must never be stored in history: {assistant_texts:?}"
    );

    let directive_present = ctx
        .working_history
        .iter()
        .any(|message| message.role == uni::MessageRole::System && message.content.as_text().contains("not executed"));
    assert!(directive_present, "a re-prompt directive should be pushed into history");
}

#[tokio::test]
async fn plan_mode_pseudo_tool_call_reprompt_is_bounded() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    for _ in 0..crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS {
        ctx.plan_session.mark_plan_pseudo_tool_call_reprompt_used();
    }

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(_)),
        "an exhausted reprompt budget must end the turn instead of looping"
    );
    let assistant_texts: Vec<String> = ctx
        .working_history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().into_owned())
        .collect();
    assert!(
        assistant_texts.iter().all(|text| !text.contains("<tool_call")),
        "even with the budget exhausted, raw markup must be stripped from the final answer: {assistant_texts:?}"
    );
}

#[tokio::test]
async fn build_mode_pseudo_tool_call_markup_keeps_existing_behavior() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(_)),
        "outside planning, text responses still end the turn (no new reprompt path)"
    );
}

const RECOVERY_VALID_PLAN: &str = r#"# Recovery plan

## Summary
Fix the read-cap recovery path from gathered evidence.

## Implementation Steps
1. Reorder the guard -> files: [src/agent/runloop/unified/turn/tool_outcomes/handlers/guards/read_guard.rs] -> verify: [cargo check --locked]

## Test Cases and Validation
1. Run cargo check --locked.

## Assumptions and Defaults
1. Keep execution-mode caps strict.
"#;

const RECOVERY_INVALID_PLAN: &str = r#"# Recovery plan

## Summary
Fix the read-cap recovery path from gathered evidence.

## Implementation Steps
1. Reorder the guard

## Test Cases and Validation
1. Run the focused test.

## Assumptions and Defaults
1. Keep existing behavior.
"#;

/// Regression fixture from session-vtcode-20260916T033857Z_346951-33962:
/// the plan has otherwise concrete steps, but more than one verification
/// item is rejected by the validator. The repair must see the complete
/// bounded draft and validator-owned feedback before it gets a corrected
/// plan.
const RECOVERY_INVALID_VERIFICATION_PLAN: &str = r#"# Preview recovery plan

## Summary
Repair planning recovery from the evidence already gathered.

## Implementation Steps
1. Update the preview guard -> files: [src/agent/runloop/unified/turn/guards.rs] -> verify: [run checks]
2. Strengthen synthesis guidance -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo check --locked]
3. Preserve validator feedback -> files: [src/agent/runloop/unified/turn/context/response_handling.rs] -> verify: [rg -n verify src/agent/runloop/unified/turn]
4. Keep tools disabled during repair -> files: [src/agent/runloop/unified/turn/turn_processing/result_handler.rs] -> verify: [cargo nextest run -p vtcode]
5. Add the transition regression -> files: [src/agent/runloop/unified/turn/turn_loop/tests.rs] -> verify: [git diff --check]

## Test Cases and Validation
1. Run the focused planning recovery test.

## Assumptions and Defaults
1. Keep strict plan validation and bounded recovery.
"#;

const RECOVERY_CORRECTED_VERIFICATION_PLAN: &str = r#"# Preview recovery plan

## Summary
Repair planning recovery from the evidence already gathered with concrete verification.

## Implementation Steps
1. Update the preview guard -> files: [src/agent/runloop/unified/turn/guards.rs] -> verify: [cargo nextest run -p vtcode]
2. Strengthen synthesis guidance -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo check --locked]
3. Preserve validator feedback -> files: [src/agent/runloop/unified/turn/context/response_handling.rs] -> verify: [rg -n verify src/agent/runloop/unified/turn]
4. Keep tools disabled during repair -> files: [src/agent/runloop/unified/turn/turn_processing/result_handler.rs] -> verify: [cargo nextest run -p vtcode]
5. Add the transition regression -> files: [src/agent/runloop/unified/turn/turn_loop/tests.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run cargo nextest run -p vtcode.

## Assumptions and Defaults
1. Keep strict plan validation and bounded recovery.
"#;

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

/// Tagless markdown plan in the turn_1075 shape: numbered steps but no
/// `<proposed_plan>` tags, no `files:` targets, and no `verify:` checks.
const UNTAGGED_PLANLIKE_TEXT: &str = "## Simple plan to improve startup\n\n1. Measure the current startup path\n2. Identify the slowest steps\n3. Defer non-critical work\n";

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

/// A valid revision of an already-approved plan, in the canonical section
/// shape the artifact validator requires.
const EXECUTION_REVISION_PLAN: &str = r#"# Revised plan

## Summary
Repairs the approved plan after the referenced paths moved.

## Implementation Steps
1. Update the module path -> files: [src/lib.rs] -> verify: [cargo check]

## Test Cases and Validation
1. Run cargo check --locked.

## Assumptions and Defaults
1. Keep the approved scope.
"#;

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
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_approved_plan_execution_for_test(true);
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
}

#[tokio::test]
async fn denied_interview_without_ready_plan_replaces_prose_with_hint() {
    // When request_user_input is permanently denied (non-interactive
    // runtime), no plan was proposed, no plan is persisted, and the model
    // emits research prose (not a clarifying question), the prose must be
    // replaced with the no-approval-ready-plan hint so the user gets an
    // actionable message instead of rambling text.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.mark_interview_denied();

    let outcome = ctx
        .handle_text_response(
            "I looked at the codebase and found several files.".to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("text response should be handled");

    // The first denied-interview response gets a bounded synthesis retry
    // (plan_synthesis_retry_allowed is true on the first attempt).
    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "first denied-interview response without a plan should retry synthesis"
    );
    let directive_present = ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message
                .content
                .as_text()
                .contains(DENIED_INTERVIEW_PLAN_SYNTHESIS_RETRY_DIRECTIVE)
    });
    assert!(directive_present, "a plan-synthesis retry directive should be pushed");
}

#[tokio::test]
async fn denied_interview_exhausted_retry_ends_with_hint_not_prose() {
    // After the bounded retry is exhausted, a second prose response must
    // end the turn with the no-approval-ready-plan hint, not the model's
    // research prose.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.mark_interview_denied();
    ctx.plan_session.mark_plan_synthesis_retry_used();

    let outcome = ctx
        .handle_text_response("I found more files to examine.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(_)),
        "after retry exhaustion, prose without a plan should end the turn"
    );
    let last = ctx
        .working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("an assistant message must be pushed");
    let text = last.content.as_text();
    assert!(
        text.contains("no approval-ready plan was produced"),
        "the no-approval-ready hint must replace prose: {text}"
    );
    assert!(
        !text.contains("I found more files"),
        "the model's research prose must NOT be the final answer: {text}"
    );
}

#[tokio::test]
async fn denied_interview_preserves_clarifying_question() {
    // A clarifying question (text ending with '?') is the text-mode
    // equivalent of the unavailable interview modal. It must NOT be
    // replaced with the hint or trigger a synthesis retry — the turn ends
    // so the user can answer it (checkpoint turn_856).
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.mark_interview_denied();

    let outcome = ctx
        .handle_text_response(
            "Should I focus on the launch path or the config loading?".to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(_)),
        "a clarifying question should end the turn for user input, not retry"
    );
    let last = ctx
        .working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("an assistant message must be pushed");
    let text = last.content.as_text();
    assert!(
        text.contains("Should I focus on the launch path or the config loading?"),
        "the clarifying question must be preserved verbatim: {text}"
    );
    assert!(
        !text.contains("no approval-ready plan was produced"),
        "the hint must NOT replace a clarifying question: {text}"
    );
}

#[tokio::test]
async fn planless_planning_turn_ends_with_hint_in_history() {
    // Checkpoint turn_912: after the validation repair was consumed, the
    // model answered with a planless status echo and the turn closed with
    // no visible next step. The assistant's stored answer must carry the
    // no-approval-ready hint so the user knows planning is still active.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.mark_plan_validation_repair_used();

    let outcome = ctx
        .handle_text_response(
            "Planning workflow is active with read-only permissions.".to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("text response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(_)), "a planless planning response should end the turn");
    let last = ctx
        .working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("an assistant message must be pushed");
    let text = last.content.as_text();
    assert!(
        text.contains("no approval-ready plan was produced"),
        "the no-approval-ready hint must accompany the planless answer: {text}"
    );
    assert!(
        text.contains("Planning workflow is active with read-only permissions."),
        "the model's own text must be preserved, not replaced: {text}"
    );
}

#[tokio::test]
async fn planless_planning_clarifying_question_ends_without_hint() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(
            "Should I focus on the runtime loop or startup?".to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("text response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(_)));
    let last = ctx
        .working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("an assistant message must be pushed");
    let text = last.content.as_text();
    assert!(
        !text.contains("no approval-ready plan was produced"),
        "a clarifying question must not gain the hint: {text}"
    );
}

#[tokio::test]
async fn planless_planning_hint_is_not_stored_when_turn_continues() {
    // Regression: the hint must be deferred until terminality is known.
    // A pseudo-tool-call response that triggers a bounded reprompt returns
    // `Continue` — storing the hint before that decision would leave stale
    // "no approval-ready plan" guidance in history for a turn that kept
    // going.
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "pseudo-tool-call markup should re-prompt (Continue), not end the turn"
    );
    let assistant_texts: Vec<String> = ctx
        .working_history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().into_owned())
        .collect();
    assert!(
        assistant_texts
            .iter()
            .all(|text| !text.contains("no approval-ready plan was produced")),
        "a continued turn must never store the terminal hint: {assistant_texts:?}"
    );
}

#[test]
fn no_ready_plan_hint_predicate_gates_on_denied_interview_and_questions() {
    assert!(should_render_no_ready_plan_hint(false, "Here is a research summary."));
    assert!(!should_render_no_ready_plan_hint(true, "Here is a research summary."));
    assert!(!should_render_no_ready_plan_hint(false, "Which approach should I take?"));
}

#[test]
fn attempted_plan_detection_catches_untagged_turn_1075_shape() {
    // Regression for checkpoint turn_1075: a markdown plan with numbered
    // steps but no `<proposed_plan>` tags must route to bounded repair,
    // not end with only the generic no-ready-plan hint.
    let tagless = "## Simple plan to improve startup\n\n1. Measure the current startup path\n2. Identify the slowest steps\n3. Defer non-critical work\n";
    assert!(looks_like_attempted_plan(tagless));

    // Asymmetric counterpart: a single numbered line in research prose
    // is not an attempted plan.
    assert!(!looks_like_attempted_plan("Found one candidate:\n1. src/main.rs looks relevant."));
    assert!(!looks_like_attempted_plan("Here is a quick update: I searched the codebase."));
    assert!(!looks_like_attempted_plan(""));
    assert!(!looks_like_attempted_plan("## 計画計画計画計画計画計画計画計画"));
}

#[test]
fn attempted_plan_detection_catches_goal_plus_plan_headings() {
    // Checkpoint turn_1074 used `## Goal` / `## Plan` instead of the
    // canonical headings; that shape must also route to repair.
    let goal_plan = "## Goal\nImprove startup.\n\n## Plan\n1. Measure timing\n2. Defer work\n";
    assert!(looks_like_attempted_plan(goal_plan));

    let implementation = "## Implementation Steps\n1. Measure timing\n";
    assert!(looks_like_attempted_plan(implementation));
}

#[test]
fn attempted_plan_detection_rejects_prose_and_non_plan_headings() {
    // Prose mentioning the phrase is research, not an attempted plan.
    assert!(!looks_like_attempted_plan("The implementation steps are unclear; need to research more."));
    // `## Planning` is not a `## Plan` heading (word boundary).
    assert!(!looks_like_attempted_plan(
        "## Planning status\nGoal is to improve startup.\n1. Found one candidate.\n"
    ));
    // A single goal heading without a plan heading is not an attempt.
    assert!(!looks_like_attempted_plan("## Goal\nImprove startup.\n"));
}

#[test]
fn attempted_plan_detection_catches_step_prefixed_numbering() {
    // Mirrors `numbered_line_parts`: `Step 1:` counts as a numbered step.
    let stepped = "Step 1: Measure timing\nStep 2: Defer work\n";
    assert!(looks_like_attempted_plan(stepped));
    assert!(!looks_like_attempted_plan("Stepwise refinement is needed."));
}

#[test]
fn clarifying_question_detection_keeps_formatting_edge_cases_explicit() {
    let structured_plan = "## Assumptions\n1. Preserve the quoted requirement: \"Should we keep this?\"";
    assert!(!looks_like_clarifying_question(structured_plan));

    let quoted_question = "The requirement is \"Should we keep this?\"";
    assert!(!looks_like_clarifying_question(quoted_question));

    // Keep the current terminal-line heuristic documented until a corpus
    // of production false positives justifies a more semantic classifier.
    assert!(looks_like_clarifying_question("This is rhetorical: why change it?"));
}

#[test]
fn rejected_plan_draft_is_reattached_to_last_assistant_message() {
    let mut history = vec![
        uni::Message::user("make a plan".to_string()),
        uni::Message::assistant("• Planning workflow is active.".to_string()),
    ];
    append_rejected_plan_draft_to_last_assistant(&mut history, "## Summary\nDo the thing");
    let last = history.last().expect("assistant message");
    let text = last.content.as_text();
    assert!(
        text.contains("Planning workflow is active")
            && text.contains("<proposed_plan>\n## Summary\nDo the thing\n</proposed_plan>"),
        "draft must be appended, not replace the stored text: {text:?}"
    );
    assert_eq!(history.len(), 2, "no new message should be pushed");
}

#[test]
fn rejected_plan_draft_replaces_empty_assistant_text_and_bounds_huge_drafts() {
    let mut history = vec![uni::Message::assistant(String::new())];
    append_rejected_plan_draft_to_last_assistant(&mut history, "  ## Summary\nOnly draft  ");
    let text = history[0].content.as_text();
    assert_eq!(text, "<proposed_plan>\n## Summary\nOnly draft\n</proposed_plan>");

    let huge = "x".repeat(REJECTED_PLAN_DRAFT_HISTORY_BUDGET * 2);
    let mut history = vec![uni::Message::assistant(String::new())];
    append_rejected_plan_draft_to_last_assistant(&mut history, &huge);
    let text = history[0].content.as_text();
    assert!(text.len() < REJECTED_PLAN_DRAFT_HISTORY_BUDGET + 64);
    assert!(text.contains("…[truncated]"));

    // A non-streaming extracted plan may have no assistant message yet;
    // preserve it as a bounded commentary message for the repair pass.
    let mut history = vec![uni::Message::user("hi".to_string())];
    append_rejected_plan_draft_to_last_assistant(&mut history, "draft");
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].role, uni::MessageRole::Assistant);
    assert_eq!(history[1].phase, Some(uni::AssistantPhase::Commentary));
    assert_eq!(history[1].content.as_text(), "<proposed_plan>\ndraft\n</proposed_plan>");
    // Empty draft → no-op.
    let mut history = vec![uni::Message::assistant("kept".to_string())];
    append_rejected_plan_draft_to_last_assistant(&mut history, "   ");
    assert_eq!(history[0].content.as_text(), "kept");
}

#[tokio::test]
async fn pseudo_reprompt_queues_bounded_cap_allowance() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    assert!(!ctx.plan_session.bounded_planning_follow_up_allowed());

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");
    assert!(matches!(outcome, TurnHandlerOutcome::Continue), "pseudo markup should schedule a bounded reprompt");
    assert!(
        ctx.plan_session.bounded_planning_follow_up_allowed(),
        "the scheduled reprompt must queue one cap allowance so the next request is not immediately blocked"
    );
}

#[tokio::test]
async fn denied_interview_retry_queues_bounded_cap_allowance() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let mut ctx = backing.turn_processing_context();
    assert!(!ctx.plan_session.bounded_planning_follow_up_allowed());

    let outcome = ctx
        .handle_text_response("Here is a research summary.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");
    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "denied-interview prose should schedule a bounded synthesis retry"
    );
    assert!(
        ctx.plan_session.bounded_planning_follow_up_allowed(),
        "the synthesis retry must queue one cap allowance"
    );
}

// Captured final draft from session 20261015T161453Z: the interview fallback
// consumed this invalid tagless plan before validation could repair it.
const SESSION_161453_PLAN: &str = include_str!("tests/session_161453_untagged_plan.txt");

fn process_planning_candidate(ctx: &mut TurnProcessingContext<'_>, draft: &str) -> TurnProcessingResult {
    use crate::agent::runloop::unified::turn::turn_processing::{
        maybe_force_planning_workflow_interview, process_llm_response,
    };
    let response = uni::LLMResponse::new("test", draft);
    let processed = process_llm_response(&response, ctx.renderer, 37, true, true, true, true, None, None)
        .expect("process planning response");
    maybe_force_planning_workflow_interview(processed, Some(draft), ctx.session_stats, ctx.plan_session, 37)
}

#[tokio::test]
async fn session_161453_draft_reaches_repair_then_persisted_approval_request() {
    use vtcode_ui::tui::app::{
        InlineCommand, InlineEvent, InlineHandle, InlineListSelection, InlineSession, TransientEvent, TransientRequest,
    };
    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let mut session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
        worker: None,
    };
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    // Dismiss only after the runtime has emitted the approval overlay.
    event_tx.send(InlineEvent::Transient(TransientEvent::Cancelled)).unwrap();
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.set_skip_confirmations_for_test(false);
    let events_path = backing.enable_harness_emitter();
    let mut ctx = backing.turn_processing_context();
    ctx.handle = &handle;
    ctx.renderer = &mut renderer;
    ctx.session = &mut session;
    ctx.session_stats.record_tool(vtcode_core::config::constants::tools::READ_FILE);
    ctx.plan_session.increment_turns();

    let TurnProcessingResult::TextResponse { text, proposed_plan, .. } =
        process_planning_candidate(&mut ctx, SESSION_161453_PLAN)
    else {
        panic!("a draft must reach validation instead of a fabricated interview");
    };
    assert_eq!(proposed_plan.as_deref(), Some(SESSION_161453_PLAN.trim()));
    assert!(!ctx.plan_session.interview_shown());
    assert!(!validate_plan_content(SESSION_161453_PLAN).is_ready());
    let result = ctx
        .handle_text_response(text, Vec::new(), None, proposed_plan, false)
        .await
        .expect("repair draft");
    assert!(matches!(result, TurnHandlerOutcome::Continue));
    assert!(!persisted_plan_is_ready(&ctx.tool_registry.planning_workflow_state()).await);
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.role == uni::MessageRole::System
                && message.content.as_text().contains("Repair the listed defects")
                && message.content.as_text().contains(CANONICAL_STEP_FORMAT))
    );

    let repaired = "## Summary\nRefine README prose.\n\n## Implementation Steps\n1. Tighten README wording -> files: [README.md] -> verify: [npx markdownlint-cli2 README.md]\n\n## Test Cases and Validation\n- Markdown lint exits zero; review links and the contributor block.\n\n## Assumptions and Defaults\n- Preserve commands and generated content.\n";
    let TurnProcessingResult::TextResponse { text, proposed_plan, .. } = process_planning_candidate(&mut ctx, repaired)
    else {
        panic!("repaired plan must reach the approval gate");
    };
    let result = ctx
        .handle_text_response(text, Vec::new(), None, proposed_plan, false)
        .await
        .expect("persist repaired plan");
    assert!(matches!(
        result,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
    ));
    assert!(persisted_plan_is_ready(&ctx.tool_registry.planning_workflow_state()).await);
    assert!(last_final_answer_text(&ctx).contains("Plan review ended without starting implementation"));
    assert!(ctx.tool_registry.planning_workflow_state().is_active());
    let events = tokio::fs::read_to_string(events_path).await.expect("approval events");
    assert_eq!(events.matches("\"type\":\"plan.approval.requested\"").count(), 1);
    assert_eq!(events.matches("\"type\":\"plan.approval.resolved\"").count(), 1);
    assert!(!events.contains("auto_accept"));
    let mut approval_overlays = 0;
    while let Ok(command) = command_rx.try_recv() {
        if let InlineCommand::ShowTransient { request } = command {
            let TransientRequest::List(list) = *request else {
                panic!("expected approval list");
            };
            assert!(
                list.items
                    .iter()
                    .any(|item| item.selection == Some(InlineListSelection::PlanApprovalExecute))
            );
            approval_overlays += 1;
        }
    }
    assert_eq!(approval_overlays, 1, "the repaired draft must show exactly one HITL approval overlay");
}

#[tokio::test]
async fn session_161453_repeated_invalid_draft_stops_with_validator_feedback() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    ctx.session_stats.record_tool(vtcode_core::config::constants::tools::READ_FILE);
    ctx.plan_session.increment_turns();
    for attempt in 0..=crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_VALIDATION_REPAIR_REPROMPTS {
        let TurnProcessingResult::TextResponse { text, proposed_plan, .. } =
            process_planning_candidate(&mut ctx, SESSION_161453_PLAN)
        else {
            panic!("draft was replaced with interview");
        };
        let result = ctx
            .handle_text_response(text, Vec::new(), None, proposed_plan, false)
            .await
            .expect("bounded repair");
        if attempt < crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_VALIDATION_REPAIR_REPROMPTS {
            assert!(matches!(result, TurnHandlerOutcome::Continue));
        } else {
            assert_terminal_plan_rejection_is_blocked(&result);
            assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
        }
    }
    assert!(!persisted_plan_is_ready(&ctx.tool_registry.planning_workflow_state()).await);
}
