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

pub(super) fn should_attempt_requesting_timeout_recovery(
    timed_out_phase: TurnPhase,
    had_tool_activity: bool,
    recovery_already_attempted: bool,
) -> bool {
    had_tool_activity
        && matches!(timed_out_phase, TurnPhase::Requesting)
        && !recovery_already_attempted
}

pub(super) fn build_partial_timeout_messages(
    timeout_secs: u64,
    timed_out_phase: TurnPhase,
    attempted_tool_calls: usize,
    active_pty_sessions_before_cancel: usize,
    continuing_with_recovery: bool,
) -> (String, String) {
    let timeout_note = if continuing_with_recovery {
        "Tool activity exists and timeout occurred during LLM requesting; continuing with a compacted tool-free recovery pass that reuses existing tool outputs.".to_string()
    } else if matches!(timed_out_phase, TurnPhase::Requesting) {
        "Tool activity exists and timeout occurred during LLM requesting; retry is skipped to avoid re-running tools.".to_string()
    } else {
        "Tool activity was detected in this attempt; retry is skipped to avoid duplicate execution."
            .to_string()
    };

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
    if continuing_with_recovery {
        error_message.push_str(
            " Continuing with a compacted tool-free recovery pass that reuses existing tool outputs.",
        );
    }

    (renderer_message, error_message)
}
