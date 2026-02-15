use super::*;
use crate::core::agent::state::TaskRunState;
use crate::core::agent::state::record_turn_duration;
use crate::core::agent::task::TaskOutcome;

#[test]
fn record_turn_duration_records_once() {
    let mut durations = Vec::with_capacity(5);
    let mut total_ms = 0u128;
    let mut max_ms = 0u128;
    let mut count = 0usize;
    let mut recorded = false;
    let start = std::time::Instant::now();

    record_turn_duration(
        &mut durations,
        &mut total_ms,
        &mut max_ms,
        &mut count,
        &mut recorded,
        &start,
    );
    record_turn_duration(
        &mut durations,
        &mut total_ms,
        &mut max_ms,
        &mut count,
        &mut recorded,
        &start,
    );

    assert_eq!(durations.len(), 1);
    assert_eq!(count, 1);
}

#[test]
fn finalize_outcome_marks_success() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5, 10000);
    state.has_completed = true;
    state.turns_executed = 2;

    state.finalize_outcome(4);

    assert_eq!(state.completion_outcome, TaskOutcome::Success);
}

#[test]
fn finalize_outcome_turn_limit() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5, 10000);
    state.turns_executed = 6;

    state.finalize_outcome(6);

    assert!(matches!(
        state.completion_outcome,
        TaskOutcome::TurnLimitReached { .. }
    ));
}

#[test]
fn finalize_outcome_tool_loop_limit() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 2, 10000);
    state.turns_executed = 2;
    state.tool_loop_limit_hit = true;

    state.finalize_outcome(10);

    assert_eq!(
        state.completion_outcome,
        TaskOutcome::tool_loop_limit_reached(state.max_tool_loops, state.consecutive_tool_loops)
    );
}

#[test]
fn into_results_computes_metrics() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5, 10000);
    state.turn_durations_ms = vec![100, 200, 300];
    state.turn_total_ms = 600;
    state.turn_max_ms = 300;
    state.turn_count = 3;
    state.turns_executed = 3;
    state.completion_outcome = TaskOutcome::Success;
    state.modified_files = vec!["file.rs".to_owned()];
    state.executed_commands = vec!["write_file".to_owned()];
    state.warnings = vec!["warning".to_owned()];

    let total_duration_ms = 1_000u128;
    let results = state.into_results("summary".to_owned(), Vec::new(), total_duration_ms);

    assert_eq!(results.outcome, TaskOutcome::Success);
    assert_eq!(results.turns_executed, 3);
    assert_eq!(results.total_duration_ms, total_duration_ms);
    assert_eq!(results.max_turn_duration_ms, Some(300));
    assert_eq!(results.average_turn_duration_ms, Some(200.0));
    assert_eq!(results.modified_files, vec!["file.rs".to_owned()]);
    assert_eq!(results.executed_commands, vec!["write_file".to_owned()]);
    assert_eq!(results.summary, "summary");
    assert_eq!(results.warnings, vec!["warning".to_owned()]);
}
