//! Tool outcome handling helpers for turn execution.

use anyhow::Result;
use serde_json::{Value, json};
use std::time::Duration;

use vtcode_core::config::constants::defaults::{
    DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN,
    DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN,
};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::tools::tool_intent;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::tool_call_safety::SafetyError;
use crate::agent::runloop::unified::tool_routing::{
    ensure_tool_permission, prompt_session_limit_increase,
};
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use super::helpers::push_tool_response;
use crate::agent::runloop::unified::tool_routing::ToolPermissionFlow;
#[path = "handlers_batch.rs"]
mod handlers_batch;
pub(crate) use handlers_batch::{execute_and_handle_tool_call, handle_tool_call_batch};

/// Result of a tool call validation phase.
pub(crate) enum ValidationResult {
    /// Proceed with execution
    Proceed(PreparedToolCall),
    /// Tool was blocked or handled internally (e.g. error pushed to history), skip execution but continue turn
    Blocked,
    /// Stop turn/loop with a specific outcome (e.g. Exit or Cancel)
    Outcome(TurnHandlerOutcome),
}

/// Canonicalized validation data reused across the execution path.
pub(crate) struct PreparedToolCall {
    pub canonical_name: String,
    pub readonly_classification: bool,
    pub effective_args: serde_json::Value,
}

const MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS: usize = 4;
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(5);
const TOOL_BUDGET_WARNING_THRESHOLD: f64 = 0.75;

fn build_failure_error_content(error: String, failure_kind: &'static str) -> String {
    super::execution_result::build_error_content(error, None, None, failure_kind).to_string()
}

fn build_validation_error_content_with_fallback(
    error: String,
    validation_stage: &'static str,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<Value>,
) -> String {
    let is_recoverable = fallback_tool.is_some();
    let next_action = if is_recoverable {
        "Retry with fallback_tool_args."
    } else {
        "Fix tool arguments to match the schema."
    };
    let mut payload = serde_json::json!({
        "error": error,
        "failure_kind": "validation",
        "error_class": "invalid_arguments",
        "validation_stage": validation_stage,
        "retryable": false,
        "is_recoverable": is_recoverable,
        "next_action": next_action,
    });
    if let Some(obj) = payload.as_object_mut() {
        if let Some(tool) = fallback_tool {
            obj.insert("fallback_tool".to_string(), Value::String(tool));
        }
        if let Some(args) = fallback_tool_args {
            obj.insert("fallback_tool_args".to_string(), args);
        }
    }
    payload.to_string()
}

fn build_rate_limit_error_content(tool_name: &str, retry_after_ms: u64) -> String {
    serde_json::json!({
        "error": format!(
            "Tool '{}' is temporarily rate limited. Try again after a short delay.",
            tool_name
        ),
        "failure_kind": "rate_limit",
        "rate_limited": true,
        "retry_after_ms": retry_after_ms,
    })
    .to_string()
}

fn build_tool_budget_warning_message(used: usize, max: usize, remaining: usize) -> String {
    format!(
        "Tool-call budget warning: {used}/{max} used; {remaining} remaining for this turn. Use targeted extraction/batching before additional tool calls."
    )
}

fn build_tool_budget_exhausted_reason(used: usize, max: usize) -> String {
    format!(
        "Tool-call budget exhausted for this turn ({used}/{max}). Start a new turn with \"continue\" to proceed."
    )
}

fn max_consecutive_blocked_tool_calls_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
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
    let streak = ctx.harness_state.record_blocked_tool_call();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(ctx);
    if streak <= max_streak {
        return None;
    }

    let display_tool = tool_action_label(tool_name, args);
    let block_reason = format!(
        "Consecutive blocked tool calls reached per-turn cap ({max_streak}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
    );
    push_tool_response(
        ctx.working_history,
        tool_call_id.to_string(),
        build_failure_error_content(
            format!("Consecutive blocked tool calls exceeded cap ({max_streak}) for this turn."),
            "blocked_streak",
        ),
    );
    ctx.working_history
        .push(uni::Message::system(block_reason.clone()));

    Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
        reason: Some(block_reason),
    }))
}

fn compact_loop_key_part(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn patch_source_arg(args: &Value) -> Option<&str> {
    args.as_str()
        .or_else(|| args.get("input").and_then(|v| v.as_str()))
        .or_else(|| args.get("patch").and_then(|v| v.as_str()))
}

fn extract_patch_target_path(patch_source: &str) -> Option<&str> {
    const PATCH_FILE_PREFIXES: [&str; 4] = [
        "*** Update File: ",
        "*** Add File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];

    patch_source.lines().find_map(|line| {
        PATCH_FILE_PREFIXES
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix))
            .map(str::trim)
            .filter(|path| !path.is_empty())
    })
}

fn patch_signature(patch_source: &str) -> String {
    // Deterministic, cheap fingerprint for loop keys; bounded to first 2KB.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in patch_source.as_bytes().iter().take(2048) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("len{}-fnv{:016x}", patch_source.len(), hash)
}

