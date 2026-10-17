//! Guard for spool chunk reads.
//!
//! Tracks sequential reads of spool files (tool output files). When the cap is
//! reached, recovery is activated to prevent the model from paginating through
//! large spool files indefinitely.
//!
//! The guard also short-circuits when a spool file contains an error payload
//! from a previous turn, injecting the error inline instead of reading.

use std::path::Path;

use serde_json::{Value, json};
use vtcode_commons::formatting::truncate_byte_budget;
use vtcode_commons::preview::condense_text_bytes;
use vtcode_core::config::constants::defaults::DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::labels::tool_action_label;

use super::super::ValidationResult;
use super::common::push_guard_failure_messages;
use crate::agent::runloop::unified::tool_reads::{
    read_spool_head_for_error_check, spool_content_looks_like_error, spool_page_source_path,
};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

const SPOOL_CHUNK_GREP_PATTERN: &str = "warning|error|TODO";
const SPOOL_CHUNK_INLINE_MAX_BYTES: usize = 32 * 1024;
const SPOOL_CHUNK_INLINE_HEAD_BYTES: usize = 8 * 1024;
const SPOOL_CHUNK_INLINE_TAIL_BYTES: usize = 8 * 1024;

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn public_spool_grep_args(path: &str, pattern: &str) -> Value {
    json!({
        "cmd": format!(
            "rg --line-number --column --color=never {} {}",
            shell_single_quote(pattern),
            shell_single_quote(path)
        ),
        "max_output_tokens": 4000
    })
}

/// Read a spool file's content for inline embedding when the spool-chunk
/// guard trips. Returns `None` if the file is missing, empty, or unreadable.
///
/// The preview is opened through the workspace-bound no-follow helper just
/// like ordinary spool reads. This keeps a concurrent symlink retarget from
/// exposing an unrelated file while the guard is constructing its response.
async fn read_spool_preview_for_guard(workspace: &Path, path: &str) -> Option<String> {
    let workspace = workspace.to_path_buf();
    let path = path.to_string();
    tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
        let reference = vtcode_core::tools::SpooledOutputReference {
            spool_path: &path,
            preview: None,
            original_bytes: None,
            sha256: None,
            state: vtcode_core::tools::SpoolState::Pending,
        };
        let content = reference.read_pending_bounded(&workspace, SPOOL_CHUNK_INLINE_MAX_BYTES)?;
        if content.is_empty() {
            return Ok(None);
        }
        // This is only the bounded prefix read above, never the full file.
        // Reuse byte-aware condensation so multibyte text cannot inflate it.
        Ok(Some(condense_text_bytes(&content, SPOOL_CHUNK_INLINE_HEAD_BYTES, SPOOL_CHUNK_INLINE_TAIL_BYTES)))
    })
    .await
    .ok()?
    .ok()
    .flatten()
}

/// Inspect the spool filename and try to derive a useful fallback tool call
/// that resumes the original workflow rather than the generic
/// `grep warning|error|TODO` placeholder.
fn derive_spool_fallback(path: &str) -> Option<(String, Value)> {
    let file_name = Path::new(path).file_name()?.to_str()?.to_string();

    if let Some(sid) = file_name.strip_suffix(".txt").and_then(|stem| stem.strip_prefix("run-")) {
        return Some((
            tool_names::WRITE_STDIN.to_string(),
            json!({
                "session_id": sid,
                "chars": "",
                "yield_time_ms": 1000,
            }),
        ));
    }

    let stem = file_name.strip_suffix(".txt")?;
    let prefix = stem.split('_').next()?;
    match prefix {
        "unified" | "outline" | "search" => {
            Some((tool_names::EXEC_COMMAND.to_string(), public_spool_grep_args(path, ".")))
        }
        _ => None,
    }
}

/// Get the max sequential spool chunk reads per turn from config.
fn max_sequential_spool_chunk_reads_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_sequential_spool_chunk_reads)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN)
}

/// Build fallback args for spool chunk guard.
fn spool_chunk_guard_fallback_args(path: &str) -> Value {
    derive_spool_fallback(path)
        .map(|(_, args)| args)
        .unwrap_or_else(|| public_spool_grep_args(path, SPOOL_CHUNK_GREP_PATTERN))
}

