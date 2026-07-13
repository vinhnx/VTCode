//! Guard for repeated shell command runs.
//!
//! Tracks consecutive identical shell command runs per turn. When the cap is
//! reached, recovery is activated to prevent the model from stuck in a loop
//! of running the same command repeatedly.

use serde_json::Value;
use vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS;
use vtcode_core::tools::registry::labels::tool_action_label;

use super::super::ValidationResult;
use super::super::looping::shell_run_signature;
use super::common::push_guard_failure_messages;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Get the max consecutive identical shell command runs per turn from config.
fn max_consecutive_identical_shell_command_runs_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_repeated_tool_calls)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_REPEATED_TOOL_CALLS)
}

/// Build the error content for a repeated shell run guard trip.
#[cold]
fn build_repeated_shell_run_error_content(max_repeated_runs: usize) -> String {
    super::super::super::execution_result::build_error_content(
        format!(
            "Repeated identical shell command runs exceeded per-turn cap ({max_repeated_runs}). Reuse prior output or change command before retrying."
        ),
        None,
        None,
        "repeated_shell_run",
    )
    .to_string()
}

/// Enforce the repeated shell run guard.
///
/// Returns `Some(ValidationResult::Blocked)` when the guard trips,
/// or `None` when the guard passes.
pub(crate) fn enforce_repeated_shell_run_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    args: &Value,
) -> Option<ValidationResult> {
    let Some(signature) = shell_run_signature(canonical_tool_name, args) else {
        ctx.harness_state.reset_shell_command_run_streak();
        return None;
    };

    let max_repeated_runs = max_consecutive_identical_shell_command_runs_per_turn(ctx);
    let streak = ctx.harness_state.record_shell_command_run(signature);
    if streak <= max_repeated_runs {
        return None;
    }

    let display_tool = tool_action_label(canonical_tool_name, args);
    let block_reason = format!(
        "Repeated shell command guard stopped '{display_tool}' after {streak} identical runs (max {max_repeated_runs}). Scheduling a final recovery pass without more tools."
    );
    ctx.activate_recovery(block_reason.clone());
    push_guard_failure_messages(
        ctx,
        tool_call_id,
        canonical_tool_name,
        build_repeated_shell_run_error_content(max_repeated_runs),
        &block_reason,
    );

    Some(ValidationResult::Blocked)
}
