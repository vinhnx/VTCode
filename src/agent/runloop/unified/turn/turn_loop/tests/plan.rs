use super::*;

#[tokio::test]
async fn plan_mode_recovery_fallback_marks_interview_pending_and_preserves_research() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);
    assert!(!plan_session.interview_pending());

    let mut history = vec![uni::Message::user("plan launch-time optimization".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    // Planning session must survive the failed recovery so the next turn
    // re-forces the interview instead of dead-ending.
    assert!(plan_session.interview_pending());
    // The plan-aware fallback must be shown (not the generic dead-end one).
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text().contains(PLANNING_RECOVERY_SYNTHESIS_FALLBACK)
    }));
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    }));
}

#[tokio::test]
async fn plan_mode_recovery_exhausted_finalizes_instead_of_reforcing_interview() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);
    // Mirrors the cross-turn loop bug: the post-tool recovery cycle cap was
    // reached because the planning context is saturated. Re-forcing the
    // interview on the next turn would re-research the still-huge context and
    // loop forever.
    plan_session.mark_recovery_exhausted();
    assert!(!plan_session.interview_pending());

    let mut history = vec![uni::Message::user("plan launch-time optimization".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("context length exceeded")),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    // Must NOT re-force the interview — that is what caused the infinite loop.
    assert!(!plan_session.interview_pending());
    // Must conclude with the USER-facing recovery-exhausted notice (not the
    // model-addressed `*_FINALIZE` directive). With no validated persisted
    // artifact, the user must be told to keep planning rather than implement.
    let last = history.last().unwrap();
    assert_eq!(last.role, uni::MessageRole::Assistant);
    assert_eq!(last.phase, Some(uni::AssistantPhase::FinalAnswer));
    let text = last.content.as_text();
    assert!(text.contains("Plan synthesis failed after repeated recovery attempts"));
    assert!(
        !text.contains("Do NOT attempt more tool calls"),
        "model directive must not leak into the user-visible final answer"
    );
    assert!(!text.contains("`implement`"), "no approval hint is allowed without a valid draft");
    assert!(text.to_ascii_lowercase().contains("keep planning"));
}

#[tokio::test]
async fn plan_mode_recovery_rejects_non_plan_salvage() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);

    let mut history = vec![uni::Message::user("plan launch-time optimization".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        None,
        // Salvage that is prose, not a real `<proposed_plan>`.
        Some("Partial plan: batch config reads.".to_string()),
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert!(plan_session.interview_pending());
    let last = history.last().unwrap();
    // The garbled/non-plan salvage must NOT be injected as the plan; the
    // structured plan-mode message is used instead.
    assert!(last.content.as_text().contains("final synthesis failed"));
    assert!(!last.content.as_text().contains("batch config reads"));
}

#[tokio::test]
async fn plan_mode_recovery_fallback_lists_files_read_when_present() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.enter(PlanningEntrySource::UserRequest);

    // Simulate the turn_640 shape: a wall-clock-budgeted plan turn that read
    // several files before the tool-free recovery follow-up failed.
    let mut history = vec![
        uni::Message::user("plan launch-time optimization".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            "{\"path\": \"src/main.rs\", \"content\": \"...\"}".to_string(),
        ),
        uni::Message::tool_response(
            "call_2".to_string(),
            "{\"path\": \"src/startup/mod.rs\", \"content\": \"...\"}".to_string(),
        ),
    ];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert!(plan_session.interview_pending());
    let last = history.last().unwrap();
    let text = last.content.as_text();
    // Plan mode must stay at least as informative as the generic dead-end:
    // it must still surface the files already read so the next turn can reuse
    // them instead of re-exploring.
    assert!(text.contains("Files already read this turn"));
    assert!(text.contains("src/main.rs"));
    assert!(text.contains("src/startup/mod.rs"));
    // And it must lead with the plan-aware message, not the generic one.
    assert!(text.contains(PLANNING_RECOVERY_SYNTHESIS_FALLBACK));
}