/// Build fallback tool for spool chunk guard.
fn spool_chunk_guard_fallback_tool(path: &str) -> Option<String> {
    if let Some((tool, _)) = derive_spool_fallback(path) {
        Some(tool)
    } else {
        Some(tool_names::EXEC_COMMAND.to_string())
    }
}

/// Build the error content for a spool chunk guard trip.
#[cold]
async fn build_spool_chunk_guard_error_content(workspace: &Path, path: &str, max_reads_per_turn: usize) -> String {
    let inline_content = read_spool_preview_for_guard(workspace, path).await;
    let fallback_tool = spool_chunk_guard_fallback_tool(path);
    let fallback_args = spool_chunk_guard_fallback_args(path);

    let mut payload = super::super::super::execution_result::build_error_content(
        format!(
            "Spool chunk reads exceeded per-turn cap ({max_reads_per_turn}). Use targeted extraction before reading more from '{path}'."
        ),
        fallback_tool.clone(),
        Some(fallback_args),
        "spool_chunk_guard",
    );

    if let Some(obj) = payload.as_object_mut() {
        obj.insert("loop_detected".to_string(), Value::Bool(true));
        obj.insert("recovery_required".to_string(), Value::Bool(true));
        obj.insert(
            "next_action".to_string(),
            Value::String(
                "Further chunk reads of this spool are blocked for this turn. \
                 Continue from the `inline_content` below and the conversation \
                 history. For more inspection, use `exec_command` for targeted \
                 shell inspection, `write_stdin` for session continuation, or \
                 `apply_patch` for edits."
                    .to_string(),
            ),
        );
        if let Some(content) = inline_content {
            obj.insert("inline_content".to_string(), Value::String(content));
            obj.insert(
                "inline_content_note".to_string(),
                Value::String(
                    "Bounded snapshot of the spool prefix embedded inline; it \
                     may omit later content and is not proof of completion. \
                     The per-turn cap blocks further reads of this spool file."
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

/// Build the tool response payload for a short-circuited spool read of an
/// error payload from a previous turn.
fn build_previous_turn_error_spool_content(spool_path: &str, head: &str) -> String {
    let preview = truncate_byte_budget(head, 1024, "... [truncated]");
    serde_json::json!({
        "spool_path": spool_path,
        "loop_detected": true,
        "is_recoverable": true,
        "error_class": "previous_turn_error_spool",
        "next_action": "This spool holds an error payload from a previous turn that is already in the conversation history; re-reading it returns the same error. Choose a different tool or approach.",
        "hint": "This spool contains an error response from an earlier turn; you do not need to re-read it. Use the error message already in your history and try a different approach.",
        "inline_content": preview,
    })
    .to_string()
}

/// Enforce the spool chunk read guard.
///
/// Returns `Some(ValidationResult)` when the guard trips,
/// or `None` when the guard passes.
pub(crate) async fn enforce_spool_chunk_read_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    args: &Value,
) -> Option<ValidationResult> {
    // Spool paging across both read channels (read-file-style and shell
    // paged reads such as `sed -n`/`rg` on a spool path) shares one
    // sequential cap: without this, paging through the exec channel evades
    // the cap while paging through read-file does not.
    let Some(spool_path) = spool_page_source_path(canonical_tool_name, args) else {
        ctx.harness_state.reset_spool_chunk_read_streak();
        return None;
    };
    let spool_path = spool_path.as_str();

    // Short-circuit: if the spool file's first bytes look like an error
    // payload, the agent already has the error in its conversation history
    // from the previous turn. Inject the error inline as a tool response and
    // tell the model to use it instead of paginating the spool.
    if let Some(head) = read_spool_head_for_error_check(&ctx.config.workspace, spool_path).await {
        if spool_content_looks_like_error(&head) {
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(canonical_tool_name),
                Some(args),
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
        canonical_tool_name,
        build_spool_chunk_guard_error_content(&ctx.config.workspace, spool_path, max_reads_per_turn).await,
        &block_reason,
    );

    Some(ValidationResult::Blocked)
}

#[cfg(test)]
mod tests;
