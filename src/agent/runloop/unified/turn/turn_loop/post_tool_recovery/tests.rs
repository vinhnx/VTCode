use super::*;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use vtcode_commons::llm::LLMError;

fn transient_err() -> anyhow::Error {
    anyhow::Error::new(LLMError::Network {
        message: "simulated network blip".to_string(),
        metadata: None,
    })
}

fn context_capacity_err() -> anyhow::Error {
    anyhow::Error::new(LLMError::InvalidRequest {
        message: "maximum context length is 114688 tokens".to_string(),
        metadata: None,
    })
}

#[test]
fn follow_up_failure_tui_lines_obey_two_line_contract() {
    let status = follow_up_failure_status_line(" (transient; bounded retry scheduled)");
    assert!(!status.contains('\n'), "status stays one line: {status}");
    assert!(!status.contains("Follow-up error category"), "no category line: {status}");
    assert!(status.contains("model follow-up failed"));

    for action in [
        follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, false, false, false),
        follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, true, false, false),
        follow_up_failure_action_line(PostToolFailureRecovery::RetryToolFree, false, false, false),
        follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, true, true),
        follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, false, false),
    ] {
        assert!(!action.contains('\n'), "action stays one line: {action}");
        assert!(!action.contains("Follow-up error category"), "no category leak: {action}");
        assert!(!action.contains("Elapsed:"), "no forensics: {action}");
    }
}

#[test]
fn follow_up_failure_action_line_matches_outcome() {
    assert!(
        follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, true, false, false)
            .contains("Context capacity")
    );
    assert!(
        follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, false, false, false)
            .contains("tool-enabled recovery")
    );
    assert!(
        follow_up_failure_action_line(PostToolFailureRecovery::RetryToolFree, false, false, false)
            .contains("tool-free recovery")
    );
    assert!(
        follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, true, true)
            .contains("keep planning"),
        "planning stop names the next verb"
    );
    assert!(
        follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, false, true)
            .contains("narrower prompt"),
        "non-retryable stop names the retry tip"
    );
    assert!(
        follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, false, false)
            .contains("continue"),
        "misconfiguration stop offers continue"
    );
}

#[test]
fn misconfiguration_network_error_is_not_transient_recovery() {
    let err = anyhow::Error::new(LLMError::Network {
        message: "network error: invalid endpoint in base_url".to_string(),
        metadata: None,
    });

    assert!(!is_transient_recovery_error(&err));
    assert!(is_transient_recovery_error(&transient_err()));
}

#[test]
fn guided_misconfiguration_is_not_reclassified_as_transient() {
    let err = anyhow::anyhow!(
        "Network error: invalid endpoint in base_url. Check settings/config first (base_url in vtcode.toml): fix it. Correct the configuration before retrying."
    );

    assert!(misconfiguration_guidance(&err).is_none(), "existing guidance must not be rendered twice");
    assert!(!is_transient_recovery_error(&err));
}

// ---- Pure policy tests for `planning_finalize_notice` ----
// All six (exhaustion × draft-ready) combinations are tested in isolation,
// independent of the async harness. The critical invariant: a ready-draft
// prompt promising "Review the plan below" must ONLY appear when
// plan_ready == true. The no-draft variants must never promise a reviewable
// draft (checkpoint turn_902).

#[test]
fn finalize_notice_budget_exhausted_with_draft_mentions_preserved_plan() {
    let notice = planning_finalize_notice(PlanningRecoveryOutcome::BudgetExhausted { plan_ready: true });
    assert!(notice.contains("session plan file"), "budget+draft must point to the persisted plan: {notice}");
    assert!(!notice.contains("Re-state"), "budget+draft must not ask to re-state: {notice}");
}

#[test]
fn finalize_notice_budget_exhausted_no_draft_asks_to_restate() {
    let notice = planning_finalize_notice(PlanningRecoveryOutcome::BudgetExhausted { plan_ready: false });
    assert!(notice.contains("Re-state"), "budget+no-draft must ask to re-state: {notice}");
    assert!(!notice.contains("session plan file"), "budget+no-draft must not reference a plan file: {notice}");
    assert!(!notice.contains("Plan draft ready"), "must not promise a draft: {notice}");
}

