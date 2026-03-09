use serde_json::{Value, json};
use vtcode_core::config::constants::defaults::{
    DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN, DEFAULT_MAX_REPEATED_TOOL_CALLS,
    DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN,
};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::labels::tool_action_label;

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use super::super::helpers::push_tool_response;
use super::looping::{shell_run_signature, spool_chunk_read_path, task_tracker_create_signature};
use super::{ValidationResult, build_failure_error_content};

pub(crate) fn max_consecutive_blocked_tool_calls_per_turn(
    ctx: &TurnProcessingContext<'_>,
) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_consecutive_blocked_tool_calls_per_turn)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN)
}

pub(super) fn enforce_blocked_tool_call_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
) -> Option<TurnHandlerOutcome> {
    let streak = {
        let parts = ctx.parts_mut();
        parts.state.harness_state.record_blocked_tool_call()
    };
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(ctx);
    if streak <= max_streak {
        return None;
    }

    let display_tool = tool_action_label(tool_name, args);
    let block_reason = format!(
        "Consecutive blocked tool calls reached per-turn cap ({max_streak}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
    );
    {
        let parts = ctx.parts_mut();
        push_tool_response(
            parts.state.working_history,
            tool_call_id.to_string(),
            build_failure_error_content(
                format!(
                    "Consecutive blocked tool calls exceeded cap ({max_streak}) for this turn."
                ),
                "blocked_streak",
            ),
        );
        parts
            .state
            .working_history
            .push(vtcode_core::llm::provider::Message::system(
                block_reason.clone(),
            ));
    }

    Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
        reason: Some(block_reason),
    }))
}

fn max_consecutive_identical_shell_command_runs_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_repeated_tool_calls)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_REPEATED_TOOL_CALLS)
}

fn build_repeated_shell_run_error_content(max_repeated_runs: usize) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "Repeated identical shell command runs exceeded per-turn cap ({}). Reuse prior output or change command before retrying.",
            max_repeated_runs
        ),
        None,
        None,
        "repeated_shell_run",
    )
    .to_string()
}

pub(super) fn enforce_duplicate_task_tracker_create_guard<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &Value,
) -> Option<ValidationResult> {
    let signature = task_tracker_create_signature(canonical_tool_name, effective_args)?;

    if ctx
        .harness_state
        .record_task_tracker_create_signature(signature)
    {
        return None;
    }

    let content = super::super::execution_result::build_error_content(
        "Duplicate task_tracker.create detected in this turn. Use task_tracker.update/list to continue tracking progress."
            .to_string(),
        Some(tool_names::TASK_TRACKER.to_string()),
        Some(serde_json::json!({ "action": "list" })),
        "duplicate_task_tracker_create",
    )
    .to_string();
    {
        let parts = ctx.parts_mut();
        push_tool_response(
            parts.state.working_history,
            tool_call_id.to_string(),
            content,
        );
    }
    let block_reason =
        "Blocked duplicate task_tracker.create in the same turn. Continue with task_tracker.update/list."
            .to_string();

    Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
        TurnLoopResult::Blocked {
            reason: Some(block_reason),
        },
    )))
}

pub(super) fn enforce_repeated_shell_run_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    args: &Value,
) -> Option<ValidationResult> {
    let Some(signature) = shell_run_signature(canonical_tool_name, args) else {
        let parts = ctx.parts_mut();
        parts.state.harness_state.reset_shell_command_run_streak();
        return None;
    };

    let max_repeated_runs = max_consecutive_identical_shell_command_runs_per_turn(ctx);
    let streak = {
        let parts = ctx.parts_mut();
        parts
            .state
            .harness_state
            .record_shell_command_run(signature)
    };
    if streak <= max_repeated_runs {
        return None;
    }

    let display_tool = tool_action_label(canonical_tool_name, args);
    let block_reason = format!(
        "Repeated shell command guard stopped '{}' after {} identical runs (max {}). Reuse prior output or change the command.",
        display_tool, streak, max_repeated_runs
    );
    {
        let parts = ctx.parts_mut();
        push_tool_response(
            parts.state.working_history,
            tool_call_id.to_string(),
            build_repeated_shell_run_error_content(max_repeated_runs),
        );
    }

    Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
        TurnLoopResult::Blocked {
            reason: Some(block_reason),
        },
    )))
}

fn max_sequential_spool_chunk_reads_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_sequential_spool_chunk_reads)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN)
}

fn build_spool_chunk_guard_error_content(path: &str, max_reads_per_turn: usize) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "Spool chunk reads exceeded per-turn cap ({}). Use targeted extraction before reading more from '{}'.",
            max_reads_per_turn, path
        ),
        Some("grep_file".to_string()),
        Some(json!({
            "path": path,
            "pattern": "warning|error|TODO"
        })),
        "spool_chunk_guard",
    )
    .to_string()
}

pub(super) fn enforce_spool_chunk_read_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    args: &Value,
) -> Option<ValidationResult> {
    let Some(spool_path) = spool_chunk_read_path(canonical_tool_name, args) else {
        let parts = ctx.parts_mut();
        parts.state.harness_state.reset_spool_chunk_read_streak();
        return None;
    };

    let max_reads_per_turn = max_sequential_spool_chunk_reads_per_turn(ctx);
    let streak = {
        let parts = ctx.parts_mut();
        parts.state.harness_state.record_spool_chunk_read()
    };
    if streak <= max_reads_per_turn {
        return None;
    }

    let display_tool = tool_action_label(canonical_tool_name, args);
    let block_reason = format!(
        "Spool chunk guard stopped repeated '{}' calls for this turn.\nUse `grep_file` or summarize before more chunk reads.",
        display_tool
    );

    {
        let parts = ctx.parts_mut();
        push_tool_response(
            parts.state.working_history,
            tool_call_id.to_string(),
            build_spool_chunk_guard_error_content(spool_path, max_reads_per_turn),
        );
    }

    Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
        TurnLoopResult::Blocked {
            reason: Some(block_reason),
        },
    )))
}
