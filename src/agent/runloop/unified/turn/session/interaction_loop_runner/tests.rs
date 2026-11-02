use super::*;
use vtcode_ui::tui::app::SubmittedInput;

#[test]
fn matrix_only_completion_returns_to_orchestration_at_the_idle_boundary() {
    assert!(matches!(
        idle_completion_outcome(&InlineLoopAction::Continue, 0, true),
        Some(InteractionOutcome::BackgroundCompletionReady)
    ));
    assert!(idle_completion_outcome(&InlineLoopAction::Continue, 0, false).is_none());
    assert!(matches!(
        idle_completion_outcome(&InlineLoopAction::Continue, 2, false),
        Some(InteractionOutcome::BackgroundCompletionReady)
    ));
}

#[test]
fn matrix_completion_preserves_user_input_and_exit_precedence() {
    for action in [
        InlineLoopAction::Submit(SubmittedInput::new("user steering", vec![])),
        InlineLoopAction::Exit(SessionEndReason::Exit),
    ] {
        assert!(idle_completion_outcome(&action, 2, true).is_none());
    }
}

fn pending_planning_model_selection() -> PlanningWorkflowSessionState {
    let mut state = PlanningWorkflowSessionState::default();
    state.recovery.research_closed = true;
    state.recovery.requests_used = 3;
    state.recovery.outcome = PlanningOutcome::AwaitingInput;
    state.recovery.model_selection_pending = true;
    state.model_picker_opened = true;
    state
}

#[test]
fn planning_model_selection_requires_explicit_acceptance_even_when_configuration_changes() {
    let mut state = pending_planning_model_selection();
    // Different human/canonical provider labels and a changed model cannot
    // stand in for acceptance; neither is an input to this transition.
    state.recovery.provider = "Merge Gateway".into();
    state.recovery.model = "old-model".into();
    assert_eq!(planning_model_selection(&mut state, true, false), PlanningModelSelection::Waiting);
    assert_eq!(state.recovery.requests_used, 3);
    assert_eq!(state.recovery.cycle, 0);
    assert!(state.recovery.model_selection_pending);
}

#[test]
fn planning_same_model_acceptance_starts_exactly_one_bounded_cycle() {
    let mut state = pending_planning_model_selection();
    let mut stats = crate::agent::runloop::unified::state::SessionStats::default();
    assert!(!stats.take_accepted_model_selection());
    stats.record_accepted_model_selection();
    assert_eq!(
        planning_model_selection(&mut state, false, stats.take_accepted_model_selection()),
        PlanningModelSelection::Accepted
    );
    assert!(!stats.take_accepted_model_selection(), "acceptance is consumed once");
    assert_eq!(state.recovery.cycle, 1);
    assert_eq!(state.recovery.requests_used, 0);
    assert!(state.recovery.research_closed);
    assert!(!state.recovery.model_selection_pending && !state.model_picker_opened);
    assert_eq!(planning_model_selection(&mut state, false, false), PlanningModelSelection::Waiting);
    assert_eq!(state.recovery.cycle, 1);
}

#[test]
fn planning_cancelled_or_unavailable_picker_stays_idle_without_replenishing_requests() {
    let mut state = pending_planning_model_selection();
    assert_eq!(planning_model_selection(&mut state, false, false), PlanningModelSelection::Dismissed);
    assert_eq!(state.recovery.requests_used, 3);
    assert_eq!(state.recovery.cycle, 0);
    assert_eq!(state.recovery.outcome, PlanningOutcome::AwaitingInput);
    assert!(!state.recovery.model_selection_pending && !state.model_picker_opened);
}

#[test]
fn planning_model_selection_queue_failure_restores_input_handoff_and_preserves_existing_queue() {
    use vtcode_core::core::agent::steering::{MAX_OVERFLOW_FOLLOW_UP_INTENTS, MAX_QUEUED_FOLLOW_UP_INTENTS};
    let mut state = pending_planning_model_selection();
    assert_eq!(planning_model_selection(&mut state, false, true), PlanningModelSelection::Accepted);
    let mut steering = RuntimeSteering::default();
    let capacity = MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS;
    for index in 0..capacity {
        steering
            .try_queue_follow_up_input(format!("existing-{index}"))
            .expect("fill known capacity");
    }
    assert!(queue_planning_model_recovery(&mut state, &mut steering).is_err());
    assert_eq!(state.recovery.outcome, PlanningOutcome::AwaitingInput);
    assert!(state.recovery.input_prompt_shown);
    assert!(state.recovery.research_closed);
    assert!(!state.recovery.model_selection_pending && !state.model_picker_opened);
    assert_eq!(steering.pending_follow_up_intents_snapshot().len(), capacity);
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("existing-0"));
}

#[test]
fn planning_model_selection_success_queues_one_tool_free_follow_up() {
    let mut state = pending_planning_model_selection();
    assert_eq!(planning_model_selection(&mut state, false, true), PlanningModelSelection::Accepted);
    let mut steering = RuntimeSteering::default();
    queue_planning_model_recovery(&mut state, &mut steering).expect("queue selected-model synthesis");
    assert_eq!(state.recovery.outcome, PlanningOutcome::Synthesizing);
    assert!(state.recovery.research_closed);
    assert_eq!(steering.pending_follow_up_intents_snapshot().len(), 1);
}