#[test]
fn finalize_notice_recovery_exhausted_with_draft_mentions_preserved_plan() {
    let notice = planning_finalize_notice(PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: true });
    assert!(notice.contains("session plan file"), "recovery+draft must point to the persisted plan: {notice}");
}

#[test]
fn finalize_notice_recovery_exhausted_no_draft_asks_to_restate() {
    let notice = planning_finalize_notice(PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: false });
    assert!(notice.contains("Re-state"), "recovery+no-draft must ask to re-state: {notice}");
    assert!(!notice.contains("session plan file"), "recovery+no-draft must not reference a plan file: {notice}");
    assert!(!notice.contains("Plan draft ready"), "must not promise a draft: {notice}");
}

#[test]
fn finalize_notice_interview_denied_with_draft_offers_review_prompt() {
    let notice = planning_finalize_notice(PlanningRecoveryOutcome::InterviewDenied { plan_ready: true });
    assert!(notice.contains("Plan draft ready"), "denied+draft must offer the review prompt: {notice}");
    assert!(notice.contains("Review the plan below"), "denied+draft must present the draft: {notice}");
}

#[test]
fn finalize_notice_interview_denied_no_draft_never_promises_review() {
    let notice = planning_finalize_notice(PlanningRecoveryOutcome::InterviewDenied { plan_ready: false });
    assert!(!notice.contains("Plan draft ready"), "denied+no-draft must NOT promise a draft: {notice}");
    assert!(!notice.contains("Review the plan below"), "denied+no-draft must NOT ask to review: {notice}");
    assert!(
        notice.contains("did not produce an approval-ready plan"),
        "must explain no plan was produced: {notice}"
    );
}

#[test]
fn finalize_notice_from_session_classifies_correctly() {
    let mut session = PlanningWorkflowSessionState::default();
    assert_eq!(
        PlanningRecoveryOutcome::from_session(&session, false),
        PlanningRecoveryOutcome::InterviewDenied { plan_ready: false }
    );
    session.mark_budget_exhausted();
    assert_eq!(
        PlanningRecoveryOutcome::from_session(&session, true),
        PlanningRecoveryOutcome::BudgetExhausted { plan_ready: true }
    );
    let mut session = PlanningWorkflowSessionState::default();
    session.mark_recovery_exhausted();
    assert_eq!(
        PlanningRecoveryOutcome::from_session(&session, false),
        PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: false }
    );
}

// ---- Async integration tests ----

#[tokio::test]
async fn transient_post_tool_failure_selects_tool_enabled_retry() {
    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![uni::Message::tool_response(
        "call-1".to_string(),
        "{\"ok\":true}".to_string(),
    )];

    let recovery = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut working_history,
        &transient_err(),
        1,
        0,
        "test",
        true,
        true,
        false,
    )
    .expect("recovery policy should resolve");

    assert_eq!(recovery, PostToolFailureRecovery::RetryToolEnabled);
    assert!(
        working_history
            .iter()
            .any(|message| { message.content.as_text().contains("required write or verification tools") })
    );
}

#[tokio::test]
async fn execution_error_in_plan_mode_selects_tool_free_plan_synthesis() {
    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![uni::Message::tool_response(
        "call-1".to_string(),
        "{\"ok\":true}".to_string(),
    )];
    let err = anyhow::anyhow!("simulated follow-up execution failure");
    assert_eq!(vtcode_commons::classify_anyhow_error(&err), ErrorCategory::ExecutionError);

    let recovery = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut working_history,
        &err,
        1,
        0,
        "test",
        true,
        false,
        true,
    )
    .expect("plan-mode execution failure should resolve");

    assert_eq!(recovery, PostToolFailureRecovery::RetryToolFree);
    assert!(
        working_history
            .iter()
            .any(|message| { message.content.as_text().contains("<proposed_plan>") })
    );
}

