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