fn read_file_path_arg(args: &Value) -> Option<&str> {
    let obj = args.as_object()?;
    for key in ["path", "file_path", "filepath", "target_path"] {
        if let Some(path) = obj.get(key).and_then(|v| v.as_str()) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn unified_file_destination_arg(args: &Value) -> Option<&str> {
    let destination = args.get("destination").and_then(|v| v.as_str())?;
    let trimmed = destination.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn read_file_has_offset_arg(args: &Value) -> bool {
    ["offset", "offset_lines", "offset_bytes"]
        .iter()
        .any(|key| args.get(*key).is_some())
}

fn read_file_offset_value(args: &Value) -> Option<usize> {
    ["offset", "offset_lines", "offset_bytes"]
        .iter()
        .filter_map(|key| args.get(*key))
        .find_map(|value| {
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
        })
}

fn read_file_has_limit_arg(args: &Value) -> bool {
    ["limit", "page_size_lines", "max_lines", "chunk_lines"]
        .iter()
        .any(|key| args.get(*key).is_some())
}

fn read_file_limit_value(args: &Value) -> Option<usize> {
    ["limit", "page_size_lines", "max_lines", "chunk_lines"]
        .iter()
        .filter_map(|key| args.get(*key))
        .find_map(|value| {
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
        })
}

fn looks_like_tool_output_spool_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains(".vtcode/context/tool_outputs/")
}

fn is_read_file_style_call(canonical_tool_name: &str, args: &Value) -> bool {
    match canonical_tool_name {
        tool_names::READ_FILE => true,
        tool_names::UNIFIED_FILE => tool_intent::unified_file_action(args)
            .unwrap_or("read")
            .eq_ignore_ascii_case("read"),
        _ => false,
    }
}

fn maybe_apply_spool_read_offset_hint(
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    canonical_tool_name: &str,
    args: &Value,
) -> Value {
    if !is_read_file_style_call(canonical_tool_name, args) {
        return args.clone();
    }

    let Some(path) = read_file_path_arg(args) else {
        return args.clone();
    };
    if !looks_like_tool_output_spool_path(path) {
        return args.clone();
    }

    let Some((next_offset, chunk_limit)) =
        tool_registry.find_recent_read_file_spool_progress(path, Duration::from_secs(180))
    else {
        return args.clone();
    };

    let requested_offset = read_file_offset_value(args);
    let should_advance_offset = match requested_offset {
        Some(existing) => existing < next_offset,
        None => true,
    };
    let should_fill_offset = !read_file_has_offset_arg(args);

    let mut adjusted = args.clone();
    let keep_existing_limit = read_file_has_limit_arg(&adjusted);
    if let Some(obj) = adjusted.as_object_mut() {
        if should_fill_offset || should_advance_offset {
            obj.insert("offset".to_string(), json!(next_offset));
        }
        if !keep_existing_limit {
            obj.insert("limit".to_string(), json!(chunk_limit));
        }
        if should_fill_offset || should_advance_offset || !keep_existing_limit {
            tracing::debug!(
                tool = canonical_tool_name,
                path = path,
                requested_offset = requested_offset.unwrap_or(0),
                next_offset,
                chunk_limit,
                "Applied spool read continuation hint to avoid repeated identical chunk reads"
            );
        }
    }
    adjusted
}

fn preflight_validation_fallback(
    tool_name: &str,
    args_val: &Value,
    error: &anyhow::Error,
) -> Option<(String, Value)> {
    let error_text = error.to_string();
    let is_unified_search = tool_name == tool_names::UNIFIED_SEARCH
        || error_text.contains("tool 'unified_search'")
        || error_text.contains("for 'unified_search'");
    if !is_unified_search {
        return None;
    }

    let normalized = tool_intent::normalize_unified_search_args(args_val);
    if normalized == *args_val || normalized.get("action").is_none() {
        return None;
    }

    Some((tool_names::UNIFIED_SEARCH.to_string(), normalized))
}

fn try_recover_preflight_for_unified_search(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &Value,
    error: &anyhow::Error,
) -> Option<(vtcode_core::tools::registry::ToolPreflightOutcome, Value)> {
    let (_, recovered_args) = preflight_validation_fallback(tool_name, args_val, error)?;
    match ctx
        .tool_registry
        .preflight_validate_call(tool_name, &recovered_args)
    {
        Ok(preflight) => Some((preflight, recovered_args)),
        Err(recovery_err) => {
            tracing::debug!(
                tool = tool_name,
                original_error = %error,
                recovery_error = %recovery_err,
                "Unified search preflight recovery failed"
            );
            None
        }
    }
}

fn task_tracker_create_signature(tool_name: &str, args: &Value) -> Option<String> {
    if tool_name != tool_names::TASK_TRACKER {
        return None;
    }

    let action = args.get("action").and_then(Value::as_str)?;
    if action != "create" {
        return None;
    }

    let mut normalized = serde_json::Map::new();
    normalized.insert("action".to_string(), Value::String("create".to_string()));
    if let Some(title) = args.get("title").and_then(Value::as_str) {
        normalized.insert("title".to_string(), Value::String(title.to_string()));
    }
    if let Some(items) = args.get("items").and_then(Value::as_array) {
        normalized.insert("items".to_string(), Value::Array(items.clone()));
    }
    if let Some(notes) = args.get("notes") {
        normalized.insert("notes".to_string(), notes.clone());
    }

    let serialized = serde_json::to_string(&Value::Object(normalized)).ok()?;
    Some(format!("task_tracker::create::{serialized}"))
}

fn enforce_duplicate_task_tracker_create_guard<'a>(
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

    let content = super::execution_result::build_error_content(
        "Duplicate task_tracker.create detected in this turn. Use task_tracker.update/list to continue tracking progress."
            .to_string(),
        Some(tool_names::TASK_TRACKER.to_string()),
        Some(serde_json::json!({ "action": "list" })),
        "duplicate_task_tracker_create",
    )
    .to_string();
    push_tool_response(ctx.working_history, tool_call_id.to_string(), content);
    ctx.working_history.push(uni::Message::system(
        "Blocked duplicate task_tracker.create in the same turn. Continue with task_tracker.update/list."
            .to_string(),
    ));
    Some(ValidationResult::Blocked)
}

fn spool_chunk_read_path<'a>(canonical_tool_name: &str, args: &'a Value) -> Option<&'a str> {
    if !is_read_file_style_call(canonical_tool_name, args) {
        return None;
    }
    let path = read_file_path_arg(args)?;
    if looks_like_tool_output_spool_path(path) {
        Some(path)
    } else {
        None
    }
}

