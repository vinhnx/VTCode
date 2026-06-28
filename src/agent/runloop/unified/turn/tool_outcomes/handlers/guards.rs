use std::path::Path;

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
use crate::agent::runloop::unified::turn::tool_outcomes::response_content::maybe_inline_spooled;

use super::looping::{
    low_signal_family_key, shell_run_signature, spool_chunk_read_path,
    task_tracker_create_signature,
};
use super::{ValidationResult, build_failure_error_content};
use crate::agent::runloop::unified::tool_reads::{
    read_spool_head_for_error_check, spool_content_looks_like_error,
};

const SPOOL_CHUNK_GREP_PATTERN: &str = "warning|error|TODO";
const SPOOL_CHUNK_INLINE_MAX_BYTES: usize = 32 * 1024;
const SPOOL_CHUNK_INLINE_HEAD_BYTES: usize = 8 * 1024;
const SPOOL_CHUNK_INLINE_TAIL_BYTES: usize = 8 * 1024;
const MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS: usize = 4;

/// Read a spool file's content for inline embedding when the spool-chunk
/// guard trips. Returns `None` if the file is missing, empty, or unreadable.
/// Caps content at `SPOOL_CHUNK_INLINE_MAX_BYTES` to bound the response size
/// and uses head+tail truncation when the file is larger than
/// `SPOOL_CHUNK_INLINE_HEAD_BYTES + SPOOL_CHUNK_INLINE_TAIL_BYTES`.
fn read_spool_preview_for_guard(path: &str) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let len = metadata.len() as usize;
    if len == 0 {
        return None;
    }

    let total_cap = SPOOL_CHUNK_INLINE_MAX_BYTES.min(len);
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0u8; total_cap];
    use std::io::Read;
    file.read_exact(&mut buffer).ok()?;
    let content = String::from_utf8_lossy(&buffer).to_string();
    if content.len() <= SPOOL_CHUNK_INLINE_HEAD_BYTES + SPOOL_CHUNK_INLINE_TAIL_BYTES {
        return Some(content);
    }
    let head: String = content
        .chars()
        .take(SPOOL_CHUNK_INLINE_HEAD_BYTES)
        .collect();
    let tail: String = content
        .chars()
        .rev()
        .take(SPOOL_CHUNK_INLINE_TAIL_BYTES)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Some(format!(
        "{head}\n\n... [spool content truncated; full file is {len} bytes] ...\n\n{tail}"
    ))
}

/// Inspect the spool filename and try to derive a useful fallback tool call
/// that resumes the original workflow rather than the generic
/// `grep warning|error|TODO` placeholder.
///
/// Recognized patterns:
///   - `run-<id>.txt`   -> `unified_exec action=poll session_id=<id>`
///   - `unified_search_<ts>.txt` -> `unified_search action=grep pattern="." path=<spool>`
///   - `unified_exec_<ts>.txt`   -> fallback to head/tail preview tool
///   - `outline_<ts>.txt`         -> `unified_search action=grep` on the spool
fn derive_spool_fallback(path: &str) -> Option<(String, Value)> {
    let file_name = Path::new(path).file_name()?.to_str()?.to_string();

    if let Some(sid) = file_name
        .strip_suffix(".txt")
        .and_then(|stem| stem.strip_prefix("run-"))
    {
        return Some((
            tool_names::UNIFIED_EXEC.to_string(),
            json!({
                "action": "poll",
                "session_id": sid,
            }),
        ));
    }

    let stem = file_name.strip_suffix(".txt")?;
    let prefix = stem.split('_').next()?;
    match prefix {
        "unified" | "outline" | "search" => Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            json!({
                "action": "grep",
                "path": path,
                "pattern": ".",
                "max_results": 200,
            }),
        )),
        _ => None,
    }
}

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

    // A single allowed tool call is enough to reset `streak`, but the model can
    // still churn on alternating blocked calls (for example repeated shell
    // denials interleaved with allowed reads). Keep the consecutive cap for the
    // common tight-loop case and add a wider total fuse for non-consecutive
    // blocked calls in normal turns. Recovery mode already uses `max_streak` as
    // its tighter total fuse after the one-pass grace above.
    let recovery_total_fuse_tripped = ctx.is_recovery_active() && blocked_total > max_streak;
    let normal_total_fuse_tripped = !ctx.is_recovery_active() && blocked_total > max_streak * 2;
    if streak <= max_streak && !recovery_total_fuse_tripped && !normal_total_fuse_tripped {
        return None;
    }

    let display_tool = tool_action_label(tool_name, args);
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
    push_guard_failure_messages(
        ctx,
        tool_call_id,
        build_failure_error_content(
            if recovery_total_fuse_tripped {
                format!(
                    "Blocked tool calls exceeded the recovery-mode cap ({max_streak}) for this turn."
                )
            } else if normal_total_fuse_tripped {
                let max_total = max_streak * 2;
                format!("Blocked tool calls exceeded cap ({max_total}) for this turn.")
            } else {
                format!("Consecutive blocked tool calls exceeded cap ({max_streak}) for this turn.")
            },
            if recovery_total_fuse_tripped || normal_total_fuse_tripped {
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
            "Repeated identical shell command runs exceeded per-turn cap ({max_repeated_runs}). Reuse prior output or change command before retrying."
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
            Some(format!("{canonical_tool_name}::run::{command_str}"))
        }
        _ => None,
    }
}

