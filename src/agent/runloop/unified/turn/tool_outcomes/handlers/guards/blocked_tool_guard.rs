//! Guard for blocked tool calls.
//!
//! Tracks consecutive and total blocked tool calls per turn. When the cap is
//! reached, the turn is stopped to prevent retry churn.
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

/// Build the block reason and error content for a blocked tool call guard trip.
fn build_blocked_tool_call_messages(
    recovery_total_fuse_tripped: bool,
    normal_total_fuse_tripped: bool,
    max_streak: usize,
    display_tool: &str,
) -> (String, String) {
    let block_reason = if recovery_total_fuse_tripped {
        format!(
            "Blocked tool calls reached the recovery-mode cap ({max_streak}) for this turn. Last blocked call: '{display_tool}'. Stopping turn."
        )
    } else if normal_total_fuse_tripped {
        let max_total = max_streak * 2;
        format!(
            "Blocked tool calls reached per-turn cap ({max_total}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
        )
    } else {
        format!(
            "Consecutive blocked tool calls reached per-turn cap ({max_streak}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
        )
    };
    let error_msg = if recovery_total_fuse_tripped {
        format!("Blocked tool calls exceeded the recovery-mode cap ({max_streak}) for this turn.")
    } else if normal_total_fuse_tripped {
        let max_total = max_streak * 2;
        format!("Blocked tool calls exceeded cap ({max_total}) for this turn.")
    } else {
        format!("Consecutive blocked tool calls exceeded cap ({max_streak}) for this turn.")
    };
    let error_label = if recovery_total_fuse_tripped || normal_total_fuse_tripped {
        "blocked_total"
    } else {
        "blocked_streak"
    };
    let error_content = build_failure_error_content(error_msg, error_label);
    (block_reason, error_content)
}

/// Enforce the blocked tool call guard.
///
/// Returns `Some(TurnHandlerOutcome)` when the guard trips (turn should stop),
/// or `None` when the guard passes (continue processing).
pub(crate) fn enforce_blocked_tool_call_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
) -> Option<TurnHandlerOutcome> {
    let streak = ctx.record_blocked_tool_call();
    let blocked_total = ctx.blocked_tool_calls();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(ctx);

    if ctx.is_recovery_active() && !ctx.recovery_pass_used() {
        return Some(TurnHandlerOutcome::Continue);
    }

    let recovery_total_fuse_tripped = ctx.is_recovery_active() && blocked_total > max_streak;
    let normal_total_fuse_tripped = !ctx.is_recovery_active() && blocked_total > max_streak * 2;
    if streak <= max_streak && !recovery_total_fuse_tripped && !normal_total_fuse_tripped {
        return None;
    }

    let display_tool = tool_action_label(tool_name, args);
    let (block_reason, error_content) = build_blocked_tool_call_messages(
        recovery_total_fuse_tripped,
        normal_total_fuse_tripped,
        max_streak,
        &display_tool,
    );
    push_guard_failure_messages(ctx, tool_call_id, tool_name, error_content, &block_reason);

    Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(block_reason) }))
}