fn max_sequential_spool_chunk_reads_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_sequential_spool_chunk_reads)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN)
}

fn build_spool_chunk_guard_error_content(path: &str, max_reads_per_turn: usize) -> String {
    super::execution_result::build_error_content(
        format!(
            "Spool chunk reads exceeded per-turn cap ({}). Use targeted extraction before reading more from '{}'.",
            max_reads_per_turn, path
        ),
        Some(tool_names::GREP_FILE.to_string()),
        Some(json!({
            "path": path,
            "pattern": "warning|error|TODO"
        })),
        "spool_chunk_guard",
    )
    .to_string()
}

fn enforce_spool_chunk_read_guard(
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
        "Spool chunk guard stopped repeated '{}' calls for this turn.\nUse `grep_file` or summarize before more chunk reads.",
        display_tool
    );

    push_tool_response(
        ctx.working_history,
        tool_call_id.to_string(),
        build_spool_chunk_guard_error_content(spool_path, max_reads_per_turn),
    );

    Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
        TurnLoopResult::Blocked {
            reason: Some(block_reason),
        },
    )))
}

fn loop_detection_tool_key(canonical_tool_name: &str, args: &serde_json::Value) -> String {
    match canonical_tool_name {
        tool_names::READ_FILE => {
            let offset = args
                .get("offset")
                .or_else(|| args.get("offset_lines"))
                .or_else(|| args.get("offset_bytes"))
                .and_then(|v| {
                    v.as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
                })
                .unwrap_or(1);
            let limit = read_file_limit_value(args).unwrap_or(0);
            if let Some(path) = read_file_path_arg(args) {
                return format!(
                    "{canonical_tool_name}::{}::offset={offset}::limit={limit}",
                    compact_loop_key_part(path, 120)
                );
            }
            format!("{canonical_tool_name}::offset={offset}::limit={limit}")
        }
        tool_names::UNIFIED_FILE => {
            let action = tool_intent::unified_file_action(args).unwrap_or("read");
            let action = action.to_ascii_lowercase();
            if action == "read"
                && let Some(path) = args
                    .get("path")
                    .or_else(|| args.get("file_path"))
                    .or_else(|| args.get("filepath"))
                    .or_else(|| args.get("target_path"))
                    .and_then(|v| v.as_str())
            {
                let offset = args
                    .get("offset")
                    .or_else(|| args.get("offset_lines"))
                    .or_else(|| args.get("offset_bytes"))
                    .and_then(|v| {
                        v.as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
                    })
                    .unwrap_or(1);
                let limit = read_file_limit_value(args).unwrap_or(0);
                return format!(
                    "{canonical_tool_name}::{action}::{}::offset={offset}::limit={limit}",
                    compact_loop_key_part(path, 120),
                );
            }
            if action == "patch"
                && let Some(patch_source) = patch_source_arg(args)
            {
                let target = extract_patch_target_path(patch_source)
                    .map(|path| compact_loop_key_part(path, 120))
                    .unwrap_or_else(|| "<unknown>".to_string());
                return format!(
                    "{canonical_tool_name}::{action}::{target}::{}",
                    patch_signature(patch_source)
                );
            }
            if matches!(
                action.as_str(),
                "edit" | "write" | "delete" | "move" | "copy"
            ) {
                let source = read_file_path_arg(args).map(|path| compact_loop_key_part(path, 120));
                let destination =
                    unified_file_destination_arg(args).map(|path| compact_loop_key_part(path, 120));
                return match (source, destination) {
                    (Some(src), Some(dest)) => {
                        format!("{canonical_tool_name}::{action}::{src}->{dest}")
                    }
                    (Some(src), None) => format!("{canonical_tool_name}::{action}::{src}"),
                    (None, Some(dest)) => {
                        format!("{canonical_tool_name}::{action}::destination={dest}")
                    }
                    (None, None) => format!("{canonical_tool_name}::{action}"),
                };
            }
            format!("{canonical_tool_name}::{action}")
        }
        tool_names::APPLY_PATCH => {
            if let Some(patch_source) = patch_source_arg(args) {
                let target = extract_patch_target_path(patch_source)
                    .map(|path| compact_loop_key_part(path, 120))
                    .unwrap_or_else(|| "<unknown>".to_string());
                return format!(
                    "{canonical_tool_name}::{target}::{}",
                    patch_signature(patch_source)
                );
            }
            canonical_tool_name.to_string()
        }
        tool_names::UNIFIED_EXEC => {
            let action = tool_intent::unified_exec_action(args).unwrap_or("run");
            let action = action.to_ascii_lowercase();
            if matches!(action.as_str(), "poll" | "continue" | "close" | "inspect")
                && let Some(session_id) = args.get("session_id").and_then(|v| v.as_str())
            {
                if action == "continue"
                    && let Some(input) = args
                        .get("input")
                        .or_else(|| args.get("chars"))
                        .or_else(|| args.get("text"))
                        .and_then(|v| v.as_str())
                {
                    return format!(
                        "{canonical_tool_name}::{action}::{}::{}",
                        compact_loop_key_part(session_id, 80),
                        compact_loop_key_part(input, 40)
                    );
                }
                return format!(
                    "{canonical_tool_name}::{action}::{}",
                    compact_loop_key_part(session_id, 80)
                );
            }
            if action == "inspect"
                && let Some(spool_path) = args.get("spool_path").and_then(|v| v.as_str())
            {
                return format!(
                    "{canonical_tool_name}::{action}::{}",
                    compact_loop_key_part(spool_path, 120)
                );
            }
            if action == "run"
                && let Some(command) = args
                    .get("command")
                    .or_else(|| args.get("cmd"))
                    .or_else(|| args.get("raw_command"))
                    .and_then(|v| v.as_str())
            {
                return format!(
                    "{canonical_tool_name}::{action}::{}",
                    compact_loop_key_part(command, 120)
                );
            }
            format!("{canonical_tool_name}::{action}")
        }
        _ => canonical_tool_name.to_string(),
    }
}