#[tokio::test]
async fn out_of_band_copilot_progress_enables_post_tool_recovery_without_tool_history() {
    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![uni::Message::user("finish the plan after the interview".to_string())];

    let recovery = maybe_recover_after_post_tool_llm_failure_with_progress(PostToolLlmRecoveryInputs {
        renderer: &mut renderer,
        working_history: &mut working_history,
        err: &transient_err(),
        step_count: 2,
        turn_history_start_len: 0,
        failure_stage: "execute_llm_request",
        allow_tool_free_retry: true,
        allow_tool_enabled_retry: true,
        planning_active: true,
        out_of_band_tool_progress: true,
    })
    .expect("inline Copilot progress should activate bounded post-tool recovery");

    assert_eq!(recovery, PostToolFailureRecovery::RetryToolEnabled);
    assert!(working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE
    }));
}

#[tokio::test]
async fn context_capacity_failure_selects_compacting_tool_enabled_retry() {
    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![uni::Message::tool_response(
        "call-1".to_string(),
        "{\"ok\":true}".to_string(),
    )];

    let recovery = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut working_history,
        &context_capacity_err(),
        1,
        0,
        "execute_llm_request",
        false,
        true,
        false,
    )
    .expect("context recovery policy should resolve");

    assert_eq!(recovery, PostToolFailureRecovery::RetryToolEnabled);
}

#[tokio::test]
async fn context_capacity_marker_from_non_request_stage_does_not_enable_tool_retry() {
    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![uni::Message::tool_response(
        "call-1".to_string(),
        "{\"ok\":true}".to_string(),
    )];

    let recovery = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut working_history,
        &context_capacity_err(),
        1,
        0,
        "process_llm_response",
        false,
        true,
        false,
    )
    .expect("non-request recovery policy should resolve");

    assert_eq!(recovery, PostToolFailureRecovery::StopAfterDirective);
    assert!(
        !working_history
            .iter()
            .any(|message| { message.content.as_text().contains("required write or verification tools") })
    );
}

#[tokio::test]
async fn repeated_tool_enabled_retry_failure_returns_blocked_handoff() {
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![
        uni::Message::user("apply the requested fix".to_string()),
        uni::Message::tool_response("call-1".to_string(), "{\"ok\":true}".to_string()),
    ];
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
    let err = transient_err();

    let first = dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut renderer,
        working_history: &mut working_history,
        harness_state: &mut harness_state,
        harness_emitter: None,
        plan_session: None,
        plan_state: None,
        err: &err,
        step_count: 1,
        turn_history_start_len: 1,
        stage: "first",
        tool_free_recovery: false,
    })
    .await
    .expect("first recovery dispatch should succeed");
    assert!(matches!(first, PostToolFailureAction::Continue));
    assert!(harness_state.consume_recovery_pass());

    let second = dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut renderer,
        working_history: &mut working_history,
        harness_state: &mut harness_state,
        harness_emitter: None,
        plan_session: None,
        plan_state: None,
        err: &err,
        step_count: 2,
        turn_history_start_len: 1,
        stage: "second",
        tool_free_recovery: false,
    })
    .await
    .expect("second recovery dispatch should produce a truthful handoff");

    assert!(matches!(second, PostToolFailureAction::Break(TurnLoopResult::Blocked { reason: Some(_) })));
}

#[tokio::test]
async fn dispatch_recovers_plan_follow_up_after_inline_copilot_interview() {
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![uni::Message::user("plan the requested runtime fix".to_string())];
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
    harness_state.record_out_of_band_tool_progress();
    harness_state.switch_to_tool_free_recovery();
    assert!(harness_state.consume_recovery_pass());
    let mut plan_session = PlanningWorkflowSessionState::default();
    let err = transient_err();

    let action = dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut renderer,
        working_history: &mut working_history,
        harness_state: &mut harness_state,
        harness_emitter: None,
        plan_session: Some(&mut plan_session),
        plan_state: None,
        err: &err,
        step_count: 2,
        turn_history_start_len: 0,
        stage: "execute_llm_request",
        tool_free_recovery: true,
    })
    .await
    .expect("inline interview progress should schedule bounded synthesis recovery");

    assert!(matches!(action, PostToolFailureAction::Continue));
    assert_eq!(harness_state.recovery_retry_count(), 1);
}

