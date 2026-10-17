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