/// Consolidated state for tool outcomes to reduce signature bloat and ensure DRY across handlers.
pub struct ToolOutcomeContext<'a, 'b> {
    pub ctx: &'b mut TurnProcessingContext<'a>,
    pub repeated_tool_attempts: &'b mut super::helpers::LoopTracker,
    pub turn_modified_files: &'b mut std::collections::BTreeSet<std::path::PathBuf>,
}

/// Unified handler for a single tool call (whether native or textual).
///
/// This handler applies the full pipeline of checks:
/// 1. Circuit Breaker
/// 2. Rate Limiting
/// 3. Loop Detection
/// 4. Safety Validation (with potential user interaction for limits)
/// 5. Permission Checking (Allow/Deny/Ask)
/// 6. Execution (with progress spinners and PTY streaming)
/// 7. Result Handling (recording metrics, history, UI output)
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_single_tool_call<'a, 'b, 'tool>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call_id: String,
    tool_name: &'tool str,
    args_val: serde_json::Value,
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.harness_state.set_phase(TurnPhase::ExecutingTools);

    // 1. Validate (Circuit Breaker, Rate Limit, Loop Detection, Safety, Permission)
    let prepared = match validate_tool_call(t_ctx.ctx, &tool_call_id, tool_name, &args_val).await? {
        ValidationResult::Outcome(outcome) => return Ok(Some(outcome)),
        ValidationResult::Blocked => {
            if let Some(outcome) =
                enforce_blocked_tool_call_guard(t_ctx.ctx, &tool_call_id, tool_name, &args_val)
            {
                return Ok(Some(outcome));
            }
            return Ok(None);
        }
        ValidationResult::Proceed(prepared) => {
            t_ctx.ctx.harness_state.reset_blocked_tool_call_streak();
            prepared
        }
    };

    // 3. Execute and Handle Result
    if let Some(outcome) = execute_and_handle_tool_call(
        t_ctx.ctx,
        t_ctx.repeated_tool_attempts,
        t_ctx.turn_modified_files,
        tool_call_id,
        &prepared.canonical_name,
        prepared.effective_args,
        None,
    )
    .await?
    {
        return Ok(Some(outcome));
    }

    Ok(None)
}

