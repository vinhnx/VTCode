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
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
    find_duplicate_in_history, signature_key_for,
};

use super::looping::{
    low_signal_family_key, shell_run_signature, spool_chunk_read_path,
    task_tracker_create_signature,
};
use super::{ValidationResult, build_failure_error_content};

const SPOOL_CHUNK_GREP_PATTERN: &str = "warning|error|TODO";
const MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS: usize = 4;

#[cold]
fn push_guard_failure_messages(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    error_content: String,
    block_reason: &str,
) {
    ctx.push_tool_response(tool_call_id, error_content);
    ctx.push_system_message(block_reason.to_string());
}

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
    push_guard_failure_messages(
        ctx,
        tool_call_id,
        build_failure_error_content(
            if recovery_total_fuse_tripped {
                format!(
                    "Blocked tool calls exceeded the recovery-mode cap ({max_streak}) for this turn."
                )
            } else {
                format!("Consecutive blocked tool calls exceeded cap ({max_streak}) for this turn.")
            },
            if recovery_total_fuse_tripped {
                "blocked_total"
            } else {
                "blocked_streak"
            },
        ),
        &block_reason,
    );

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

#[cold]
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
        tool_names::UNIFIED_EXEC => {
            // Track file-reading shell commands in the family guard to prevent
            // bypass via unified_exec. Only commands on the is_readonly_unified_exec_command
            // allowlist (tool_intent.rs) reach this point — cat, head, tail, bat.
            // less/more are not on that allowlist so they get readonly_classification=false
            // and are caught by enforce_repeated_shell_run_guard instead.
            let parts = vtcode_core::tools::command_args::command_words(args).ok()??;
            let command_name = parts.first()?.as_str();
            if !matches!(command_name, "cat" | "head" | "tail" | "bat") {
                return None;
            }
            // Use the full command as the family key so different files are tracked separately
            let command_str = parts.join(" ");
            Some(format!("{}::run::{}", canonical_tool_name, command_str))
        }
        _ => None,
    }
}

#[cold]
fn build_repeated_file_read_family_error_content(target: &str) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "File '{}' already read. Content is in conversation history above. Synthesize your answer from existing data. Do NOT re-read.",
            target
        ),
        None,
        None,
        "repeated_read_family",
    )
    .to_string()
}

fn is_read_action(canonical_tool_name: &str, args: &Value) -> bool {
    match canonical_tool_name {
        tool_names::READ_FILE => true,
        tool_names::UNIFIED_FILE => {
            let action = args.get("action").and_then(Value::as_str).unwrap_or("read");
            action.eq_ignore_ascii_case("read")
        }
        _ => false,
    }
}

fn extract_read_path(args: &Value) -> Option<String> {
    args.get("path")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

#[cold]
fn build_read_after_write_error(path: &str) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "File '{}' was just written in this turn. The write response includes a diff preview. Reuse the diff output or specify offset/limit for a specific range.",
            path
        ),
        None,
        None,
        "read_after_write",
    )
    .to_string()
}

