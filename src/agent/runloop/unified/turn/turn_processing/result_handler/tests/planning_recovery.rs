use super::*;

#[tokio::test]
async fn planning_tokens_without_content_salvage_arms_one_final_content_retry() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;

    // The synthesis pass itself records completion tokens but no final
    // content (reasoning-interlaced end). Bounded salvage must arm instead of
    // parking the turn in a blocked handoff.
    let third = handle_planning_empty_with(
        &mut backing,
        NoOutputFacts { completion_tokens: 477, ..NoOutputFacts::default() },
        3,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
    )
    .await;
    assert!(matches!(third, TurnHandlerOutcome::Continue));
    assert!(
        backing
            .last_history_message_contains("recorded 477 completion tokens but returned no extractable final content"),
        "salvage directive must carry the captured token count, not an unverified emptiness claim"
    );
}
#[tokio::test]
async fn planning_third_tokens_without_content_handoff_states_captured_facts() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    for request in 1..=3 {
        let outcome = handle_planning_empty_with(
            &mut backing,
            NoOutputFacts { completion_tokens: 477, ..NoOutputFacts::default() },
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
    let notices = diagnostics.lock().expect("diagnostics");
    let notice = notices
        .iter()
        .find(|notice| notice.code == "planning.awaiting_input")
        .expect("canonical awaiting-input diagnostic");
    let details: serde_json::Value = serde_json::from_str(&notice.agent_details).expect("diagnostic details");
    assert_eq!(details["requests_used"], 3);
    assert_eq!(details["retained_draft"], false, "an empty response cannot create a saved draft");
    assert!(
        details["failure"]
            .as_str()
            .unwrap()
            .contains("provider recorded 477 completion tokens")
    );
    assert_eq!(notices.iter().filter(|notice| notice.code == "planning.awaiting_input").count(), 1);
    assert!(backing.last_history_message_contains("Planning needs your input. Saved progress is available."));
}
#[tokio::test]
async fn planning_true_no_output_handoff_never_claims_returned_tokens() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
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
            assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
        }
    }
    let notices = diagnostics.lock().expect("diagnostics");
    let notice = notices
        .iter()
        .find(|notice| notice.code == "planning.awaiting_input")
        .expect("canonical awaiting-input diagnostic");
    let details: serde_json::Value = serde_json::from_str(&notice.agent_details).expect("diagnostic details");
    assert_eq!(details["category"], "empty_content");
    assert!(
        details["failure"]
            .as_str()
            .unwrap()
            .contains("the model returned an empty synthesis response")
    );
    assert!(!details["failure"].as_str().unwrap().contains("completion tokens"));
    assert!(!backing.last_history_message_contains("completion tokens"));
}
#[tokio::test]
async fn planning_dropped_tool_calls_report_the_actual_failure_shape() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    for request in 1..=3 {
        let facts = NoOutputFacts {
            dropped_tool_calls: true,
            reasoning_only: true,
            completion_tokens: 210,
            ..NoOutputFacts::default()
        };
        let outcome = handle_planning_empty_with(
            &mut backing,
            facts,
            request + 2,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
        )
        .await;
        if request < 3 {
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        } else {
            assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
        }
    }
    let notices = diagnostics.lock().expect("diagnostics");
    let notice = notices
        .iter()
        .find(|notice| notice.code == "planning.awaiting_input")
        .expect("canonical awaiting-input diagnostic");
    let details: serde_json::Value = serde_json::from_str(&notice.agent_details).expect("diagnostic details");
    assert_eq!(details["category"], "native_tool_calls");
    assert!(
        details["failure"]
            .as_str()
            .unwrap()
            .contains("attempted native tool calls on a tool-free pass")
    );
    assert!(!details["failure"].as_str().unwrap().contains("empty synthesis response"));
    assert!(!notice.agent_details.contains("private planning reasoning"));
    assert!(turn_modified_files.is_empty());
}

fn session_tool_call_only_facts() -> NoOutputFacts {
    // Session session-vtcode-20261111T131103Z_367979-86194: each tool-free
    // synthesis recorded ~57 completion tokens that were a native tool call
    // with no plan prose, so the harness voided the whole response.
    NoOutputFacts {
        dropped_tool_calls: true,
        completion_tokens: 57,
        ..NoOutputFacts::default()
    }
}

async fn drive_budget_exhausted_planning_cycle(
    backing: &mut TestTurnProcessingBacking,
    facts: NoOutputFacts,
    step_count: usize,
    repeated_tool_attempts: &mut LoopTracker,
    turn_modified_files: &mut BTreeSet<std::path::PathBuf>,
) -> TurnHandlerOutcome {
    // Research spent the turn budget, so each explicit cycle admits exactly
    // one synthesis request and a failure parks in the awaiting-input handoff.
    handle_planning_empty_with(backing, facts, step_count, repeated_tool_attempts, turn_modified_files).await
}