/// Validates a tool call against all safety and permission checks.
/// Returns Some(TurnHandlerOutcome) if the turn loop should break/exit/cancel.
/// Returns None if execution should proceed (or if a local error was already handled/pushed).
pub(crate) async fn validate_tool_call<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
) -> Result<ValidationResult> {
    if ctx.harness_state.tool_budget_exhausted() {
        let error_msg = format!(
            "Policy violation: exceeded max tool calls per turn ({})",
            ctx.harness_state.max_tool_calls
        );
        push_tool_response(
            ctx.working_history,
            tool_call_id.to_string(),
            build_failure_error_content(error_msg, "policy"),
        );
        let block_reason = build_tool_budget_exhausted_reason(
            ctx.harness_state.tool_calls,
            ctx.harness_state.max_tool_calls,
        );
        if !ctx.harness_state.tool_budget_exhausted_emitted {
            ctx.working_history
                .push(uni::Message::system(block_reason.clone()));
            ctx.harness_state.mark_tool_budget_exhausted_emitted();
        }
        return Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(
            TurnLoopResult::Blocked {
                reason: Some(block_reason),
            },
        )));
    }

    if ctx.harness_state.wall_clock_exhausted() {
        let error_msg = format!(
            "Policy violation: exceeded tool wall clock budget ({}s)",
            ctx.harness_state.max_tool_wall_clock.as_secs()
        );
        push_tool_response(
            ctx.working_history,
            tool_call_id.to_string(),
            build_failure_error_content(error_msg, "policy"),
        );
        return Ok(ValidationResult::Blocked);
    }

    let (preflight, preflight_args) = match ctx
        .tool_registry
        .preflight_validate_call(tool_name, args_val)
    {
        Ok(preflight) => (preflight, args_val.clone()),
        Err(err) => {
            if let Some((recovered_preflight, recovered_args)) =
                try_recover_preflight_for_unified_search(ctx, tool_name, args_val, &err)
            {
                tracing::info!(
                    tool = tool_name,
                    "Recovered preflight for unified_search by normalizing arguments"
                );
                (recovered_preflight, recovered_args)
            } else {
                let fallback = preflight_validation_fallback(tool_name, args_val, &err);
                let (fallback_tool, fallback_tool_args) = fallback
                    .map(|(tool, args)| (Some(tool), Some(args)))
                    .unwrap_or((None, None));
                push_tool_response(
                    ctx.working_history,
                    tool_call_id.to_string(),
                    build_validation_error_content_with_fallback(
                        format!("Tool preflight validation failed: {}", err),
                        "preflight",
                        fallback_tool,
                        fallback_tool_args,
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
        }
    };
    let canonical_tool_name = preflight.normalized_tool_name.clone();
    let effective_args = maybe_apply_spool_read_offset_hint(
        ctx.tool_registry,
        &canonical_tool_name,
        &preflight_args,
    );

    if let Some(outcome) = enforce_duplicate_task_tracker_create_guard(
        ctx,
        tool_call_id,
        &canonical_tool_name,
        &effective_args,
    ) {
        return Ok(outcome);
    }

    // Charge tool-call budget only after preflight succeeds.
    ctx.harness_state.record_tool_call();
    if ctx
        .harness_state
        .should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD)
    {
        let used = ctx.harness_state.tool_calls;
        let max = ctx.harness_state.max_tool_calls;
        let remaining = ctx.harness_state.remaining_tool_calls();
        ctx.working_history
            .push(uni::Message::system(build_tool_budget_warning_message(
                used, max, remaining,
            )));
        ctx.harness_state.mark_tool_budget_warning_emitted();
    }

    if let Some(outcome) =
        enforce_spool_chunk_read_guard(ctx, tool_call_id, &canonical_tool_name, &effective_args)
    {
        return Ok(outcome);
    }

    // Phase 4 Check: Per-tool Circuit Breaker
    if !ctx
        .circuit_breaker
        .allow_request_for_tool(&canonical_tool_name)
    {
        let error_msg = format!(
            "Tool '{}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).",
            canonical_tool_name
        );
        tracing::warn!(tool = %canonical_tool_name, "Circuit breaker open, tool disabled");
        push_tool_response(
            ctx.working_history,
            tool_call_id.to_string(),
            build_failure_error_content(error_msg, "circuit_breaker"),
        );
        return Ok(ValidationResult::Blocked);
    }

    // Phase 4 Check: Adaptive Rate Limiter
    if let Some(outcome) =
        acquire_adaptive_rate_limit_slot(ctx, tool_call_id, &canonical_tool_name).await?
    {
        return Ok(outcome);
    }

    // Phase 4 Check: Adaptive Loop Detection
    let loop_tool_key = loop_detection_tool_key(&canonical_tool_name, &effective_args);
    if let Some(warning) = ctx
        .autonomous_executor
        .record_tool_call(&loop_tool_key, &effective_args)
    {
        let should_block = {
            if let Ok(detector) = ctx.autonomous_executor.loop_detector().read() {
                detector.is_hard_limit_exceeded(&loop_tool_key)
            } else {
                false
            }
        };

        if should_block {
            tracing::warn!(tool = %loop_tool_key, "Loop detector blocked tool");
            let display_tool = tool_action_label(&canonical_tool_name, args_val);
            let block_reason = format!(
                "Loop detector stopped repeated '{}' calls for this turn.\nType \"continue\" to retry with a different strategy.",
                display_tool
            );
            // Ensure no orphan PTY processes keep running after a hard loop-detection stop.
            ctx.tool_registry.terminate_all_pty_sessions();
            ctx.handle.set_input_status(None, None);
            ctx.input_status_state.left = None;
            ctx.input_status_state.right = None;
            if let Some(mut spooled) = ctx.tool_registry.find_recent_spooled_output(
                &canonical_tool_name,
                &effective_args,
                Duration::from_secs(120),
            ) {
                if let Some(obj) = spooled.as_object_mut() {
                    obj.remove("output");
                    obj.remove("content");
                    obj.remove("stdout");
                    obj.remove("stderr");
                    obj.remove("stderr_preview");
                    obj.insert(
                        "reused_spooled_output".to_string(),
                        serde_json::Value::Bool(true),
                    );
                    obj.insert("loop_detected".to_string(), serde_json::Value::Bool(true));
                    obj.insert("spool_ref_only".to_string(), serde_json::Value::Bool(true));
                    obj.insert(
                        "loop_detected_note".to_string(),
                        serde_json::Value::String(
                            "Loop detected; using recent spool reference. Read the spool file instead of re-running this call.".to_string(),
                        ),
                    );
                }
                push_tool_response(
                    ctx.working_history,
                    tool_call_id.to_string(),
                    super::execution_result::maybe_inline_spooled(&canonical_tool_name, &spooled),
                );
                return Ok(ValidationResult::Blocked);
            }

            if preflight.readonly_classification
                && let Some(mut reused) = ctx.tool_registry.find_recent_successful_output(
                    &canonical_tool_name,
                    &effective_args,
                    Duration::from_secs(120),
                )
            {
                if let Some(obj) = reused.as_object_mut() {
                    // Drop bulky payload fields for repeated read-only reuse to avoid
                    // flooding context with duplicate content.
                    obj.remove("output");
                    obj.remove("content");
                    obj.remove("stdout");
                    obj.remove("stderr");
                    obj.remove("stderr_preview");
                    obj.insert(
                        "reused_recent_result".to_string(),
                        serde_json::Value::Bool(true),
                    );
                    obj.insert("result_ref_only".to_string(), serde_json::Value::Bool(true));
                    obj.insert("loop_detected".to_string(), serde_json::Value::Bool(true));
                    obj.insert(
                        "loop_detected_note".to_string(),
                        serde_json::Value::String(
                            "Loop detected on repeated read-only call; reusing recent output. Use `grep_file` or summarize before another read."
                                .to_string(),
                        ),
                    );
                    obj.insert(
                        "next_action".to_string(),
                        serde_json::Value::String(
                            "Use grep_file or adjust offset/limit before retrying this read."
                                .to_string(),
                        ),
                    );
                }
                push_tool_response(
                    ctx.working_history,
                    tool_call_id.to_string(),
                    super::execution_result::maybe_inline_spooled(&canonical_tool_name, &reused),
                );
                return Ok(ValidationResult::Blocked);
            }

            let error_msg = format!(
                "Tool '{}' is blocked due to excessive repetition (Loop Detected).",
                display_tool
            );
            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                build_failure_error_content(error_msg, "loop_detection"),
            );

            if preflight.readonly_classification {
                ctx.working_history.push(uni::Message::system(
                    "Loop detector blocked repeated read-only calls. Use `grep_file` or adjust `offset`/`limit` before retrying."
                        .to_string(),
                ));
                return Ok(ValidationResult::Blocked);
            }

            return Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                TurnLoopResult::Blocked {
                    reason: Some(block_reason),
                },
            )));
        } else {
            tracing::warn!(tool = %loop_tool_key, warning = %warning, "Loop detector warning");
        }
    }

    // Safety Validation Loop
    loop {
        let validation_result = {
            let mut validator = ctx.safety_validator.write().await;
            validator
                .validate_call(&canonical_tool_name, &effective_args)
                .await
        };

        match validation_result {
            Ok(_) => break,
            Err(SafetyError::SessionLimitReached { max }) => {
                match prompt_session_limit_increase(
                    ctx.handle,
                    ctx.session,
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    max,
                )
                .await
                {
                    Ok(Some(increment)) => {
                        let mut validator = ctx.safety_validator.write().await;
                        validator.increase_session_limit(increment);
                        continue;
                    }
                    _ => {
                        push_tool_response(
                            ctx.working_history,
                            tool_call_id.to_string(),
                            build_failure_error_content(
                                "Session tool limit reached and not increased by user".to_string(),
                                "safety_limit",
                            ),
                        );
                        return Ok(ValidationResult::Blocked);
                    }
                }
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Safety validation failed: {}", err),
                )?;
                push_tool_response(
                    ctx.working_history,
                    tool_call_id.to_string(),
                    build_failure_error_content(
                        format!("Safety validation failed: {}", err),
                        "safety_validation",
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
        }
    }

    // Ensure tool permission
    match ensure_tool_permission(
        crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
            tool_registry: ctx.tool_registry,
            renderer: ctx.renderer,
            handle: ctx.handle,
            session: ctx.session,
            default_placeholder: ctx.default_placeholder.clone(),
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            hooks: ctx.lifecycle_hooks,
            justification: None,
            approval_recorder: Some(ctx.approval_recorder.as_ref()),
            decision_ledger: Some(ctx.decision_ledger),
            tool_permission_cache: Some(ctx.tool_permission_cache),
            hitl_notification_bell: ctx
                .vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
            autonomous_mode: ctx.session_stats.is_autonomous_mode(),
            human_in_the_loop: ctx
                .vt_cfg
                .map(|cfg| cfg.security.human_in_the_loop)
                .unwrap_or(true),
            delegate_mode: ctx.session_stats.is_delegate_mode(),
            skip_confirmations: false, // Normal tool calls should prompt if configured
        },
        &canonical_tool_name,
        Some(&effective_args),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => Ok(ValidationResult::Proceed(PreparedToolCall {
            canonical_name: canonical_tool_name,
            readonly_classification: preflight.readonly_classification,
            effective_args,
        })),
        Ok(ToolPermissionFlow::Denied) => {
            let denial = ToolExecutionError::new(
                canonical_tool_name,
                ToolErrorType::PolicyViolation,
                format!(
                    "Tool '{}' execution denied by policy",
                    preflight.normalized_tool_name
                ),
            )
            .to_json_value();

            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
            );
            Ok(ValidationResult::Blocked)
        }
        Ok(ToolPermissionFlow::Exit) => Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(
            TurnLoopResult::Exit,
        ))),
        Ok(ToolPermissionFlow::Interrupted) => Ok(ValidationResult::Outcome(
            TurnHandlerOutcome::Break(TurnLoopResult::Cancelled),
        )),
        Err(err) => {
            let err_json = serde_json::json!({
                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
            });
            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                err_json.to_string(),
            );
            Ok(ValidationResult::Blocked)
        }
    }
}