#[tokio::test]
async fn planning_synthesis_transient_failure_gets_one_outer_retry() {
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

    let mut renderer = AnsiRenderer::stdout();
    let mut working_history = vec![
        uni::Message::user("plan the launch-time optimization".to_string()),
        uni::Message::tool_response("call-1".to_string(), "{\"evidence\":\"startup path\"}".to_string()),
    ];
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
    let mut plan_session = PlanningWorkflowSessionState::default();
    let err = transient_err();

    harness_state.switch_to_tool_free_recovery();
    assert!(harness_state.consume_recovery_pass());

    let first = dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut renderer,
        working_history: &mut working_history,
        harness_state: &mut harness_state,
        harness_emitter: None,
        plan_session: Some(&mut plan_session),
        plan_state: None,
        err: &err,
        step_count: 1,
        turn_history_start_len: 0,
        stage: "execute_llm_request",
        tool_free_recovery: true,
    })
    .await
    .expect("the first synthesis failure should schedule a retry");

    assert!(matches!(first, PostToolFailureAction::Continue));
    assert_eq!(harness_state.recovery_retry_count(), 1);
    assert!(harness_state.consume_recovery_pass());

    let second = dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut renderer,
        working_history: &mut working_history,
        harness_state: &mut harness_state,
        harness_emitter: None,
        plan_session: Some(&mut plan_session),
        plan_state: None,
        err: &err,
        step_count: 2,
        turn_history_start_len: 0,
        stage: "execute_llm_request",
        tool_free_recovery: true,
    })
    .await
    .expect("the bounded retry should finalize recovery");

    assert!(matches!(second, PostToolFailureAction::Break(TurnLoopResult::Completed { .. })));
    assert!(
        plan_session.interview_pending(),
        "after the bounded retry, planning must remain resumable instead of approving an incomplete plan"
    );
    assert_eq!(harness_state.recovery_retry_count(), 1);
}

#[tokio::test]
async fn tool_free_recovery_keeps_planning_alive_on_transient_error() {
    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        Some(&transient_err()),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(
        plan_session.interview_pending(),
        "transient error must keep planning alive by re-forcing the interview"
    );
}

#[tokio::test]
async fn tool_free_recovery_keeps_planning_alive_on_non_transient_error() {
    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let err = anyhow::Error::new(LLMError::InvalidRequest { message: "bad request".to_string(), metadata: None });

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        Some(&err),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(
        plan_session.interview_pending(),
        "any tool-free recovery failure must keep planning alive (not dead-end)"
    );
}

#[tokio::test]
async fn dispatch_marks_recovery_exhausted_when_wall_clock_exhausted_in_plan_mode() {
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
    use vtcode_core::utils::ansi::AnsiRenderer;

    let mut renderer = AnsiRenderer::stdout();
    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
    harness_state.wall_clock_exhausted_emitted = true;
    let mut plan_session = PlanningWorkflowSessionState::default();
    let err = transient_err();

    let action = dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut renderer,
        working_history: &mut working_history,
        harness_state: &mut harness_state,
        harness_emitter: None,
        plan_session: Some(&mut plan_session),
        plan_state: None,
        err: &err,
        step_count: 1,
        turn_history_start_len: 0,
        stage: "stage",
        tool_free_recovery: true,
    })
    .await
    .expect("dispatch must not error");

    assert!(
        plan_session.is_recovery_exhausted(),
        "wall-clock exhaustion during planning must mark the session \
         recovery-exhausted so the plan finalizes instead of looping"
    );
    assert!(!plan_session.interview_pending(), "must not re-force the interview after wall-clock exhaustion");
    assert!(matches!(action, PostToolFailureAction::Break(_)));
}

