use super::*;
use vtcode_core::llm::provider::{FinishReason, LLMResponse};

const DRAFT: &str = "## Summary\nRepair the parser.\n## Implementation Steps\n1. Validate input -> files: [src/parser.rs] -> verify: [cargo check --locked]\n## Test Cases and Validation\n- cargo check --locked exits 0.\n## Assumptions and Defaults\n- Preserve the API.";

#[test]
fn recovery_allowance_survives_checkpoint_and_mixed_failure_categories() {
    let mut snapshot = PlanningRecoverySnapshot::default();
    close_research(&mut snapshot, &[uni::Message::user("repair parser".into())], "ollama", "minimax-m3.1-flash:cloud");
    for class in [
        PlanningResponseClass::EmptyContent,
        PlanningResponseClass::NativeToolCalls,
        PlanningResponseClass::MalformedPlan,
    ] {
        assert!(admit_request(&mut snapshot));
        snapshot.response_class = Some(class);
        // A process resume gets exactly the consumed state, never a new allowance.
        snapshot = serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        close_research(&mut snapshot, &[uni::Message::user("automatic continuation".into())], "other", "other");
        assert!(snapshot.research_closed);
        assert_eq!(snapshot.latest_request, "repair parser");
    }
    assert_eq!(snapshot.requests_used, 3);
    assert!(!admit_request(&mut snapshot));
    begin_explicit_cycle(&mut snapshot);
    assert!(snapshot.research_closed);
    assert!(admit_request(&mut snapshot));
    assert_eq!(snapshot.cycle, 1);
}

#[test]
fn approval_prose_never_replaces_best_draft_or_changes_readiness() {
    let mut snapshot = PlanningRecoverySnapshot::default();
    retain_draft(&mut snapshot, DRAFT);
    retain_draft(&mut snapshot, "The plan is ready for approval. Approve and I'll implement it.");
    assert_eq!(snapshot.best_draft.as_deref(), Some(DRAFT));
    assert_ne!(snapshot.outcome, PlanningOutcome::AwaitingApproval);
}

#[test]
fn clarified_request_prefers_new_draft_without_discarding_previous_progress() {
    let revised_draft = "## Summary\nRepair the endpoint.\n## Implementation Steps\n1. Update endpoint";
    let mut snapshot = PlanningRecoverySnapshot::default();
    retain_draft(&mut snapshot, DRAFT);
    record_clarification(&mut snapshot, "  ", false);
    retain_draft(&mut snapshot, revised_draft);
    assert_eq!(snapshot.best_draft.as_deref(), Some(DRAFT));

    record_clarification(&mut snapshot, "Plan the endpoint instead", false);
    assert_eq!(snapshot.best_draft.as_deref(), Some(DRAFT));
    retain_draft(&mut snapshot, revised_draft);
    assert_eq!(snapshot.best_draft.as_deref(), Some(revised_draft));
    assert!(
        snapshot
            .validation_report
            .as_deref()
            .is_some_and(|report| !report.contains("retained draft validates"))
    );
    assert!(snapshot.best_draft_defects.is_some());
}

#[test]
fn closing_research_does_not_replace_current_draft_with_previous_task() {
    let current_draft = "## Summary\nRepair the current endpoint.\n## Implementation Steps\n1. Update endpoint";
    let history = vec![
        uni::Message::user("plan the parser".into()),
        uni::Message::assistant(DRAFT.into()),
        uni::Message::user("plan the endpoint".into()),
        uni::Message::assistant(current_draft.into()),
    ];
    let mut snapshot = PlanningRecoverySnapshot::default();
    retain_draft(&mut snapshot, current_draft);
    close_research(&mut snapshot, &history, "ollama", "test");
    assert_eq!(snapshot.best_draft.as_deref(), Some(current_draft));
    assert!(
        snapshot
            .validation_report
            .as_deref()
            .is_some_and(|report| !report.contains("retained draft validates"))
    );
}