async fn acquire_adaptive_rate_limit_slot<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
) -> Result<Option<ValidationResult>> {
    for attempt in 0..MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS {
        match ctx.rate_limiter.try_acquire(tool_name) {
            Ok(_) => return Ok(None),
            Err(wait_time) => {
                if ctx.ctrl_c_state.is_cancel_requested() {
                    return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                        TurnLoopResult::Cancelled,
                    ))));
                }
                if ctx.ctrl_c_state.is_exit_requested() {
                    return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                        TurnLoopResult::Exit,
                    ))));
                }

                let bounded_wait = wait_time.min(MAX_RATE_LIMIT_WAIT);
                if attempt + 1 >= MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS {
                    let retry_after_ms = bounded_wait.as_millis() as u64;
                    tracing::warn!(
                        tool = %tool_name,
                        attempts = MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS,
                        retry_after_ms,
                        "Adaptive rate limiter blocked tool execution after repeated attempts"
                    );
                    push_tool_response(
                        ctx.working_history,
                        tool_call_id.to_string(),
                        build_rate_limit_error_content(tool_name, retry_after_ms),
                    );
                    return Ok(Some(ValidationResult::Blocked));
                }

                if bounded_wait.is_zero() {
                    tokio::task::yield_now().await;
                    continue;
                }

                tokio::select! {
                    _ = tokio::time::sleep(bounded_wait) => {},
                    _ = ctx.ctrl_c_notify.notified() => {
                        if ctx.ctrl_c_state.is_exit_requested() {
                            return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                                TurnLoopResult::Exit,
                            ))));
                        }
                        if ctx.ctrl_c_state.is_cancel_requested() {
                            return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                                TurnLoopResult::Cancelled,
                            ))));
                        }
                    }
                }
            }
        }
    }

    Ok(Some(ValidationResult::Blocked))
}

