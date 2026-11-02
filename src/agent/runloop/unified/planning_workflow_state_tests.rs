use super::{MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS, MAX_PLAN_VALIDATION_REPAIR_REPROMPTS, PlanningWorkflowSessionState};
use vtcode_core::core::interfaces::session::PlanningEntrySource;

#[test]
fn explicit_plan_entry_requires_approval_across_turns() {
    for source in [
        PlanningEntrySource::UserRequest,
        PlanningEntrySource::AgentSelection,
        PlanningEntrySource::CliFlag,
    ] {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(source);
        state.start_turn();
        assert!(state.requires_explicit_plan_approval(), "entry source {source:?}");
        state.exit();
        assert!(!state.requires_explicit_plan_approval());
    }
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::AgentSuggestion);
    assert!(!state.requires_explicit_plan_approval());
    state.mark_explicit_plan_request();
    assert!(state.requires_explicit_plan_approval());
}

#[test]
fn interview_result_updates_cycle_metrics() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);

    state.record_interview_result(2, false);
    assert_eq!(state.interview_cycles_completed(), 1);
    assert!(!state.last_interview_cancelled());

    state.record_interview_result(0, true);
    assert_eq!(state.interview_cycles_completed(), 1);
    assert!(state.last_interview_cancelled());
}

#[test]
fn entering_resets_interview_cycle_metrics() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    state.record_interview_result(1, false);
    assert_eq!(state.interview_cycles_completed(), 1);

    state.exit();
    state.enter(PlanningEntrySource::UserRequest);
    assert_eq!(state.interview_cycles_completed(), 0);
    assert!(!state.last_interview_cancelled());
}

#[test]
fn restore_agent_after_planning_prefers_previous_then_fallback() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::AgentSuggestion);
    state.set_previous_primary_agent(Some("build".to_string()));
    state.set_fallback_primary_agent(Some("auto".to_string()));
    assert_eq!(state.restore_agent_after_planning(), Some("build"));

    state.set_previous_primary_agent(None);
    assert_eq!(state.restore_agent_after_planning(), Some("auto"));

    state.set_fallback_primary_agent(None);
    assert_eq!(state.restore_agent_after_planning(), None);
}

#[test]
fn restore_agent_after_planning_never_returns_plan_agent() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    // `/plan on` while the plan agent was already active records "plan" as
    // previous; restoring must fall through to the execution fallback
    // instead of re-selecting the read-only planner.
    state.set_previous_primary_agent(Some("plan".to_string()));
    state.set_fallback_primary_agent(Some("build".to_string()));
    assert_eq!(state.restore_agent_after_planning(), Some("build"));

    state.set_fallback_primary_agent(Some("plan".to_string()));
    assert_eq!(state.restore_agent_after_planning(), None);
}

#[test]
fn exit_clears_restore_agent_targets() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    state.set_previous_primary_agent(Some("build".to_string()));
    state.set_fallback_primary_agent(Some("auto".to_string()));

    state.exit();
    assert_eq!(state.restore_agent_after_planning(), None);
}

#[test]
fn plan_exit_header_name_falls_back_to_active_display() {
    use super::plan_exit_header_name;

    assert_eq!(plan_exit_header_name(Some("build"), "auto"), "build");
    assert_eq!(plan_exit_header_name(Some("  "), "auto"), "auto");
    assert_eq!(plan_exit_header_name(None, "build"), "build");
}

#[test]
fn plan_entry_agent_switch_is_deferred_and_consumed_once() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::AgentSuggestion);
    assert!(!state.take_plan_entry_agent_switch());

    // Mid-turn start_planning queues the switch without ending the turn.
    state.queue_plan_entry_agent_switch();
    assert!(state.take_plan_entry_agent_switch());
    // Consumed once: a second take must not re-trigger SwitchPrimaryAgent.
    assert!(!state.take_plan_entry_agent_switch());
}

#[test]
fn plan_entry_agent_switch_cleared_on_enter_and_exit() {
    let mut state = PlanningWorkflowSessionState::default();
    state.queue_plan_entry_agent_switch();
    state.enter(PlanningEntrySource::AgentSuggestion);
    assert!(!state.take_plan_entry_agent_switch(), "enter must clear a stale deferred switch");

    state.queue_plan_entry_agent_switch();
    state.exit();
    assert!(!state.take_plan_entry_agent_switch(), "exit must clear the deferred plan-agent switch");
}