#[tokio::test]
async fn tool_free_recovery_finalizes_when_budget_exhausted() {
    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.mark_budget_exhausted();

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        Some(&transient_err()),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(
        !plan_session.interview_pending(),
        "budget-exhausted must not re-force the interview (would loop forever)"
    );
    assert!(
        working_history.iter().any(|m| m.role == uni::MessageRole::Assistant),
        "budget-exhausted must finalize the plan with a fallback answer"
    );
}

#[tokio::test]
async fn tool_free_recovery_keeps_planning_when_interview_denied_without_valid_plan() {
    // When `request_user_input` is permanently denied (non-interactive
    // runtime), the recovery fallback must not advertise implementation
    // until a validated persisted artifact exists. It must keep the
    // planning session actionable without promising another interview.
    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.mark_interview_denied();

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        Some(&transient_err()),
        None,
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(
        !plan_session.interview_pending(),
        "interview-denied must not re-force the interview (would recur forever)"
    );
    let text = working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("a final answer must be pushed")
        .content
        .as_text();
    // No draft was persisted, so the ready-draft HITL prompt must NOT be
    // used: it promises "Plan draft ready / Review the plan below" and
    // offers `yes`/`implement`/`no`/`edit` choices that dead-end without a
    // persisted plan (checkpoint turn_902).
    assert!(
        !text.contains("Plan draft ready"),
        "must not promise a reviewable draft when none was persisted: {text}"
    );
    assert!(
        !text.contains("Review the plan below"),
        "must not tell the user to review a draft that does not exist: {text}"
    );
    assert!(
        !text.contains("Yes, clear context and implement"),
        "must not offer an implementation choice without a persisted plan: {text}"
    );
    assert!(
        !text.contains("interview will be presented"),
        "interview-denied fallback must NOT promise a future interview: {text}"
    );
    assert!(text.to_ascii_lowercase().contains("keep planning"), "fallback must keep planning active: {text}");
    assert!(
        text.contains("did not produce an approval-ready plan"),
        "the no-draft notice must explain that synthesis produced no plan: {text}"
    );
}

#[tokio::test]
async fn plan_mode_recovery_rejects_garbled_tool_call_salvage() {
    // When the tool-free synthesis fails and the only "salvage" is a
    // rambling monologue with tool-call markup stripped out (no real
    // plan), plan mode must NOT inject that garbage as the proposed plan.
    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let garbled = "I have enough to plan. <invoke name=\"unified_search\"> \
        read more files</invoke> Here is my half-baked plan.";

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        None,
        Some(garbled.to_string()),
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(plan_session.interview_pending(), "non-exhausted plan failure must re-force the interview");
    let text = working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("a final answer must be pushed")
        .content
        .as_text();
    assert!(
        text.contains("final synthesis failed"),
        "plan-mode fallback must be the structured message, not garbled salvage: {text}"
    );
    assert!(!text.contains("unified_search"), "garbled tool-call salvage must not leak into the plan: {text}");
}

#[tokio::test]
async fn plan_mode_recovery_rejects_partial_proposed_plan_salvage() {
    // A partial proposed plan is not approval-ready and must not be
    // presented or persisted as if it were a completed artifact.
    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let partial_plan = "<proposed_plan>\n- Action: add caching -> src/cache.rs\n  verify: cargo test\n</proposed_plan>";

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        None,
        Some(partial_plan.to_string()),
        Some(&mut plan_session),
        None,
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    let text = working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("a final answer must be pushed")
        .content
        .as_text();
    assert!(!text.contains("<proposed_plan"), "partial plans must not be presented: {text}");
    assert!(
        text.to_ascii_lowercase().contains("keep planning"),
        "partial plans must keep planning active: {text}"
    );
}