#[cfg(test)]
mod tests {
    use super::{
        build_validation_error_content_with_fallback, loop_detection_tool_key,
        preflight_validation_fallback, spool_chunk_read_path, task_tracker_create_signature,
    };
    use anyhow::anyhow;
    use serde_json::json;
    use vtcode_core::config::constants::tools as tool_names;

    #[test]
    fn loop_key_for_unified_file_read_includes_path() {
        let key = loop_detection_tool_key(
            tool_names::UNIFIED_FILE,
            &json!({"action":"read","path":"src/main.rs"}),
        );
        assert!(key.contains("unified_file::read::"));
        assert!(key.contains("src/main.rs"));
    }

    #[test]
    fn loop_key_for_unified_file_edit_includes_path() {
        let key = loop_detection_tool_key(
            tool_names::UNIFIED_FILE,
            &json!({"action":"edit","path":"src/main.rs","old_str":"old","new_str":"new"}),
        );
        assert!(key.contains("unified_file::edit::"));
        assert!(key.contains("src/main.rs"));
    }

    #[test]
    fn loop_key_for_unified_file_edit_differs_by_target_path() {
        let first = loop_detection_tool_key(
            tool_names::UNIFIED_FILE,
            &json!({"action":"edit","path":"src/main.rs","old_str":"a","new_str":"b"}),
        );
        let second = loop_detection_tool_key(
            tool_names::UNIFIED_FILE,
            &json!({"action":"edit","path":"src/lib.rs","old_str":"a","new_str":"b"}),
        );
        assert_ne!(first, second);
    }