#[tokio::test]
async fn planning_repeated_identical_failures_flag_and_vary_the_handoff() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    backing.turn_processing_context().plan_session.mark_budget_exhausted();

    // Cycle 0: first tool-call-only synthesis → standard handoff.
    let first = drive_budget_exhausted_planning_cycle(
        &mut backing,
        session_tool_call_only_facts(),
        3,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
    )
    .await;
    assert!(
        matches!(first, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) if reason == "planning awaiting input")
    );
    assert!(!backing.last_history_message_contains("failed identically"));

    // Explicit user retry → cycle 1 admits one more request, which fails the
    // same way. The retry must carry a correction naming the last failure,
    // and the handoff must say the failure repeated instead of offering a
    // blind retry that cannot work.
    backing.turn_processing_context().plan_session.begin_explicit_recovery_cycle();
    let second = drive_budget_exhausted_planning_cycle(
        &mut backing,
        session_tool_call_only_facts(),
        4,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
    )
    .await;
    assert!(
        matches!(second, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) if reason == "planning awaiting input")
    );

    let notices = diagnostics.lock().expect("diagnostics");
    let await_inputs: Vec<_> = notices
        .iter()
        .filter(|notice| notice.code == "planning.awaiting_input")
        .collect();
    assert_eq!(await_inputs.len(), 2);
    let first_details: serde_json::Value =
        serde_json::from_str(&await_inputs[0].agent_details).expect("cycle-0 details");
    let second_details: serde_json::Value =
        serde_json::from_str(&await_inputs[1].agent_details).expect("cycle-1 details");
    assert_eq!(first_details["category"], "native_tool_calls");
    assert_eq!(first_details["cycle"], 0);
    assert_eq!(first_details["requests_used"], 1);
    assert_eq!(second_details["category"], "native_tool_calls");
    assert_eq!(second_details["cycle"], 1);
    assert_eq!(second_details["requests_used"], 1);
    assert_eq!(first_details["repeated_identical_failure"], false);
    assert_eq!(second_details["repeated_identical_failure"], true);
    assert_eq!(await_inputs[0].actions[0].label.as_str(), "Retry from retained evidence");
    assert_eq!(await_inputs[1].actions[0].label.as_str(), "Retry once anyway");
    assert!(backing.last_history_message_contains("failed identically"));
    assert!(
        backing.history_contains("Planning retry correction"),
        "explicit retry must name the last failure instead of resending an identical request"
    );
}

#[tokio::test]
async fn planning_differing_failure_class_does_not_flag_repetition() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    backing.turn_processing_context().plan_session.mark_budget_exhausted();

    let first = drive_budget_exhausted_planning_cycle(
        &mut backing,
        session_tool_call_only_facts(),
        3,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
    )
    .await;
    assert!(matches!(first, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));

    // Asymmetric control: the retry fails differently (genuinely empty), so
    // the handoff must stay standard — the detector compares classes instead
    // of flagging every second-cycle failure.
    backing.turn_processing_context().plan_session.begin_explicit_recovery_cycle();
    let second = drive_budget_exhausted_planning_cycle(
        &mut backing,
        NoOutputFacts::default(),
        4,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
    )
    .await;
    assert!(matches!(second, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));

    let notices = diagnostics.lock().expect("diagnostics");
    let await_inputs: Vec<_> = notices
        .iter()
        .filter(|notice| notice.code == "planning.awaiting_input")
        .collect();
    assert_eq!(await_inputs.len(), 2);
    let second_details: serde_json::Value =
        serde_json::from_str(&await_inputs[1].agent_details).expect("cycle-1 details");
    assert_eq!(second_details["category"], "empty_content");
    assert_eq!(second_details["repeated_identical_failure"], false);
    assert!(!backing.last_history_message_contains("failed identically"));
}

/// Ready plan shaped like the canonical streamed fixture: all four sections
/// with file and verify evidence on the step.
const SALVAGEABLE_PLAN_PROSE: &str = "# Salvaged plan\n\n## Summary\nRestore the README overview.\n\n## Implementation Steps\n1. Rewrite the overview -> files: [README.md] -> verify: [cargo nextest run -p vtcode]\n\n## Test Cases and Validation\n1. Run the docs link check.\n\n## Assumptions and Defaults\n1. Keep the existing structure.\n";

#[tokio::test]
async fn planning_violated_synthesis_with_ready_plan_salvages_to_repair() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;

    // Tool-free synthesis mixed a ready plan with forbidden tool calls. The
    // calls stay dropped, but the plan must enter bounded validation-repair
    // (clean text-only re-emit) instead of an immediate handoff.
    let facts = NoOutputFacts {
        dropped_tool_calls: true,
        completion_tokens: 340,
        voided_text: Some(SALVAGEABLE_PLAN_PROSE.to_string()),
        ..NoOutputFacts::default()
    };
    let outcome =
        handle_planning_empty_with(&mut backing, facts, 3, &mut repeated_tool_attempts, &mut turn_modified_files).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(
        backing.history_contains("Re-emit the same plan as text only"),
        "ready violated plan must schedule a clean re-emit, not a generic correction"
    );
}

#[tokio::test]
async fn planning_violated_synthesis_with_junk_prose_keeps_the_handoff() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;

    // Asymmetric control: non-plan prose alongside the violation must not
    // consume validation-repair budget — the generic correction path applies.
    let facts = NoOutputFacts {
        dropped_tool_calls: true,
        completion_tokens: 120,
        voided_text: Some("Let me check a few more files first.".to_string()),
        ..NoOutputFacts::default()
    };
    let outcome =
        handle_planning_empty_with(&mut backing, facts, 3, &mut repeated_tool_attempts, &mut turn_modified_files).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!backing.history_contains("Re-emit the same plan as text only"));
    assert!(backing.history_contains("Planning correction: repair the retained draft"));
}
