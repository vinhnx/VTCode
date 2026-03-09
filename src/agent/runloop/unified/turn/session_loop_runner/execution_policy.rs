use crate::agent::runloop::unified::run_loop_context::TurnPhase;

const PLAN_MODE_MIN_TOOL_CALLS_PER_TURN: usize = 48;

pub(super) fn resolve_effective_turn_timeout_secs(
    configured_turn_timeout_secs: u64,
    max_tool_wall_clock_secs: u64,
) -> u64 {
    // Keep turn timeout aligned with harness wall-clock budget to avoid aborting
    // valid long-running tool+request cycles mid-turn.
    //
    // The buffer must cover at least one LLM-attempt timeout window so a turn that
    // reaches the harness wall-clock budget can still complete its in-flight request.
    // Keep this formula aligned with turn_processing::llm_request::llm_attempt_timeout_secs.
    let llm_attempt_grace_secs = (configured_turn_timeout_secs / 5).clamp(30, 120);
    let buffer_secs = 60_u64.max(llm_attempt_grace_secs);
    let min_for_harness = max_tool_wall_clock_secs.saturating_add(buffer_secs);
    configured_turn_timeout_secs.max(min_for_harness)
}

pub(super) fn effective_max_tool_calls_for_turn(
    configured_limit: usize,
    plan_mode_active: bool,
) -> usize {
    if configured_limit == 0 {
        0
    } else if plan_mode_active {
        configured_limit.max(PLAN_MODE_MIN_TOOL_CALLS_PER_TURN)
    } else {
        configured_limit
    }
}

pub(super) fn build_partial_timeout_messages(
    timeout_secs: u64,
    timed_out_phase: TurnPhase,
    attempted_tool_calls: usize,
    active_pty_sessions_before_cancel: usize,
    plan_mode_active: bool,
    had_tool_activity: bool,
) -> (String, String) {
    let timed_out_during_request = matches!(timed_out_phase, TurnPhase::Requesting);
    let mut timeout_note = if timed_out_during_request {
        "Tool activity exists and timeout occurred during LLM requesting; retry is skipped to avoid re-running tools.".to_string()
    } else {
        "Tool activity was detected in this attempt; retry is skipped to avoid duplicate execution."
            .to_string()
    };

    let include_autonomous_recovery_note =
        plan_mode_active && had_tool_activity && timed_out_during_request;
    if include_autonomous_recovery_note {
        timeout_note
            .push_str(" Autonomous recovery will retry with an adjusted strategy when possible.");
    }

    let renderer_message = format!(
        "Turn timed out after {} seconds in phase {:?}. PTY sessions cancelled; {} (calls={}, active_pty_before_cancel={})",
        timeout_secs,
        timed_out_phase,
        timeout_note,
        attempted_tool_calls,
        active_pty_sessions_before_cancel
    );

    let mut error_message = format!(
        "Turn timed out after {} seconds in phase {:?} after partial tool execution (calls={}, active_pty_before_cancel={})",
        timeout_secs, timed_out_phase, attempted_tool_calls, active_pty_sessions_before_cancel
    );
    if include_autonomous_recovery_note {
        error_message
            .push_str(" Autonomous recovery will retry with an adjusted strategy when possible.");
    }

    (renderer_message, error_message)
}