    #[test]
    fn loop_key_for_unified_file_move_includes_source_and_destination() {
        let key = loop_detection_tool_key(
            tool_names::UNIFIED_FILE,
            &json!({"action":"move","path":"src/old.rs","destination":"src/new.rs"}),
        );
        assert!(key.contains("unified_file::move::"));
        assert!(key.contains("src/old.rs"));
        assert!(key.contains("src/new.rs"));
    }

    #[test]
    fn loop_key_for_unified_exec_poll_includes_session_id() {
        let key = loop_detection_tool_key(
            tool_names::UNIFIED_EXEC,
            &json!({"action":"poll","session_id":"run-abc123"}),
        );
        assert!(key.contains("unified_exec::poll::"));
        assert!(key.contains("run-abc123"));
    }

    #[test]
    fn loop_key_for_read_file_includes_path_and_offset() {
        let key = loop_detection_tool_key(
            tool_names::READ_FILE,
            &json!({
                "path": ".vtcode/context/tool_outputs/unified_exec_123.txt",
                "offset": 41,
                "limit": 40
            }),
        );
        assert!(key.contains("read_file::"));
        assert!(key.contains(".vtcode/context/tool_outputs/unified_exec_123.txt"));
        assert!(key.contains("offset=41"));
        assert!(key.contains("limit=40"));
    }

    #[test]
    fn loop_key_for_apply_patch_includes_target_and_signature() {
        let key = loop_detection_tool_key(
            tool_names::APPLY_PATCH,
            &json!({
                "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch\n"
            }),
        );
        assert!(key.contains("apply_patch::"));
        assert!(key.contains("src/lib.rs"));
        assert!(key.contains("len"));
        assert!(key.contains("fnv"));
    }

    #[test]
    fn spool_chunk_read_path_detects_spooled_read_calls() {
        let args = json!({
            "path": ".vtcode/context/tool_outputs/unified_exec_123.txt",
            "offset": 41,
            "limit": 40
        });
        let path = spool_chunk_read_path(tool_names::READ_FILE, &args);
        assert_eq!(
            path,
            Some(".vtcode/context/tool_outputs/unified_exec_123.txt")
        );
    }

    #[test]
    fn spool_chunk_read_path_ignores_regular_reads() {
        let args = json!({
            "path": "src/main.rs",
            "offset": 1,
            "limit": 100
        });
        let path = spool_chunk_read_path(tool_names::READ_FILE, &args);
        assert_eq!(path, None);
    }

    #[test]
    fn preflight_fallback_normalizes_unified_search_args() {
        let error = anyhow!(
            "Invalid arguments for tool 'unified_search': \"action\" is a required property"
        );
        let args = json!({
            "Pattern": "LLMStreamEvent::",
            "Path": "."
        });
        let fallback = preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
            .expect("fallback expected for recoverable unified_search preflight");
        assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
        assert_eq!(fallback.1["action"], "grep");
        assert_eq!(fallback.1["pattern"], "LLMStreamEvent::");
    }

    #[test]
    fn validation_error_payload_includes_fallback_metadata() {
        let payload = build_validation_error_content_with_fallback(
            "Tool preflight validation failed: x".to_string(),
            "preflight",
            Some(tool_names::UNIFIED_SEARCH.to_string()),
            Some(json!({"action":"grep","pattern":"foo","path":"."})),
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&payload).expect("validation payload should be json");
        assert_eq!(parsed["error_class"], "invalid_arguments");
        assert_eq!(parsed["is_recoverable"], true);
        assert_eq!(parsed["fallback_tool"], tool_names::UNIFIED_SEARCH);
        assert_eq!(parsed["fallback_tool_args"]["action"], "grep");
    }

    #[test]
    fn task_tracker_create_signature_matches_identical_payloads() {
        let first = json!({
            "action": "create",
            "title": "Fix clippy warnings",
            "items": ["A", "B"],
            "notes": "n"
        });
        let second = json!({
            "action": "create",
            "title": "Fix clippy warnings",
            "items": ["A", "B"],
            "notes": "n"
        });
        let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
        let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn task_tracker_create_signature_ignores_non_create_calls() {
        let args = json!({
            "action": "update",
            "index": 1,
            "status": "completed"
        });
        let sig = task_tracker_create_signature(tool_names::TASK_TRACKER, &args);
        assert!(sig.is_none());
    }
}