pub(super) fn enforce_read_after_write_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &Value,
) -> Option<ValidationResult> {
    if !is_read_action(canonical_tool_name, effective_args) {
        return None;
    }

    let path = extract_read_path(effective_args)?;

    if !ctx.harness_state.was_recently_written(&path) {
        return None;
    }

    let content = build_read_after_write_error(&path);
    ctx.push_tool_response(tool_call_id, content);
    Some(ValidationResult::Blocked)
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
            push_guard_failure_messages(
                ctx,
                tool_call_id,
                build_repeated_file_read_family_error_content(target),
                &block_reason,
            );
            return Some(ValidationResult::Blocked);
        }
    }

    let signature = signature_key_for(canonical_tool_name, effective_args);
    if ctx
        .harness_state
        .has_successful_readonly_signature(signature.as_str())
    {
        // Same-turn duplicate: use the registry's cached output (has TTL)
        if let Some(mut reused_value) = ctx.tool_registry.find_recent_successful_output(
            canonical_tool_name,
            effective_args,
            ctx.harness_state.max_tool_wall_clock,
        ) {
            if let Some(obj) = reused_value.as_object_mut() {
                super::apply_reused_read_only_loop_metadata(obj);
            }
            ctx.push_tool_response(tool_call_id, reused_value.to_string());
            return Some(ValidationResult::Handled);
        }
    }

    // Cross-turn TTL-bounded cache: consult the registry's execution history
    // even when the per-turn signature set is empty (e.g. after a "continue"
    // turn that creates a fresh HarnessTurnState).  The TTL is bounded by
    // `max_tool_wall_clock` (600s by default), so stale results naturally
    // expire.  This prevents 5× re-reads of the same file across turns when
    // the model varies pagination arguments slightly.
    //
    // Uses path-based matching (`find_recent_successful_by_read_target`) which
    // ignores offset/limit/page fields.  The exact-arg cache
    // (`find_recent_successful_output`) won't match when pagination differs,
    // so the path-based lookup is the primary cross-turn dedup mechanism.
    if let Some(mut reused_value) = ctx.tool_registry.find_recent_successful_by_read_target(
        canonical_tool_name,
        effective_args,
        ctx.harness_state.max_tool_wall_clock,
    ) {
        if let Some(obj) = reused_value.as_object_mut() {
            super::apply_reused_read_only_loop_metadata(obj);
        }
        ctx.push_tool_response(tool_call_id, reused_value.to_string());
        // Also register in the per-turn set so subsequent in-turn calls
        // short-circuit at the top of this function.
        ctx.harness_state
            .record_successful_readonly_signature(signature);
        return Some(ValidationResult::Handled);
    }

    // Cross-turn duplicate: scan working history for an identical readonly call
    // from a previous turn. Produces the same structured reused_recent_result
    // payload so the model recognizes the signal to stop retrying.
    if let Some(raw_output) =
        find_duplicate_in_history(ctx.working_history, canonical_tool_name, effective_args)
    {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&raw_output) {
            if let Some(obj) = parsed.as_object_mut() {
                super::apply_reused_read_only_loop_metadata(obj);
            }
            ctx.push_tool_response(tool_call_id, parsed.to_string());
        } else {
            ctx.push_tool_response(tool_call_id, raw_output);
        }
        return Some(ValidationResult::Handled);
    }

    None
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
    push_guard_failure_messages(
        ctx,
        tool_call_id,
        build_repeated_shell_run_error_content(max_repeated_runs),
        &block_reason,
    );

    Some(ValidationResult::Blocked)
}

fn max_sequential_spool_chunk_reads_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_sequential_spool_chunk_reads)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN)
}

#[cold]
fn spool_chunk_guard_fallback_args(path: &str) -> Value {
    json!({
        "action": "grep",
        "path": path,
        "pattern": SPOOL_CHUNK_GREP_PATTERN
    })
}

#[cold]
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
    push_guard_failure_messages(
        ctx,
        tool_call_id,
        build_spool_chunk_guard_error_content(spool_path, max_reads_per_turn),
        &block_reason,
    );

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

    #[test]
    fn repeated_file_read_family_key_tracks_cat_via_unified_exec() {
        let args = serde_json::json!({"command": "cat README.md"});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, Some("unified_exec::run::cat README.md".to_string()));
    }

    #[test]
    fn repeated_file_read_family_key_tracks_head_via_unified_exec() {
        let args = serde_json::json!({"command": "head -n 10 file.txt"});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(
            key,
            Some("unified_exec::run::head -n 10 file.txt".to_string())
        );
    }

    #[test]
    fn repeated_file_read_family_key_ignores_non_file_reading_commands() {
        let args = serde_json::json!({"command": "ls -la"});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, None);
    }

    #[test]
    fn repeated_file_read_family_key_ignores_git_status() {
        let args = serde_json::json!({"command": "git status"});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, None);
    }

    #[test]
    fn repeated_file_read_family_key_handles_cmd_alias() {
        let args = serde_json::json!({"cmd": "cat Cargo.toml"});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, Some("unified_exec::run::cat Cargo.toml".to_string()));
    }

    #[test]
    fn repeated_file_read_family_key_returns_none_for_missing_command() {
        let args = serde_json::json!({});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, None);
    }
}
