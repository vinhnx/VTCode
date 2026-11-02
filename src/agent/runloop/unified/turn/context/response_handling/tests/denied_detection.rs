use super::*;

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
