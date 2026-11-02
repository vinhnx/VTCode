use super::*;

#[test]
fn harness_state_tracks_blocked_call_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.blocked_tool_calls, 0);
    assert_eq!(state.record_blocked_tool_call(), 1);
    assert_eq!(state.record_blocked_tool_call(), 2);
    assert_eq!(state.blocked_tool_calls, 2);
    state.reset_blocked_tool_call_streak();
    assert_eq!(state.consecutive_blocked_tool_calls, 0);
}
#[test]
fn harness_state_tracks_and_resets_preflight_failure_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.record_preflight_failure(), 1);
    assert_eq!(state.record_preflight_failure(), 2);
    state.reset_preflight_failure_streak();
    assert_eq!(state.consecutive_preflight_failures, 0);
}
#[test]
fn harness_state_tracks_recovery_state() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(!state.is_recovery_active());
    assert!(!state.recovery_pass_used());

    state.activate_recovery("loop detector");
    assert!(state.is_recovery_active());
    assert_eq!(state.recovery_reason(), Some("loop detector"));
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.recovery_is_tool_free());

    assert!(state.consume_recovery_pass());
    assert!(state.recovery_pass_used());
    assert!(state.finish_recovery_pass());
    assert!(!state.is_recovery_active());
}
#[test]
fn harness_state_consumes_recovery_pass_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(!state.consume_recovery_pass());

    state.activate_recovery("loop detector");
    assert!(state.consume_recovery_pass());
    assert!(!state.consume_recovery_pass());
    assert!(state.recovery_pass_used());
    assert!(state.finish_recovery_pass());
    assert!(!state.finish_recovery_pass());
}
#[test]
fn harness_state_supports_tool_enabled_recovery_retries() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);

    assert!(state.is_recovery_active());
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));
    assert!(!state.recovery_is_tool_free());
    assert!(state.consume_recovery_pass());
    assert!(state.finish_recovery_pass());
}
#[test]
fn harness_state_arms_one_post_tool_compaction_retry() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(state.arm_post_tool_tool_enabled_retry("transient post-tool failure", false));
    assert!(state.is_recovery_active());
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));
    assert!(!state.recovery_is_tool_free());
    assert!(state.post_tool_compaction_pending());
    assert!(state.post_tool_tool_enabled_retry_used());
    assert!(!state.post_tool_context_capacity_failure());
    assert!(state.take_post_tool_compaction_pending());
    assert!(!state.post_tool_compaction_pending());
    assert!(!state.arm_post_tool_tool_enabled_retry("must remain bounded", false));
}
#[test]
fn harness_state_marks_context_capacity_recovery() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(state.arm_post_tool_tool_enabled_retry("context limit", true));
    assert!(state.post_tool_context_capacity_failure());
    assert!(!state.post_tool_context_compaction_failed());

    state.mark_post_tool_context_compaction_failed();
    assert!(state.post_tool_context_compaction_failed());
}
#[test]
fn harness_state_switch_to_tool_free_recovery_from_inactive() {
    // Regression guard for the post-tool follow-up infinite loop:
    // `switch_to_tool_free_recovery` must transition `Inactive -> Pending`
    // (not just `InPass` or `Completed` to `Pending`). When a normal
    // (non-recovery)
    // turn's follow-up LLM phase fails, the phase is `Inactive`; if the
    // switch left it there, `consume_recovery_pass()` would return false,
    // `tool_free_recovery` would evaluate to false, and tools would never
    // be disabled at the API level.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    // Fresh state: recovery is inactive.
    assert!(!state.is_recovery_active());
    assert!(!state.recovery_is_tool_free());

    // Switching from Inactive must engage a tool-free recovery pass.
    assert!(state.switch_to_tool_free_recovery(), "switch from Inactive must report a phase change");
    assert!(state.is_recovery_active(), "phase must be Pending");
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.recovery_is_tool_free());

    // The pass must be consumable. This is what the turn loop checks to
    // decide `tool_free_recovery = true` and disable tools at the API level.
    assert!(state.consume_recovery_pass(), "consume_recovery_pass must succeed after switch from Inactive");

    // A default recovery reason must be seeded so the [Recovery Mode]
    // request block reports why recovery was engaged.
    assert!(state.recovery_reason().is_some(), "recovery_reason must be seeded when switching from Inactive");
}
#[test]
fn harness_state_switch_to_tool_free_recovery_from_in_pass_keeps_consumable() {
    // Switching from InPass (a pass already in flight) must still reset to
    // Pending so the next loop iteration can consume it.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
    assert!(state.consume_recovery_pass()); // -> InPass
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));

    assert!(state.switch_to_tool_free_recovery());
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.consume_recovery_pass(), "pass must be consumable again after switching from InPass");
}
#[test]
fn harness_state_switch_to_tool_free_recovery_from_completed_keeps_consumable() {
    // No-regression guard: switching from Completed must reset to Pending.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
    assert!(state.consume_recovery_pass()); // -> InPass
    assert!(state.finish_recovery_pass()); // -> Completed
    assert!(!state.is_recovery_active());

    assert!(state.switch_to_tool_free_recovery());
    assert!(state.is_recovery_active());
    assert!(state.consume_recovery_pass());
}
#[test]
fn harness_state_switch_to_tool_free_recovery_idempotent_when_pending() {
    // When already Pending, switching reports no phase change but still
    // forces the mode to ToolFreeSynthesis.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery("loop detector");
    assert!(state.is_recovery_active()); // Pending

    assert!(!state.switch_to_tool_free_recovery(), "switch from Pending must report no phase change");
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.consume_recovery_pass());
}
#[test]
fn harness_state_switch_to_tool_free_recovery_resets_retry_count_from_inactive() {
    // Switching from Inactive resets the retry counter (mirrors
    // activate_recovery_with_mode) so any stale count does not
    // prematurely exhaust the in-pass retry budget on the new pass.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    // Fresh state: retry_count is 0, phase is Inactive.
    assert_eq!(state.recovery_retry_count(), 0);

    // Switch from Inactive to Pending: retry count stays 0.
    state.switch_to_tool_free_recovery();
    assert_eq!(state.recovery_retry_count(), 0);

    // Complete this pass and start a second cycle.
    assert!(state.consume_recovery_pass());
    assert!(state.retry_recovery_pass()); // retry_count becomes 1
    assert_eq!(state.recovery_retry_count(), 1);

    // Switch from Completed to Pending: retry count is not reset
    // (only Inactive triggers the reset).
    assert!(state.consume_recovery_pass());
    assert!(state.finish_recovery_pass());
    state.switch_to_tool_free_recovery();
    assert_eq!(state.recovery_retry_count(), 1, "retry count must NOT be reset when switching from Completed");

    // Consume and retry: the budget should tick up to 2, not start over.
    assert!(state.consume_recovery_pass());
    assert!(state.retry_recovery_pass());
    assert_eq!(state.recovery_retry_count(), 2);
}
