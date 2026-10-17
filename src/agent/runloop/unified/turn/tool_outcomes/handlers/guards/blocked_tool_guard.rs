//! Guard for blocked tool calls.
//!
//! Tracks consecutive and total blocked tool calls per turn. When the cap is
//! reached, the current batch is closed and one bounded tool-free recovery
//! pass is scheduled to prevent retry churn without dropping the turn.
//!
//! The guard has two thresholds:
//! - **Consecutive cap**: Stops the turn after N consecutive blocked calls
//! - **Total fuse**: Stops the turn after M total blocked calls (even if not consecutive)
//!
//! Recovery mode uses a tighter total fuse than normal mode.

use serde_json::Value;
use vtcode_core::config::constants::defaults::DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN;
use vtcode_core::tools::registry::labels::tool_action_label;

use super::super::build_failure_error_content;
use super::common::push_guard_failure_messages;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext};

/// Get the max consecutive blocked tool calls per turn from config.
pub(crate) fn max_consecutive_blocked_tool_calls_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_consecutive_blocked_tool_calls_per_turn)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN)
}

/// Per-tool consecutive cap override from `tools.blocked_tool_thresholds`.
///
/// Read-only tools tolerate more denies than mutating tools by default when no
/// explicit override exists: `code_search` gets 2x headroom so exploratory
/// denies do not trip the fuse as fast as repeated `exec_command` denies.
pub(crate) fn consecutive_cap_for_tool(ctx: &TurnProcessingContext<'_>, tool_name: &str) -> usize {
    let base = max_consecutive_blocked_tool_calls_per_turn(ctx);
    if let Some(cfg) = ctx.vt_cfg
        && let Some(override_cap) = cfg.tools.blocked_tool_thresholds.get(tool_name)
        && *override_cap > 0
    {
        return *override_cap;
    }
    if tool_name == vtcode_core::config::constants::tools::CODE_SEARCH {
        return base.saturating_mul(2).max(base);
    }
    base
}

/// Actionable remedy hint per tool so the model and user know what to change
/// instead of retrying the same denied call.
pub(crate) fn remedy_hint_for_tool(tool_name: &str) -> &'static str {
    match tool_name {
        vtcode_core::config::constants::tools::EXEC_COMMAND => {
            "Check sandbox/approval policy, narrow the command, or request approval instead of retrying verbatim."
        }
        vtcode_core::config::constants::tools::CODE_SEARCH => {
            "Narrow code_search filters (omit empty path, use specific query) instead of retrying the same search."
        }
        vtcode_core::config::constants::tools::WRITE_STDIN => {
            "Verify the target session exists and is writable before retrying write_stdin."
        }
        vtcode_core::config::constants::tools::APPLY_PATCH => {
            "Rebase the patch on current file contents and confirm edit approval before retrying."
        }
        _ => "Adjust arguments, policy, or approvals instead of retrying the identical call.",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlockedToolCallLimits {
    pub(crate) consecutive_cap: usize,
    pub(crate) total_cap: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlockedToolCallFuseTrip {
    Consecutive { cap: usize },
    Total { cap: usize },
}

impl BlockedToolCallFuseTrip {
    pub(crate) const fn cap(self) -> usize {
        match self {
            Self::Consecutive { cap } | Self::Total { cap } => cap,
        }
    }

    pub(crate) const fn metric(self) -> &'static str {
        match self {
            Self::Consecutive { .. } => "blocked_streak_break",
            Self::Total { .. } => "blocked_total_break",
        }
    }
}

pub(crate) fn blocked_tool_call_limits(ctx: &TurnProcessingContext<'_>) -> BlockedToolCallLimits {
    blocked_tool_call_limits_for_tool(ctx, "")
}

pub(crate) fn blocked_tool_call_limits_for_tool(
    ctx: &TurnProcessingContext<'_>,
    tool_name: &str,
) -> BlockedToolCallLimits {
    let consecutive_cap = if tool_name.is_empty() {
        max_consecutive_blocked_tool_calls_per_turn(ctx)
    } else {
        consecutive_cap_for_tool(ctx, tool_name)
    };
    let explicit_total = ctx
        .vt_cfg
        .and_then(|cfg| cfg.tools.max_total_blocked_tool_calls_per_turn)
        .filter(|value| *value > 0);
    let total_cap = if let Some(explicit) = explicit_total {
        explicit
    } else if ctx.is_recovery_active() {
        consecutive_cap
    } else if ctx.is_planning_active() {
        consecutive_cap.saturating_mul(4)
    } else {
        consecutive_cap.saturating_mul(2)
    };

    BlockedToolCallLimits { consecutive_cap, total_cap }
}

/// Advisory warning when exactly one attempt remains before the fuse trips.
/// Returns the warning text so callers can surface it without tripping recovery.
pub(crate) fn blocked_tool_call_advisory(
    consecutive_blocked_tool_calls: usize,
    blocked_tool_calls: usize,
    limits: BlockedToolCallLimits,
    tool_name: &str,
) -> Option<String> {
    let consecutive_left = limits.consecutive_cap.saturating_sub(consecutive_blocked_tool_calls);
    let total_left = limits.total_cap.saturating_sub(blocked_tool_calls);
    if consecutive_left == 1 || total_left == 1 {
        let remedy = remedy_hint_for_tool(tool_name);
        return Some(format!(
            "Warning: 1 blocked attempt left (streak {consecutive_blocked_tool_calls}/{consecutive} total {blocked_tool_calls}/{total}) for '{tool_name}'. {remedy}",
            consecutive = limits.consecutive_cap,
            total = limits.total_cap,
        ));
    }
    None
}

pub(crate) fn blocked_tool_call_fuse_trip(
    consecutive_blocked_tool_calls: usize,
    blocked_tool_calls: usize,
    limits: BlockedToolCallLimits,
) -> Option<BlockedToolCallFuseTrip> {
    if blocked_tool_calls > limits.total_cap {
        Some(BlockedToolCallFuseTrip::Total { cap: limits.total_cap })
    } else if consecutive_blocked_tool_calls > limits.consecutive_cap {
        Some(BlockedToolCallFuseTrip::Consecutive { cap: limits.consecutive_cap })
    } else {
        None
    }
}

/// Build the block reason and error content for a blocked tool call fuse trip.
#[allow(
    dead_code,
    reason = "Intentional compat wrapper; production paths use detailed variant with counters/remedy."
)]
pub(crate) fn blocked_tool_call_messages(
    fuse_trip: BlockedToolCallFuseTrip,
    recovery_mode: bool,
    display_tool: &str,
) -> (String, String) {
    blocked_tool_call_messages_detailed(fuse_trip, recovery_mode, display_tool, 0, 0, "")
}