/// Regression test for the plan-mode "cut off mid-flight" bug: a planning
/// synthesis truncated at the model's output token limit (unclosed
/// supported plan tag) must be detected so the turn loop can condense and
/// re-emit instead of accepting a partial plan or looping.
#[test]
fn plan_synthesis_truncated_detects_unclosed_proposed_plan() {
    let truncated = uni::LLMResponse {
        content: Some("<proposed_plan>\n# Improve launch time\n## Steps\n1. Fix warmup -> src/main.rs".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(
        plan_synthesis_was_truncated(&truncated),
        "unclosed <proposed_plan> with Length finish must be detected as truncated"
    );

    let alternate_truncated = uni::LLMResponse {
        content: Some("<plan>\n# Improve launch time\n## Steps\n1. Fix warmup -> src/main.rs".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(
        plan_synthesis_was_truncated(&alternate_truncated),
        "unclosed <plan> with Length finish must be detected as truncated"
    );

    // A complete plan (closed tag) is not a truncation even with Length.
    let complete = uni::LLMResponse {
        content: Some("<proposed_plan>\n# Title\n## Steps\n1. x\n</proposed_plan>".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(!plan_synthesis_was_truncated(&complete), "closed <proposed_plan> must not be flagged as truncated");

    let alternate_complete = uni::LLMResponse {
        content: Some("<plan>\n# Title\n## Steps\n1. x\n</plan>".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Length,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(!plan_synthesis_was_truncated(&alternate_complete), "closed <plan> must not be flagged as truncated");

    // A normal (Stop) response that happens to mention the tag is not truncated.
    let normal = uni::LLMResponse {
        content: Some("<proposed_plan>\n# Title\n</proposed_plan>".to_string()),
        model: "noop".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: uni::FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: Vec::new(),
        compaction: None,
    };
    assert!(!plan_synthesis_was_truncated(&normal), "Stop-finished plan must not be flagged as truncated");
}

/// End-to-end regression test for the plan-mode "cut off mid-flight" fix (Fix B):
/// when the planning synthesis is truncated at the model's output token limit
/// (unclosed `<proposed_plan>`, `finish_reason == Length`), the turn loop must
/// inject `PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE` and re-run the
/// synthesis once to produce a compact completion — NOT loop forever or accept
/// the partial plan. The retry response is a plain completion (no
/// `<proposed_plan>`) so the planning interview is not re-triggered, keeping
/// the test deterministic and focused on the re-prompt control flow.
#[tokio::test]
async fn planning_synthesis_truncated_retries_with_compact_spec() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct TruncateThenCompactProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TruncateThenCompactProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let (content, finish_reason) = if n == 0 {
                // First pass: plan cut off mid-<proposed_plan> (output token limit).
                (
                    "<proposed_plan>\n# Improve launch time\n## Summary\nMake cold start faster.\n## Steps\n1. Fix warmup -> src/main.rs -> verify: build".to_string(),
                    uni::FinishReason::Length,
                )
            } else {
                // Second pass: compact completion after the condense directive.
                (
                    "Plan condensed: warmup path in src/main.rs fixed; rebuild to verify.".to_string(),
                    uni::FinishReason::Stop,
                )
            };
            Ok(uni::LLMResponse {
                content: Some(content),
                model: request.model.clone(),
                tool_calls: None,
                usage: None,
                finish_reason,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }
        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(TruncateThenCompactProvider { calls: calls.clone() }));

    let mut history = vec![uni::Message::user("make a plan to improve launch time".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("turn loop must complete after condensing the truncated plan");

    // Two generations: the truncated pass + exactly one compact retry (bounded
    // by MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS, so it must NOT loop).
    assert_eq!(calls.load(Ordering::SeqCst), 2, "must re-run synthesis exactly once after truncation, not loop");

    // The condense directive must have been injected into the history.
    assert!(
        history.iter().any(|message| message
            .content
            .as_text()
            .contains(PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE)),
        "condense directive must be injected after a truncated plan"
    );

    // The final assistant message must be the compact retry, not the truncated
    // draft (proving the partial plan was discarded and re-emitted).
    let final_text = history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().to_string())
        .unwrap_or_default();
    assert!(final_text.contains("Plan condensed:"), "final answer must be the compact retry, got: {final_text}");
    assert!(
        !final_text.contains("Fix warmup -> src/main.rs -> verify: build"),
        "final answer must not be the truncated draft"
    );
}
