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
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::signature_key_for;

use super::looping::{
    low_signal_family_key, shell_run_signature, spool_chunk_read_path,
    task_tracker_create_signature,
};
use super::{ValidationResult, build_failure_error_content};

const SPOOL_CHUNK_GREP_PATTERN: &str = "warning|error|TODO";
const MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS: usize = 4;

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
    let streak = ctx.record_blocked_tool_call();
    let blocked_total = ctx.blocked_tool_calls();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(ctx);

    if ctx.is_recovery_active() && !ctx.recovery_pass_used() {
        return Some(TurnHandlerOutcome::Continue);
    }

    let recovery_total_fuse_tripped = ctx.is_recovery_active() && blocked_total > max_streak;
    if streak <= max_streak && !recovery_total_fuse_tripped {
        return None;
    }

    let display_tool = tool_action_label(tool_name, args);
    let block_reason = if recovery_total_fuse_tripped {
        format!(
            "Blocked tool calls reached the recovery-mode cap ({max_streak}) for this turn. Last blocked call: '{display_tool}'. Stopping turn."
        )
    } else {
        format!(
            "Consecutive blocked tool calls reached per-turn cap ({max_streak}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
        )
    };
    ctx.push_tool_response(
        tool_call_id,
        build_failure_error_content(
            if recovery_total_fuse_tripped {
                format!("Blocked tool calls exceeded the recovery-mode cap ({max_streak}) for this turn.")
            } else {
                format!("Consecutive blocked tool calls exceeded cap ({max_streak}) for this turn.")
            },
            if recovery_total_fuse_tripped {
                "blocked_total"
            } else {
                "blocked_streak"
            },
        ),
    );
    ctx.push_system_message(block_reason.clone());

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

fn repeated_file_read_family_key(canonical_tool_name: &str, args: &Value) -> Option<String> {
    if spool_chunk_read_path(canonical_tool_name, args).is_some() {
        return None;
    }

    match canonical_tool_name {
        tool_names::READ_FILE | tool_names::UNIFIED_FILE => {
            low_signal_family_key(canonical_tool_name, args)
        }
        _ => None,
    }
}

fn build_repeated_file_read_family_error_content(target: &str) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "Repeated reads of '{}' with limited progress detected. Reuse the collected output, summarize findings, or narrow the range before reading again.",
            target
        ),
        None,
        None,
        "repeated_read_family",
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
    ctx.push_tool_response(tool_call_id, content);
    Some(ValidationResult::Blocked)
}

pub(super) fn enforce_repeated_read_only_call_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &Value,
    readonly_classification: bool,
) -> Option<ValidationResult> {
    if !readonly_classification {
        return None;
    }

    if let Some(family_key) = repeated_file_read_family_key(canonical_tool_name, effective_args) {
        let streak = ctx
            .harness_state
            .record_file_read_family_call(family_key.clone());
        if streak >= MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS {
            let target = family_key.rsplit("::").next().unwrap_or("current file");
            let block_reason = format!(
                "Repeated read-only exploration of '{}' hit the per-turn family cap ({}). Scheduling a final recovery pass without more tools.",
                target, MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS
            );
            ctx.activate_recovery(block_reason.clone());
            ctx.push_tool_response(
                tool_call_id,
                build_repeated_file_read_family_error_content(target),
            );
            ctx.push_system_message(block_reason);
            return Some(ValidationResult::Blocked);
        }
    }

    let signature = signature_key_for(canonical_tool_name, effective_args);
    if !ctx
        .harness_state
        .has_successful_readonly_signature(signature.as_str())
    {
        return None;
    }

    let mut reused_value = ctx.tool_registry.find_recent_successful_output(
        canonical_tool_name,
        effective_args,
        ctx.harness_state.max_tool_wall_clock,
    )?;
    let obj = reused_value.as_object_mut()?;
    super::apply_reused_read_only_loop_metadata(obj);
    ctx.push_tool_response(tool_call_id, reused_value.to_string());

    Some(ValidationResult::Handled)
}

pub(super) fn enforce_repeated_shell_run_guard(
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
        "Repeated shell command guard stopped '{}' after {} identical runs (max {}). Scheduling a final recovery pass without more tools.",
        display_tool, streak, max_repeated_runs
    );
    ctx.activate_recovery(block_reason.clone());
    ctx.push_tool_response(
        tool_call_id,
        build_repeated_shell_run_error_content(max_repeated_runs),
    );
    ctx.push_system_message(block_reason);

    Some(ValidationResult::Blocked)
}

fn max_sequential_spool_chunk_reads_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_sequential_spool_chunk_reads)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN)
}

fn spool_chunk_guard_fallback_args(path: &str) -> Value {
    json!({
        "action": "grep",
        "path": path,
        "pattern": SPOOL_CHUNK_GREP_PATTERN
    })
}

fn build_spool_chunk_guard_error_content(path: &str, max_reads_per_turn: usize) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "Spool chunk reads exceeded per-turn cap ({}). Use targeted extraction before reading more from '{}'.",
            max_reads_per_turn, path
        ),
        Some(tool_names::UNIFIED_SEARCH.to_string()),
        Some(spool_chunk_guard_fallback_args(path)),
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
        ctx.harness_state.reset_spool_chunk_read_streak();
        return None;
    };

    let max_reads_per_turn = max_sequential_spool_chunk_reads_per_turn(ctx);
    let streak = ctx.harness_state.record_spool_chunk_read();
    if streak <= max_reads_per_turn {
        return None;
    }

    let display_tool = tool_action_label(canonical_tool_name, args);
    let block_reason = format!(
        "Spool chunk guard stopped repeated '{}' calls for this turn. Scheduling a final recovery pass without more tools.",
        display_tool
    );

    ctx.activate_recovery(block_reason.clone());
    ctx.push_tool_response(
        tool_call_id,
        build_spool_chunk_guard_error_content(spool_path, max_reads_per_turn),
    );
    ctx.push_system_message(block_reason);

    Some(ValidationResult::Blocked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spool_chunk_guard_error_uses_unified_search_fallback() {
        let payload =
            build_spool_chunk_guard_error_content(".vtcode/context/tool_outputs/run-1.txt", 3);
        let parsed: Value =
            serde_json::from_str(&payload).expect("spool chunk guard payload should be json");

        assert_eq!(
            parsed.get("fallback_tool").and_then(Value::as_str),
            Some(tool_names::UNIFIED_SEARCH)
        );
        assert_eq!(parsed["fallback_tool_args"]["action"], "grep");
        assert_eq!(
            parsed["fallback_tool_args"]["path"],
            ".vtcode/context/tool_outputs/run-1.txt"
        );
        assert_eq!(
            parsed["fallback_tool_args"]["pattern"],
            SPOOL_CHUNK_GREP_PATTERN
        );
        assert!(parsed.get("next_action").and_then(Value::as_str).is_some());
    }
}
