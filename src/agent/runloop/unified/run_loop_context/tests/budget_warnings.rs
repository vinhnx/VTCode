use super::*;

#[test]
fn tool_budget_warning_system_message_matches_contract() {
    assert_eq!(
        ToolBudgetWarning { used: 3, max: 4, remaining: 1 }.system_message(),
        "Tool-call budget warning: 3/4 used; 1 remaining for this turn. Use targeted extraction/batching before additional tool calls."
    );
}
#[test]
fn harness_state_records_budget_exhaustion_notice_once_via_helper() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 10, 1);

    assert!(!state.tool_budget_exhausted());
    assert!(!state.tool_budget_exhausted_emitted);
    state.record_tool_call();
    assert!(state.tool_budget_exhausted());
    assert_eq!(
        state.record_tool_budget_exhaustion_notice(),
        Some(ToolBudgetExhaustionNotice {
            exhaustion: ToolBudgetExhaustion { used: 1, max: 1, remaining: 0 },
            first_notice: true,
        })
    );
    assert!(state.tool_budget_exhausted_emitted);
    assert_eq!(
        state.record_tool_budget_exhaustion_notice(),
        Some(ToolBudgetExhaustionNotice {
            exhaustion: ToolBudgetExhaustion { used: 1, max: 1, remaining: 0 },
            first_notice: false,
        })
    );
}
#[test]
fn tool_budget_exhaustion_synthesis_directive_matches_contract() {
    assert_eq!(
        ToolBudgetExhaustion { used: 4, max: 4, remaining: 0 }.synthesis_directive_message(),
        "Tool-call budget exhausted for this turn (4/4). Tools are disabled for the rest of this turn, so further tool calls are skipped. Synthesize your final answer now from the tool outputs already gathered in this conversation."
    );
}
#[test]
fn tool_budget_exhaustion_notice_arms_synthesis_directive_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 600, 3);
    state.record_tool_call();
    assert!(state.record_tool_budget_exhaustion_notice().is_some());
    assert!(state.take_tool_budget_directive_pending());
    // Second rejected call in the same turn must not re-arm the directive.
    assert!(state.record_tool_budget_exhaustion_notice().is_some());
    assert!(!state.take_tool_budget_directive_pending());
}
#[test]
fn tool_budget_rejection_is_separate_from_permission_denial() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 10, 1);

    state.record_tool_budget_rejection();
    assert!(state.take_tool_budget_rejection());
    assert!(!state.take_tool_budget_rejection());
    assert_eq!(state.snapshot_turn_diagnostics(Default::default(), 0).denied_tool_calls, 0);

    state.record_denied_tool_call();
    assert_eq!(state.snapshot_turn_diagnostics(Default::default(), 0).denied_tool_calls, 1);
}
#[test]
fn auto_permission_probe_warning_is_queued_once_until_flushed() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(state.queue_auto_permission_probe_warning("trusted warning".to_string()));
    assert!(!state.queue_auto_permission_probe_warning("duplicate warning".to_string()));
    assert_eq!(state.take_auto_permission_probe_warning().as_deref(), Some("trusted warning"));
    assert!(state.take_auto_permission_probe_warning().is_none());
}
#[test]
fn tool_wall_clock_exhaustion_policy_violation_message_matches_contract() {
    assert_eq!(
        ToolWallClockExhaustion { max_secs: 600 }.policy_violation_message(),
        "Policy violation: exceeded tool wall clock budget (600s)"
    );
}
#[test]
fn harness_state_treats_zero_tool_budget_as_unlimited() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 0, 10, 1);

    for _ in 0..8 {
        state.record_tool_call();
    }

    assert!(!state.has_tool_call_budget());
    assert!(!state.tool_budget_exhausted());
    assert_eq!(state.tool_budget_exhaustion(), None);
    assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
}
#[test]
fn harness_state_reports_wall_clock_budget_exhaustion() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.wall_clock_budget_exhaustion(), None);
    state.tool_wall_clock_elapsed = Duration::from_secs(11);
    assert_eq!(state.wall_clock_budget_exhaustion(), Some(ToolWallClockExhaustion { max_secs: 10 }));
}

#[test]
fn tool_wall_clock_accumulates_only_recorded_execution_intervals() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);
    state.record_tool_wall_clock(Duration::from_secs(4), state.tool_clock_checkpoint());
    state.record_tool_wall_clock(Duration::from_secs(5), state.tool_clock_checkpoint());
    assert!(!state.wall_clock_exhausted());
    state.record_tool_wall_clock(Duration::from_secs(1), state.tool_clock_checkpoint());
    assert!(state.wall_clock_exhausted());
}
#[test]
fn harness_state_excludes_active_external_wait_from_wall_clock_budget() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);
    let excluded_before = state.tool_clock_checkpoint();
    state.begin_budget_excluded_wait();
    state.wait_started_at = Instant::now().checked_sub(Duration::from_secs(11));
    state.end_budget_excluded_wait();
    state.record_tool_wall_clock(Duration::from_secs(12), excluded_before);
    assert_eq!(state.wall_clock_budget_exhaustion(), None);
    state.record_tool_wall_clock(Duration::from_secs(10), state.tool_clock_checkpoint());
    assert_eq!(state.wall_clock_budget_exhaustion(), Some(ToolWallClockExhaustion { max_secs: 10 }));
}
#[test]
fn harness_state_records_wall_clock_exhaustion_notice_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    // Not exhausted yet: no notice, no pending directive.
    assert_eq!(state.record_wall_clock_exhaustion_notice(), None);
    assert!(!state.take_wall_clock_directive_pending());

    // Simulate the wall-clock budget elapsing.
    state.tool_wall_clock_elapsed = Duration::from_secs(11);

    // First rejection: first_notice=true and arms the directive.
    assert_eq!(
        state.record_wall_clock_exhaustion_notice(),
        Some(ToolWallClockExhaustionNotice {
            exhaustion: ToolWallClockExhaustion { max_secs: 10 },
            first_notice: true,
        })
    );
    assert!(state.wall_clock_exhausted_emitted);

    // Subsequent rejections in the same batch: first_notice=false.
    assert_eq!(
        state.record_wall_clock_exhaustion_notice(),
        Some(ToolWallClockExhaustionNotice {
            exhaustion: ToolWallClockExhaustion { max_secs: 10 },
            first_notice: false,
        })
    );

    // The directive is consumed exactly once.
    assert!(state.take_wall_clock_directive_pending());
    assert!(!state.take_wall_clock_directive_pending());
}
#[test]
fn tool_wall_clock_exhaustion_directive_messages_match_contract() {
    let exhaustion = ToolWallClockExhaustion { max_secs: 600 };
    assert_eq!(exhaustion.skipped_call_message(), "Tool wall-clock budget exhausted for this turn; call skipped.");
    assert_eq!(
        exhaustion.synthesis_directive_message(),
        "Tool wall-clock budget exhausted for this turn (600s). Tools are disabled for the rest of this turn, so further tool calls are skipped. Synthesize your final answer now from the tool outputs already gathered in this conversation."
    );
}