#[test]
fn take_plan_entry_agent_switch_always_consumes_even_when_not_applied() {
    // Stronger handoffs discard the deferred switch at the turn boundary,
    // but the take itself must still clear the flag so a later turn cannot
    // fire a stale plan-agent switch mid-implementation.
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::AgentSuggestion);
    state.queue_plan_entry_agent_switch();
    assert!(state.take_plan_entry_agent_switch());
    assert!(!state.take_plan_entry_agent_switch());
}

#[test]
fn mark_interview_denied_is_permanent_until_reset() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(!state.is_interview_denied());

    state.mark_interview_pending();
    state.mark_interview_denied();
    assert!(state.is_interview_denied());
    // A denial also clears any pending interview request — re-forcing it
    // would just repeat the same policy failure.
    assert!(!state.interview_pending());

    // Re-entering the planning workflow (a fresh session) clears the flag.
    state.exit();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(!state.is_interview_denied());
}

#[test]
fn interview_denial_allows_one_plan_synthesis_retry() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    state.mark_interview_denied();

    assert!(state.plan_synthesis_retry_allowed());
    state.mark_plan_synthesis_retry_used();
    assert!(!state.plan_synthesis_retry_allowed());

    state.exit();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(!state.is_interview_denied());
    assert!(!state.plan_synthesis_retry_allowed());
}

#[test]
fn pseudo_tool_call_reprompts_are_bounded_and_reset_by_enter() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);

    for attempt in 0..MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS {
        assert!(
            state.plan_pseudo_tool_call_reprompt_allowed(),
            "reprompt attempt {attempt} should be allowed before the bound is reached"
        );
        state.mark_plan_pseudo_tool_call_reprompt_used();
    }
    assert!(
        !state.plan_pseudo_tool_call_reprompt_allowed(),
        "reprompts must stop once the bound is exhausted so a checkpoint that keeps emitting markup cannot loop the turn"
    );

    state.exit();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(state.plan_pseudo_tool_call_reprompt_allowed());
}

#[test]
fn plan_validation_repair_survives_turn_boundaries_and_resets_on_explicit_cycle() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    for attempt in 0..MAX_PLAN_VALIDATION_REPAIR_REPROMPTS {
        assert!(
            state.plan_validation_repair_allowed(),
            "repair attempt {attempt} should be allowed before the bound is reached"
        );
        state.mark_plan_validation_repair_used();
    }
    assert!(
        !state.plan_validation_repair_allowed(),
        "validation repairs must stop after the bounded automatic passes"
    );
    assert!(state.bounded_planning_follow_up_allowed());
    state.consume_bounded_planning_follow_up();
    assert!(state.bounded_planning_follow_up_allowed());
    state.consume_bounded_planning_follow_up();
    assert!(!state.bounded_planning_follow_up_allowed());

    state.start_turn();
    assert!(!state.plan_validation_repair_allowed(), "automatic turns cannot replenish repair allowances");
    assert!(!state.bounded_planning_follow_up_allowed(), "a fresh turn has no stale repair request");
    state.begin_explicit_recovery_cycle();
    assert!(state.plan_validation_repair_allowed());
    state.mark_plan_validation_repair_used();
    assert!(
        state.bounded_planning_follow_up_allowed(),
        "a repair remains pending regardless of earlier text responses"
    );

    state.exit();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(state.plan_validation_repair_allowed());
    assert!(!state.bounded_planning_follow_up_allowed(), "re-entry clears pending repair requests");
}

#[test]
fn empty_synthesis_salvage_survives_turn_boundaries_and_resets_on_explicit_cycle() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(state.empty_synthesis_salvage_allowed(), "a fresh planning turn starts with one salvage");

    state.mark_empty_synthesis_salvage_used();
    assert!(
        !state.empty_synthesis_salvage_allowed(),
        "one salvage per turn; a second empty synthesis must fall through to the handoff"
    );

    state.start_turn();
    assert!(!state.empty_synthesis_salvage_allowed(), "automatic continuations cannot replenish salvage");
    state.begin_explicit_recovery_cycle();
    assert!(state.empty_synthesis_salvage_allowed());

    // The salvage schedules a provider request, so exhausted budgets must
    // refuse it rather than spending one more call that cannot succeed.
    state.mark_budget_exhausted();
    assert!(!state.empty_synthesis_salvage_allowed());
    state.start_turn();
    state.mark_recovery_exhausted();
    assert!(!state.empty_synthesis_salvage_allowed());

    state.exit();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(state.empty_synthesis_salvage_allowed(), "re-entry clears the salvage latch");
}