#[tokio::test]
async fn plan_mode_recovery_persists_salvaged_proposed_plan_to_session_file() {
    // Regression: when the tool-free recovery pass finalizes the plan from
    // an inline `<proposed_plan>` (tools were disabled, so the model could
    // not write the file itself), the plan must be persisted to the
    // session plan file so the "preserved in the session plan file"
    // notice is truthful. Previously the plan lived only in chat history
    // and `.vtcode/plans/` stayed empty/template-only.
    use crate::agent::runloop::unified::planning_workflow::PlanningWorkflowState;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
    let plan_file = state.plans_dir().join("recovered-plan.md");
    state.set_plan_file(Some(plan_file.clone())).await;

    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.mark_budget_exhausted();
    let salvaged = r#"<proposed_plan>
# Launch-time recovery

## Summary
Persist a concrete recovery plan without implementing it.

## Implementation Steps
1. Add caching -> files: [src/cache.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run the targeted planning tests.

## Assumptions and Defaults
1. The existing cache policy remains unchanged.
</proposed_plan>"#;

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        Some(&transient_err()),
        Some(salvaged.to_string()),
        Some(&mut plan_session),
        Some(&state),
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    let content =
        std::fs::read_to_string(&plan_file).expect("salvaged plan must be persisted to the session plan file");
    assert!(
        content.contains("Add caching"),
        "salvaged plan must be written to the session plan file, got: {content}"
    );
}

#[tokio::test]
async fn interview_denied_with_persisted_draft_offers_approval_prompt() {
    // Counterpart to `tool_free_recovery_keeps_planning_when_interview_denied_without_valid_plan`:
    // when the interview is denied AND a valid draft was already persisted
    // (e.g. from a prior turn) but THIS turn's recovery salvage produced no
    // ready plan, the ready-draft HITL prompt ("Plan draft ready ... Review
    // the plan below ... yes/implement/no/edit") must still be shown so the
    // user can approve the real on-disk draft. Gating the prompt on
    // `persisted_plan_ready` must not suppress it in the legitimate
    // with-draft case (turn_902).
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
    let plan_file = state.plans_dir().join("denied-interview-plan.md");
    state.set_plan_file(Some(plan_file.clone())).await;
    // Pre-persist a valid plan so `persisted_plan_is_ready` returns true,
    // simulating a draft carried over from a previous turn.
    let valid_plan = r#"<proposed_plan>
# Denied-interview approval

## Summary
Offer the approval prompt when a real draft was persisted despite the denial.

## Implementation Steps
1. Add caching -> files: [src/cache.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run the targeted planning tests.

## Assumptions and Defaults
1. The existing cache policy remains unchanged.
</proposed_plan>"#;
    persist_plan_draft(&state, valid_plan)
        .await
        .expect("pre-persisting a valid plan must succeed");
    assert!(persisted_plan_is_ready(&state).await, "test precondition: a valid plan must be persisted and ready");

    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.mark_interview_denied();
    // No salvage this turn — the on-disk draft is what the prompt offers.

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        Some(&transient_err()),
        None,
        Some(&mut plan_session),
        Some(&state),
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    let text = working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("a final answer must be pushed")
        .content
        .as_text();
    assert!(
        text.contains("Plan draft ready"),
        "a persisted draft must still get the approval HITL prompt: {text}"
    );
    assert!(
        text.contains("Review the plan below"),
        "the reviewable draft must be presented for approval: {text}"
    );
    assert!(
        !text.contains("did not produce an approval-ready plan"),
        "the no-draft notice must NOT appear when a draft exists: {text}"
    );
}

#[test]
fn plan_mode_recovery_fallback_returns_extracted_plan_without_framing() {
    let valid_inner = "# Plan\n\n## Summary\nClean.\n\n## Implementation Steps\n1. Do it -> files: [src/lib.rs] -> verify: [cargo check]\n\n## Test Cases and Validation\n1. Run cargo check.\n\n## Assumptions and Defaults\n1. Keep existing behavior.\n";
    let salvaged = format!("Here is the plan:\n\n<proposed_plan>\n{valid_inner}\n</proposed_plan>\n\nLet me know.");
    let fallback = plan_mode_recovery_fallback(Some(salvaged), "structured", &[]);
    assert!(
        !fallback.contains("Here is the plan"),
        "framing prose must not leak into the user-visible plan: {fallback}"
    );
    assert!(fallback.contains("## Summary"), "extracted plan must be returned: {fallback}");
}

#[test]
fn plan_mode_recovery_fallback_rejects_garbage_salvage() {
    let fallback = plan_mode_recovery_fallback(
        Some("rambling monologue with no plan block".to_string()),
        "structured message",
        &[],
    );
    assert_eq!(fallback, "structured message");
}

#[tokio::test]
async fn recovery_exhausted_with_persisted_draft_offers_preserved_plan_notice() {
    // Coverage gap: `RecoveryExhausted { plan_ready: true }` was the only
    // (exhaustion × draft-ready) combination not exercised by an async
    // integration test. When recovery is exhausted AND a valid draft was
    // persisted, the notice must reference the session plan file (not ask
    // to re-state) so the user can approve the preserved draft.
    use crate::agent::runloop::unified::planning_workflow::PlanningWorkflowState;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
    let plan_file = state.plans_dir().join("recovery-exhausted-plan.md");
    state.set_plan_file(Some(plan_file.clone())).await;
    let valid_plan = r#"<proposed_plan>
# Recovery-exhausted approval

## Summary
Offer the preserved-plan notice when recovery is exhausted with a persisted draft.

## Implementation Steps
1. Add caching -> files: [src/cache.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run the targeted planning tests.

## Assumptions and Defaults
1. The existing cache policy remains unchanged.
</proposed_plan>"#;
    persist_plan_draft(&state, valid_plan)
        .await
        .expect("pre-persisting a valid plan must succeed");
    assert!(persisted_plan_is_ready(&state).await, "test precondition: a valid plan must be persisted and ready");

    let mut working_history: Vec<uni::Message> = Vec::new();
    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.mark_recovery_exhausted();

    let result = complete_turn_after_failed_tool_free_recovery(
        &mut working_history,
        "stage",
        Some(&transient_err()),
        None,
        Some(&mut plan_session),
        Some(&state),
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(!plan_session.interview_pending(), "recovery-exhausted must not re-force the interview");
    let text = working_history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .expect("a final answer must be pushed")
        .content
        .as_text();
    assert!(
        text.contains("session plan file"),
        "recovery-exhausted+draft must reference the preserved plan file: {text}"
    );
    assert!(!text.contains("Re-state"), "recovery-exhausted+draft must not ask to re-state: {text}");
    assert!(
        !text.contains("did not produce an approval-ready plan"),
        "the no-draft notice must NOT appear when a draft exists: {text}"
    );
}

#[tokio::test]
async fn recovery_fallback_marks_explicit_fallback_flag() {
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

    let mut working_history = vec![uni::Message::user("summarize".to_string())];
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
    assert!(!harness_state.final_response_was_fallback());

    let result = complete_turn_after_failed_tool_free_recovery_with_events(
        &mut working_history,
        "test.stage",
        None,
        None,
        None,
        None,
        None,
        Some(&mut harness_state),
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(
        harness_state.final_response_was_fallback(),
        "recovery fallback must mark the explicit flag so ensure_completed does not rely on !rendered"
    );
    assert!(
        working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && !message.content.as_text().trim().is_empty()
        }),
        "recovery fallback must push a final answer"
    );
}

#[tokio::test]
async fn recovery_fallback_without_harness_state_still_pushes_final() {
    let mut working_history = vec![uni::Message::user("summarize".to_string())];

    let result = complete_turn_after_failed_tool_free_recovery_with_events(
        &mut working_history,
        "test.stage",
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(matches!(result, TurnLoopResult::Completed { .. }));
    assert!(
        working_history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant && message.phase == Some(uni::AssistantPhase::FinalAnswer)
        }),
        "None harness_state must remain test-compatible and still push a final"
    );
}