#[cold]
fn build_repeated_file_read_family_error_content(target: &str) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "File '{target}' already read. Content is in conversation history above. Synthesize your answer from existing data. Do NOT re-read."
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

/// Returns the path if this is a read of a planning artifact (a plan file or
/// directory) while planning mode is active.
fn is_plan_artifact_read(canonical_tool_name: &str, args: &Value) -> Option<String> {
    if !is_read_action(canonical_tool_name, args) {
        return None;
    }
    let path = extract_read_path(args)?;
    let lower = path.to_ascii_lowercase();
    if lower.contains("plan") || lower.ends_with(".md") {
        Some(path)
    } else {
        None
    }
}

#[cold]
fn build_read_after_write_error(path: &str) -> String {
    super::super::execution_result::build_error_content(
        format!(
            "File '{path}' was just written in this turn. The write response includes a diff preview. Reuse the diff output or specify offset/limit for a specific range."
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
                "Repeated read-only exploration of '{target}' hit the per-turn family cap ({MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS}). Scheduling a final recovery pass without more tools."
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
            ctx.push_tool_response(
                tool_call_id,
                maybe_inline_spooled(canonical_tool_name, &reused_value),
            );
            return Some(ValidationResult::Handled);
        }
    }

    // Planning-mode-specific guard: repeated plan-file reads across turns.
    if ctx.tool_registry.is_planning_active() {
        if let Some(plan_path) = is_plan_artifact_read(canonical_tool_name, effective_args) {
            if let Some(mut reused_value) = ctx.tool_registry.find_recent_successful_by_read_target(
                canonical_tool_name,
                effective_args,
                ctx.harness_state.max_tool_wall_clock,
            ) {
                if let Some(obj) = reused_value.as_object_mut() {
                    super::apply_reused_read_only_loop_metadata(obj);
                    // Overwrite with planning-specific guidance AFTER the generic
                    // metadata is applied, since apply_reused_read_only_loop_metadata
                    // sets its own loop_detected_note.
                    obj.insert(
                        "loop_detected_note".to_string(),
                        json!(format!(
                            "Planning mode: plan file '{}' was already read. Stop re-reading and finalize the plan.",
                            plan_path
                        )),
                    );
                }
                ctx.push_tool_response(
                    tool_call_id,
                    maybe_inline_spooled(canonical_tool_name, &reused_value),
                );
                ctx.harness_state
                    .record_successful_readonly_signature(signature);
                return Some(ValidationResult::Handled);
            }
        }
    }

    // Cross-turn TTL-bounded cache.
    if let Some(mut reused_value) = ctx.tool_registry.find_recent_successful_by_read_target(
        canonical_tool_name,
        effective_args,
        ctx.harness_state.max_tool_wall_clock,
    ) {
        if let Some(obj) = reused_value.as_object_mut() {
            super::apply_reused_read_only_loop_metadata(obj);
        }
        ctx.push_tool_response(
            tool_call_id,
            maybe_inline_spooled(canonical_tool_name, &reused_value),
        );
        ctx.harness_state
            .record_successful_readonly_signature(signature);
        return Some(ValidationResult::Handled);
    }

    // Cross-turn duplicate: scan working history.
    if let Some(raw_output) =
        find_duplicate_in_history(ctx.working_history, canonical_tool_name, effective_args)
    {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&raw_output) {
            if let Some(obj) = parsed.as_object_mut() {
                super::apply_reused_read_only_loop_metadata(obj);
            }
            ctx.push_tool_response(
                tool_call_id,
                maybe_inline_spooled(canonical_tool_name, &parsed),
            );
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
        "Repeated shell command guard stopped '{display_tool}' after {streak} identical runs (max {max_repeated_runs}). Scheduling a final recovery pass without more tools."
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
    derive_spool_fallback(path)
        .map(|(_, args)| args)
        .unwrap_or_else(|| {
            json!({
                "action": "grep",
                "path": path,
                "pattern": SPOOL_CHUNK_GREP_PATTERN
            })
        })
}

#[cold]
fn spool_chunk_guard_fallback_tool(path: &str) -> Option<String> {
    if let Some((tool, _)) = derive_spool_fallback(path) {
        Some(tool)
    } else {
        Some(tool_names::UNIFIED_SEARCH.to_string())
    }
}

#[cold]
fn build_spool_chunk_guard_error_content(path: &str, max_reads_per_turn: usize) -> String {
    let inline_content = read_spool_preview_for_guard(path);
    let fallback_tool = spool_chunk_guard_fallback_tool(path);
    let fallback_args = spool_chunk_guard_fallback_args(path);

    let mut payload = super::super::execution_result::build_error_content(
        format!(
            "Spool chunk reads exceeded per-turn cap ({max_reads_per_turn}). Use targeted extraction before reading more from '{path}'."
        ),
        fallback_tool.clone(),
        Some(fallback_args),
        "spool_chunk_guard",
    );

    if let Some(obj) = payload.as_object_mut() {
        // Mark the response so the model knows the tool output is already
        // present in the conversation and recovery is required.
        obj.insert("loop_detected".to_string(), Value::Bool(true));
        obj.insert("recovery_required".to_string(), Value::Bool(true));
        // Replace the generic "next_action" with one that points at the
        // inline content we just embedded.
        obj.insert(
            "next_action".to_string(),
            Value::String(
                "STOP calling read_file/unified_file on this spool. The full \
                 content is in `inline_content` below. Synthesize your final \
                 answer from the existing conversation history."
                    .to_string(),
            ),
        );
        if let Some(content) = inline_content {
            obj.insert("inline_content".to_string(), Value::String(content));
            obj.insert(
                "inline_content_note".to_string(),
                Value::String(
                    "Full spool content embedded inline. Do NOT re-read this \
                     spool file — the per-turn cap will continue to block you."
                        .to_string(),
                ),
            );
        }
        if let Some(tool) = fallback_tool {
            obj.insert("fallback_tool".to_string(), Value::String(tool));
        }
    }

    payload.to_string()
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

    // Short-circuit: if the spool file's first bytes look like an error
    // payload, the agent already has the error in its conversation history
    // from the previous turn. Inject the error inline as a tool response and
    // tell the model to use it instead of paginating the spool.
    if let Some(head) = read_spool_head_for_error_check(spool_path) {
        if spool_content_looks_like_error(&head) {
            ctx.push_tool_response(
                tool_call_id,
                build_previous_turn_error_spool_content(spool_path, &head),
            );
            ctx.push_system_message(format!(
                "Spool file '{spool_path}' contains a tool error from an earlier turn. Use the error payload already in your conversation history instead of re-reading the spool."
            ));
            // Do not increment the streak for this short-circuit — the model
            // is being told to stop reading the spool, not to try again.
            ctx.harness_state.reset_spool_chunk_read_streak();
            return Some(ValidationResult::Handled);
        }
    }

    let max_reads_per_turn = max_sequential_spool_chunk_reads_per_turn(ctx);
    let streak = ctx.harness_state.record_spool_chunk_read();
    if streak <= max_reads_per_turn {
        return None;
    }

    // Once the cap trips, do not increment the streak again for this path so
    // subsequent attempts don't double-count and the recovery pass can
    // synthesize a final answer without re-entering this guard.
    ctx.harness_state.reset_spool_chunk_read_streak();

    let display_tool = tool_action_label(canonical_tool_name, args);
    let block_reason = format!(
        "Spool chunk guard stopped repeated '{display_tool}' calls for this turn. Scheduling a final recovery pass without more tools."
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

/// Build the tool response payload for a short-circuited spool read of an
/// error payload from a previous turn. The payload exposes the original
/// error and a `hint` directing the model to use the error in its history
/// rather than re-reading the spool file.
fn build_previous_turn_error_spool_content(spool_path: &str, head: &str) -> String {
    let preview = if head.len() > 1024 {
        format!("{}... [truncated]", &head[..1024])
    } else {
        head.to_string()
    };
    serde_json::json!({
        "spool_path": spool_path,
        "loop_detected": true,
        "is_recoverable": true,
        "error_class": "previous_turn_error_spool",
        "next_action": "STOP re-reading this spool file. The original error payload is already in your conversation history from the previous turn. Choose a different tool or approach.",
        "hint": "This spool contains an error response from an earlier turn; you do not need to re-read it. Use the error message already in your history and try a different approach.",
        "inline_content": preview,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spool_chunk_guard_error_resolves_to_pty_poll_for_run_prefix() {
        let payload =
            build_spool_chunk_guard_error_content(".vtcode/context/tool_outputs/run-1.txt", 3);
        let parsed: Value =
            serde_json::from_str(&payload).expect("spool chunk guard payload should be json");

        assert_eq!(
            parsed.get("fallback_tool").and_then(Value::as_str),
            Some(tool_names::UNIFIED_EXEC)
        );
        assert_eq!(parsed["fallback_tool_args"]["action"], "poll");
        assert_eq!(parsed["fallback_tool_args"]["session_id"], "1");
        assert!(parsed.get("next_action").and_then(Value::as_str).is_some());
        assert_eq!(
            parsed.get("loop_detected").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            parsed.get("recovery_required").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn spool_chunk_guard_error_resolves_to_search_grep_for_search_prefix() {
        let payload = build_spool_chunk_guard_error_content(
            ".vtcode/context/tool_outputs/unified_search_1782625284532136.txt",
            3,
        );
        let parsed: Value =
            serde_json::from_str(&payload).expect("spool chunk guard payload should be json");

        assert_eq!(
            parsed.get("fallback_tool").and_then(Value::as_str),
            Some(tool_names::UNIFIED_SEARCH)
        );
        assert_eq!(parsed["fallback_tool_args"]["action"], "grep");
        assert_eq!(
            parsed["fallback_tool_args"]["path"],
            ".vtcode/context/tool_outputs/unified_search_1782625284532136.txt"
        );
        assert_eq!(
            parsed.get("loop_detected").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            parsed.get("recovery_required").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn spool_chunk_guard_error_falls_back_to_warning_error_todo_for_unknown_prefix() {
        let payload = build_spool_chunk_guard_error_content(
            ".vtcode/context/tool_outputs/custom_tool_42.txt",
            3,
        );
        let parsed: Value =
            serde_json::from_str(&payload).expect("spool chunk guard payload should be json");

        assert_eq!(
            parsed.get("fallback_tool").and_then(Value::as_str),
            Some(tool_names::UNIFIED_SEARCH)
        );
        assert_eq!(parsed["fallback_tool_args"]["action"], "grep");
        assert_eq!(
            parsed["fallback_tool_args"]["pattern"],
            SPOOL_CHUNK_GREP_PATTERN
        );
    }

    #[test]
    fn derive_spool_fallback_recognizes_pty_session_id() {
        let (tool, args) = derive_spool_fallback(".vtcode/context/tool_outputs/run-abc123.txt")
            .expect("pty session spool should resolve to a fallback");
        assert_eq!(tool, tool_names::UNIFIED_EXEC);
        assert_eq!(args["action"], "poll");
        assert_eq!(args["session_id"], "abc123");
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