#[test]
fn private_reasoning_is_classified_without_becoming_a_draft() {
    let response = LLMResponse {
        reasoning: Some(DRAFT.into()),
        ..Default::default()
    };
    assert_eq!(response_class(&response), PlanningResponseClass::ReasoningOnly);
    let mut snapshot = PlanningRecoverySnapshot::default();
    close_research(
        &mut snapshot,
        &[uni::Message::assistant(String::new()).with_reasoning(Some(DRAFT.into()))],
        "ollama",
        "test",
    );
    assert!(snapshot.best_draft.is_none());
    assert!(!serde_json::to_string(&snapshot).unwrap().contains("Repair the parser"));
    assert_eq!(
        response_class(&LLMResponse {
            content: Some(DRAFT.into()),
            finish_reason: FinishReason::Length,
            ..Default::default()
        }),
        PlanningResponseClass::Truncation
    );
    assert_eq!(
        response_class(&LLMResponse { content: Some(DRAFT.into()), ..Default::default() }),
        PlanningResponseClass::ValidPlan
    );
}

#[test]
fn retained_evidence_is_bounded_redacted_and_chronological() {
    let history = vec![
        uni::Message::user("repair".into()),
        uni::Message::tool_response("a".into(), "first: password=secret-value-1234".into()),
        uni::Message::tool_response("b".into(), "last evidence".into()),
    ];
    let mut snapshot = PlanningRecoverySnapshot::default();
    close_research(&mut snapshot, &history, "ollama", "test");
    assert_eq!(snapshot.evidence.len(), 2);
    assert!(snapshot.evidence[0].starts_with("first:"));
    assert_eq!(snapshot.evidence[1], "last evidence");
    assert!(!snapshot.evidence[0].contains("secret-value-1234"));
}
#[test]
fn legacy_resume_retains_progress_without_inventing_an_unused_allowance() {
    let history = vec![
        uni::Message::user("Plan the saved task".into()),
        uni::Message::tool_response("saved-read".into(), "Public saved evidence".into()),
    ];
    let mut snapshot = recover_legacy_session(&history, "ollama", "saved-model");
    assert!(snapshot.research_closed);
    assert_eq!(snapshot.outcome, PlanningOutcome::AwaitingInput);
    assert_eq!(snapshot.requests_used, 3);
    assert_eq!(snapshot.evidence, ["Public saved evidence"]);
    assert!(!snapshot.input_prompt_shown);
    assert!(!admit_request(&mut snapshot));
    begin_explicit_cycle(&mut snapshot);
    assert!(snapshot.research_closed && admit_request(&mut snapshot));
    assert_eq!(snapshot.requests_used, 1);
}

#[test]
fn explicit_research_merges_new_public_facts_without_resetting_recovery() {
    let mut snapshot = recover_legacy_session(
        &[uni::Message::tool_response("old".into(), "Saved earlier facts".into())],
        "ollama",
        "model",
    );
    begin_explicit_cycle(&mut snapshot);
    snapshot.outcome = PlanningOutcome::Researching;
    snapshot.requests_used = 1;
    close_research(
        &mut snapshot,
        &[uni::Message::tool_response(
            "new".into(),
            "New clarification facts".into(),
        )],
        "ollama",
        "model",
    );
    assert_eq!(snapshot.evidence, ["Saved earlier facts", "New clarification facts"]);
    assert_eq!(snapshot.requests_used, 1);
    assert_eq!(snapshot.outcome, PlanningOutcome::Synthesizing);
    assert!(snapshot.research_closed);
}

#[test]
fn newest_clarification_survives_a_full_request_context() {
    let mut snapshot = PlanningRecoverySnapshot {
        latest_request: "older evidence ".repeat(700),
        ..Default::default()
    };
    record_clarification(&mut snapshot, "Use the revised schema with the required new field.", false);
    assert!(snapshot.latest_request.len() <= 8192);
    assert!(
        snapshot
            .latest_request
            .ends_with("Clarification: Use the revised schema with the required new field.")
    );
    assert!(snapshot.targeted_research.is_none());
}