/// Detailed fuse message with streak/total counters, per-tool remedy, and
/// resume guidance so both model and user know what to do next.
pub(crate) fn blocked_tool_call_messages_detailed(
    fuse_trip: BlockedToolCallFuseTrip,
    recovery_mode: bool,
    display_tool: &str,
    streak: usize,
    total: usize,
    tool_name: &str,
) -> (String, String) {
    let remedy = remedy_hint_for_tool(tool_name);
    let counters = if streak > 0 || total > 0 {
        format!(" (streak {streak}, total {total})")
    } else {
        String::new()
    };
    let resume_guidance = "History and outputs are retained. Type 'continue' with new guidance, or run `vtcode --resume <session>`; details: .vtcode/tasks/current_blocked.md.";
    let (block_reason, error_msg, error_label) = match (fuse_trip, recovery_mode) {
        (BlockedToolCallFuseTrip::Total { cap }, true) => (
            format!(
                "Recovery tool-call limit reached after {cap} blocked calls{counters} (last blocked call: '{display_tool}'). {remedy} {resume_guidance}"
            ),
            format!(
                "The recovery tool-call limit of {cap} blocked calls was exceeded for this turn.{counters} {remedy}"
            ),
            "blocked_total",
        ),
        (BlockedToolCallFuseTrip::Total { cap }, false) => (
            format!(
                "Blocked tool-call limit reached after {cap} total blocked calls this turn{counters}. Last blocked call: '{display_tool}'. {remedy} A bounded recovery response will run without more tool calls. {resume_guidance}"
            ),
            format!("The tool-call safety limit of {cap} blocked calls was exceeded for this turn.{counters} {remedy}"),
            "blocked_total",
        ),
        (BlockedToolCallFuseTrip::Consecutive { cap }, _) => (
            format!(
                "Blocked tool-call limit reached after {cap} consecutive blocked calls{counters}. Last blocked call: '{display_tool}'. {remedy} A bounded recovery response will run without more tool calls. {resume_guidance}"
            ),
            format!(
                "The tool-call safety limit of {cap} consecutive blocked calls was exceeded for this turn.{counters} {remedy}"
            ),
            "blocked_streak",
        ),
    };
    let error_content = build_failure_error_content(error_msg, error_label);
    (block_reason, error_content)
}