#[test]
fn budget_and_recovery_exhaustion_cleared_by_enter_and_exit() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);

    state.mark_budget_exhausted();
    state.mark_recovery_exhausted();
    assert!(state.is_budget_exhausted());
    assert!(state.is_recovery_exhausted());

    // exit() clears both exhaustion flags.
    state.exit();
    assert!(!state.is_budget_exhausted());
    assert!(!state.is_recovery_exhausted());

    // Re-apply and verify enter() also clears them.
    state.mark_budget_exhausted();
    state.mark_recovery_exhausted();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(!state.is_budget_exhausted());
    assert!(!state.is_recovery_exhausted());
}

#[test]
fn record_interview_result_treats_zero_answered_as_cancelled() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);

    // answered_questions=0 with cancelled=false should still count as cancelled.
    state.record_interview_result(0, false);
    assert!(state.last_interview_cancelled());
    assert_eq!(state.interview_cycles_completed(), 0);
}

#[test]
fn record_interview_result_clamps_answered_questions_to_three() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);

    state.record_interview_result(10, false);
    assert_eq!(state.interview_cycles_completed(), 1);
    assert!(!state.last_interview_cancelled());
}

#[test]
fn pending_approval_identity_is_consumed_once() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    state.mark_plan_approval_pending("thread-1".to_string(), "turn-2".to_string());

    assert_eq!(
        state.take_pending_plan_approval(),
        Some(super::PendingPlanApproval {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-2".to_string(),
        })
    );
    assert_eq!(state.take_pending_plan_approval(), None);
}

#[test]
fn approved_exit_preserves_pending_identity_until_resolution() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    state.mark_plan_approval_pending("thread-1".to_string(), "turn-2".to_string());

    state.exit_preserving_pending_approval();

    assert_eq!(
        state.take_pending_plan_approval(),
        Some(super::PendingPlanApproval {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-2".to_string(),
        })
    );
    assert_eq!(state.take_pending_plan_approval(), None);
}

#[test]
fn bounded_retry_queue_bypasses_cap_without_consuming_validation_budget() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(!state.bounded_planning_follow_up_allowed());

    // A denied-interview or pseudo-tool-call retry queues one allowance
    // without touching the validation-repair budget.
    state.queue_bounded_planning_follow_up();
    assert!(state.bounded_planning_follow_up_allowed());
    assert!(state.plan_validation_repair_allowed());

    state.consume_bounded_planning_follow_up();
    assert!(!state.bounded_planning_follow_up_allowed());
    assert!(state.plan_validation_repair_allowed());
}

#[test]
fn bounded_retry_allowance_survives_prior_text_budget_use() {
    let mut state = PlanningWorkflowSessionState::default();
    state.enter(PlanningEntrySource::UserRequest);

    // Validation repair and generic retries share the pending queue so a
    // retry scheduled after an ordinary planning response still admits
    // exactly the queued requests.
    state.mark_plan_validation_repair_used();
    state.queue_bounded_planning_follow_up();
    assert!(state.bounded_planning_follow_up_allowed());
    state.consume_bounded_planning_follow_up();
    assert!(state.bounded_planning_follow_up_allowed());
    state.consume_bounded_planning_follow_up();
    assert!(!state.bounded_planning_follow_up_allowed());

    state.start_turn();
    assert!(!state.bounded_planning_follow_up_allowed());

    state.queue_bounded_planning_follow_up();
    state.exit();
    state.enter(PlanningEntrySource::UserRequest);
    assert!(!state.bounded_planning_follow_up_allowed());
}

fn drain_transcript_text(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_ui::tui::app::InlineCommand>,
) -> String {
    use vtcode_ui::tui::app::InlineCommand;
    std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.into_iter().map(|s| s.text).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn promote_planning_stage_emits_no_researching_row() {
    // Mid-turn `start_planning` entry must not render the researching row:
    // the turn-loop detector owns it (rendered after the `• Start planning`
    // summary). Emitting here duplicates it (❋, •, ❋).
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_ui::tui::app::InlineHandle;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let _renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    super::promote_planning_stage(&handle);

    let text = drain_transcript_text(&mut receiver);
    assert!(!text.contains("Drafting plan"), "stage promotion must not emit a transcript row, got: {text:?}");
}

#[test]
fn mark_planning_turn_started_emits_single_researching_row() {
    // Turn start / detector path owns the single researching row.
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_ui::tui::app::InlineHandle;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    super::mark_planning_turn_started(&mut renderer, &handle);

    let text = drain_transcript_text(&mut receiver);
    assert_eq!(
        text.matches("Drafting plan").count(),
        1,
        "exactly one researching row per planning turn, got: {text:?}"
    );
}