#[test]
fn recorded_classes_shift_so_repeats_compare_against_the_previous_shape() {
    let mut snapshot = PlanningRecoverySnapshot::default();
    assert!(!repeated_identical_failure(&snapshot));
    record_response_class(&mut snapshot, PlanningResponseClass::NativeToolCalls);
    assert!(!repeated_identical_failure(&snapshot));
    assert_eq!(snapshot.previous_response_class, None);
    record_response_class(&mut snapshot, PlanningResponseClass::NativeToolCalls);
    assert_eq!(snapshot.previous_response_class, Some(PlanningResponseClass::NativeToolCalls));
    assert!(repeated_identical_failure(&snapshot));
    // A differing shape breaks the streak without clearing history.
    record_response_class(&mut snapshot, PlanningResponseClass::EmptyContent);
    assert!(!repeated_identical_failure(&snapshot));
    assert_eq!(snapshot.response_class, Some(PlanningResponseClass::EmptyContent));
    assert_eq!(snapshot.previous_response_class, Some(PlanningResponseClass::NativeToolCalls));
    // Only failures repeat: consecutive valid plans are success, while a
    // clarifying question on a synthesis pass is still not a plan.
    assert!(is_failure_response_class(PlanningResponseClass::NativeToolCalls));
    assert!(is_failure_response_class(PlanningResponseClass::ClarifyingQuestion));
    assert!(!is_failure_response_class(PlanningResponseClass::ValidPlan));
    let mut valid = PlanningRecoverySnapshot::default();
    record_response_class(&mut valid, PlanningResponseClass::ValidPlan);
    record_response_class(&mut valid, PlanningResponseClass::ValidPlan);
    assert!(!repeated_identical_failure(&valid));
    let mut clarifying = PlanningRecoverySnapshot::default();
    record_response_class(&mut clarifying, PlanningResponseClass::ClarifyingQuestion);
    record_response_class(&mut clarifying, PlanningResponseClass::ClarifyingQuestion);
    assert!(repeated_identical_failure(&clarifying));
}

#[test]
fn clarification_breaks_the_identical_failure_streak() {
    let mut snapshot = PlanningRecoverySnapshot::default();
    record_response_class(&mut snapshot, PlanningResponseClass::NativeToolCalls);
    record_response_class(&mut snapshot, PlanningResponseClass::NativeToolCalls);
    assert!(repeated_identical_failure(&snapshot));
    record_clarification(&mut snapshot, "The target file is README.md.", false);
    assert!(!repeated_identical_failure(&snapshot));
    assert_eq!(snapshot.response_class, Some(PlanningResponseClass::NativeToolCalls));
}

#[test]
fn retry_correction_names_every_failure_shape_without_debug_formatting() {
    for class in [
        PlanningResponseClass::EmptyContent,
        PlanningResponseClass::ReasoningOnly,
        PlanningResponseClass::NativeToolCalls,
        PlanningResponseClass::MalformedPlan,
        PlanningResponseClass::Truncation,
        PlanningResponseClass::ProviderFailure,
        PlanningResponseClass::ValidPlan,
        PlanningResponseClass::ClarifyingQuestion,
    ] {
        let first = planning_retry_correction_directive(class, false);
        assert!(first.starts_with("Planning retry correction: the previous synthesis "));
        assert!(first.contains("Tools are not available on this pass"));
        assert!(!first.contains("NativeToolCalls"), "noun prose, not debug names: {first}");
        let repeated = planning_retry_correction_directive(class, true);
        assert!(repeated.contains("the last two syntheses each"));
        assert!(repeated.contains("retrying unchanged is unlikely to help"));
    }
    let native = planning_retry_correction_directive(PlanningResponseClass::NativeToolCalls, false);
    assert!(native.contains("emitted native tool calls on a tool-free pass"));
}
