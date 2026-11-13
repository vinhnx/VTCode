use super::*;

#[test]
fn harness_state_tracks_post_tool_recovery_cycles() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    assert_eq!(state.post_tool_recovery_cycles(), 0);
    assert_eq!(state.increment_post_tool_recovery_cycle(), 1);
    assert_eq!(state.post_tool_recovery_cycles(), 1);
    assert_eq!(state.increment_post_tool_recovery_cycle(), 2);
    assert_eq!(state.post_tool_recovery_cycles(), 2);
    assert_eq!(state.increment_post_tool_recovery_cycle(), 3);
    assert_eq!(state.post_tool_recovery_cycles(), 3);
}
#[test]
fn harness_state_tracks_task_tracker_create_signatures() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    assert!(
        state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        )
    );
    assert!(
        !state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        )
    );
    assert!(
        state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"y\"]}".to_string()
        )
    );
}
#[test]
fn harness_state_tracks_successful_readonly_signatures() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    assert!(!state.has_successful_readonly_signature("file_operation:ro:len10-fnv1234"));
    assert!(state.record_successful_readonly_signature("file_operation:ro:len10-fnv1234".to_string()));
    assert!(state.has_successful_readonly_signature("file_operation:ro:len10-fnv1234"));
    assert!(!state.record_successful_readonly_signature("file_operation:ro:len10-fnv1234".to_string()));
}
#[test]
fn harness_state_tracks_identical_shell_command_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    assert_eq!(state.record_shell_command_run("exec_command::cargo check".to_string()), 1);
    assert_eq!(state.record_shell_command_run("exec_command::cargo check".to_string()), 2);
    assert_eq!(state.record_shell_command_run("exec_command::cargo test".to_string()), 1);
    assert_eq!(state.last_shell_command_signature.as_deref(), Some("exec_command::cargo test"));
}
#[test]
fn harness_state_resets_shell_command_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    state.record_shell_command_run("exec_command::cargo check".to_string());
    state.reset_shell_command_run_streak();
    assert_eq!(state.consecutive_same_shell_command_runs, 0);
    assert!(state.last_shell_command_signature.is_none());
}
#[test]
fn harness_state_tracks_file_read_family_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    assert_eq!(state.record_file_read_family_call("apply_patch::read::src/lib.rs".to_string()), 1);
    assert_eq!(state.record_file_read_family_call("apply_patch::read::src/lib.rs".to_string()), 2);
    assert_eq!(state.record_file_read_family_call("apply_patch::read::src/main.rs".to_string()), 1);

    state.reset_file_read_family_streak();
    assert_eq!(state.consecutive_same_file_read_family_calls, 0);
}
#[test]
fn file_read_path_counts_track_per_path_regardless_of_slice() {
    let mut state = HarnessTurnState::new(TurnRunId("run-path".to_string()), TurnId("turn-path".to_string()), 600, 3);

    // Same path, different offsets — each increments the path counter.
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1);
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 2);
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 3);
    // Different path gets its own counter.
    assert_eq!(state.record_file_read_path_call("src/main.rs".to_string()), 1);
    // Original path continues counting.
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 4);

    state.reset_file_read_path_counts();
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1);
}

#[test]
fn written_file_resets_only_its_own_read_path_counter() {
    let mut state = HarnessTurnState::new(TurnRunId("run-reset".to_string()), TurnId("turn-reset".to_string()), 600, 3);

    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1);
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 2);
    assert_eq!(state.record_file_read_path_call("src/main.rs".to_string()), 1);
    assert_eq!(state.record_file_read_path_call("src/main.rs".to_string()), 2);

    // A successful write invalidates the looping re-read hypothesis for that
    // path only: post-write reads are verification of new content.
    state.record_written_file("src/lib.rs");
    assert!(state.was_recently_written("src/lib.rs"));
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1, "written path restarts its counter");
    assert_eq!(state.record_file_read_path_call("src/main.rs".to_string()), 3, "unwritten path keeps counting");
}
#[test]
fn harness_state_builds_execution_snapshot() {
    let mut state = HarnessTurnState::new(TurnRunId("run-9".to_string()), TurnId("turn-3".to_string()), 120, 2);
    state.set_phase(TurnPhase::ExecutingTools);

    let snapshot = state.execution_snapshot();
    assert_eq!(snapshot.run_id, "run-9");
    assert_eq!(snapshot.turn_id, "turn-3");
    assert_eq!(snapshot.phase, TurnPhase::ExecutingTools);
    assert_eq!(snapshot.max_tool_wall_clock_secs, 120);
    assert_eq!(snapshot.max_tool_retries, 2);
}
#[test]
fn turn_diagnostic_counters_saturate_and_count_recovery_once() {
    let mut state =
        HarnessTurnState::new(TurnRunId("run-diagnostics".to_string()), TurnId("turn-diagnostics".to_string()), 120, 2);
    state.requested_tool_calls = u32::MAX - 1;
    state.record_requested_tool_calls(usize::MAX);
    state.raw_spooled_bytes = u64::MAX - 2;
    state.model_visible_output_bytes = u64::MAX - 2;
    state.record_tool_output_metrics(true, true, 10, usize::MAX);
    state.record_reused_result();
    state.activate_recovery("adaptive planning synthesis");
    state.activate_recovery("duplicate activation");

    let diagnostics = state.snapshot_turn_diagnostics(Default::default(), u32::MAX);
    assert_eq!(diagnostics.requested_tool_calls, u32::MAX);
    assert_eq!(diagnostics.unadmitted_tool_calls, u32::MAX);
    assert_eq!(diagnostics.reused_results, 2);
    assert_eq!(diagnostics.spooled_results, 1);
    assert_eq!(diagnostics.raw_spooled_bytes, u64::MAX);
    assert_eq!(diagnostics.model_visible_output_bytes, u64::MAX);
    assert_eq!(diagnostics.low_signal_tool_calls, u32::MAX);
    assert_eq!(diagnostics.recovery_activations, 1);
}