/// Enforce the blocked tool call guard.
///
/// Returns `Some(TurnHandlerOutcome)` when the guard trips, or `None` when the
/// guard passes. A fuse trip returns `Continue` once so the caller can flush
/// the current tool responses and schedule bounded recovery.
pub(crate) fn enforce_blocked_tool_call_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
) -> Option<TurnHandlerOutcome> {
    enforce_blocked_tool_call_guard_with_scope(ctx, tool_call_id, tool_name, args, false)
}

pub(crate) fn enforce_read_cap_blocked_tool_call_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
) -> Option<TurnHandlerOutcome> {
    enforce_blocked_tool_call_guard_with_scope(ctx, tool_call_id, tool_name, args, true)
}

fn enforce_blocked_tool_call_guard_with_scope(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
    path_local: bool,
) -> Option<TurnHandlerOutcome> {
    let streak = if path_local {
        ctx.harness_state.record_blocked_tool_call_streak()
    } else {
        ctx.record_blocked_tool_call()
    };
    let blocked_total = ctx.blocked_tool_calls();
    let limits = blocked_tool_call_limits_for_tool(ctx, tool_name);

    if ctx.is_recovery_active() && !ctx.recovery_pass_used() {
        return Some(TurnHandlerOutcome::Continue);
    }

    if let Some(advisory) = blocked_tool_call_advisory(streak, blocked_total, limits, tool_name)
        && blocked_tool_call_fuse_trip(streak, blocked_total, limits).is_none()
    {
        ctx.push_system_message(advisory);
    }

    let fuse_trip = blocked_tool_call_fuse_trip(streak, blocked_total, limits)?;
    let display_tool = tool_action_label(tool_name, args);
    let recovery_active = ctx.is_recovery_active();
    // Captured before arming/breaking so `finalize_turn` can populate the
    // `TurnBlockedEvent` fields instead of `None`/zeros.
    let telemetry = crate::agent::runloop::unified::run_loop_context::BlockedToolRecoveryTelemetry {
        last_tool: display_tool.to_string(),
        consecutive_cap: limits.consecutive_cap,
        total_cap: limits.total_cap,
        blocked_streak: streak,
        blocked_total,
    };
    let (block_reason, error_content) = blocked_tool_call_messages_detailed(
        fuse_trip,
        recovery_active,
        &display_tool,
        streak,
        blocked_total,
        tool_name,
    );

    // Clear any inline loading placeholder and restore the input status so
    // the UI doesn't remain in a stale "loading" or shimmer state after the
    // guard trips.
    ctx.reset_input_to_default_placeholder();
    ctx.restore_input_status(None, None);

    if !recovery_active {
        // Keep the current tool response contiguous with any later responses
        // in the assistant batch. The recovery directive is appended only
        // after the caller drains those remaining calls.
        ctx.push_tool_response(tool_call_id, Some(tool_name), error_content);
        ctx.harness_state.arm_blocked_tool_recovery(block_reason, telemetry);
        Some(TurnHandlerOutcome::Continue)
    } else {
        ctx.harness_state.record_blocked_tool_recovery_telemetry(telemetry);
        push_guard_failure_messages(ctx, tool_call_id, tool_name, error_content, &block_reason);
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(block_reason) }))
    }
}

#[cfg(test)]
mod tests {
    use super::{blocked_tool_call_advisory, blocked_tool_call_messages_detailed};

    use super::{BlockedToolCallFuseTrip, BlockedToolCallLimits};

    #[test]
    fn advisory_warns_when_one_attempt_remains() {
        let limits = BlockedToolCallLimits { consecutive_cap: 3, total_cap: 6 };
        let advisory = blocked_tool_call_advisory(2, 2, limits, "exec_command");
        assert!(advisory.is_some());
        let text = advisory.expect("advisory");
        assert!(text.contains("1 blocked attempt left"));
        assert!(text.contains("exec_command"));

        assert!(blocked_tool_call_advisory(1, 1, limits, "exec_command").is_none());
        assert!(blocked_tool_call_advisory(3, 3, limits, "exec_command").is_none());
    }

    #[test]
    fn detailed_message_includes_counters_remedy_and_resume() {
        let (reason, _) = blocked_tool_call_messages_detailed(
            BlockedToolCallFuseTrip::Consecutive { cap: 3 },
            false,
            "exec_command 'ls'",
            4,
            4,
            "exec_command",
        );
        assert!(reason.contains("3 consecutive blocked calls"));
        assert!(reason.contains("streak 4"));
        assert!(reason.contains("sandbox/approval"));
        assert!(reason.contains("Type 'continue'"));
        assert!(reason.contains(".vtcode/tasks/current_blocked.md"));
    }
}
