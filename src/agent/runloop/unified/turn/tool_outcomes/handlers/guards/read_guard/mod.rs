//! Guards for file read operations.
//!
//! Contains two guards:
//! 1. **Read-after-write guard**: Blocks bare full reads of a file written
//!    this turn (the write response carries a diff preview); bounded slice
//!    reads (explicit offset/limit/page) are admitted
//! 2. **Repeated read-only call guard**: Prevents excessive reads of the same file
//!
//! The repeated read guard uses a two-tier approach:
//! - **Family cap**: Catches identical slice retries (same path + same offset/limit)
//! - **Per-file-path cap**: Catches paginated reads of the same file (different offsets)

use serde_json::{Value, json};
use vtcode_core::config::constants::tools as tool_names;

use super::super::ValidationResult;
use super::super::looping::low_signal_family_key;
use super::common::{extract_read_path, is_read_action, push_guard_failure_messages};
use crate::agent::runloop::git::normalize_workspace_path;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{find_duplicate_in_history, signature_key_for};
use crate::agent::runloop::unified::turn::tool_outcomes::read_extent;
use crate::agent::runloop::unified::turn::tool_outcomes::response_content::maybe_inline_spooled;

/// Maximum consecutive reads of the same file with the same slice (offset/limit/raw).
const MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS: usize = 4;

/// Per-file-path read cap, independent of slice (offset/limit/raw). Catches
/// paginated reads of the same file that the slice-aware family key lets
/// through. Set higher than the family cap to allow legitimate pagination
/// (e.g., reading a large file in 3-4 chunks) while stopping excessive
/// re-reads (8+ reads of the same file with different offsets).
const MAX_SAME_FILE_PATH_READ_CALLS: usize = 6;

/// Planning doubles both read caps, mirroring the generous planning research
/// budget (120 calls/turn floor) and the wider blocked-call fuse in plan mode.
/// Execution mode keeps the strict caps so genuine loops still converge.
pub(crate) fn effective_read_family_cap(planning_active: bool) -> usize {
    if planning_active {
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS.saturating_mul(2)
    } else {
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS
    }
}

/// Planning-aware per-file-path cap. See [`effective_read_family_cap`].
pub(crate) fn effective_read_path_cap(planning_active: bool) -> usize {
    if planning_active {
        MAX_SAME_FILE_PATH_READ_CALLS.saturating_mul(2)
    } else {
        MAX_SAME_FILE_PATH_READ_CALLS
    }
}

/// Decision returned by `check_read_family_cap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadFamilyCapDecision {
    /// No family-key applies (non-read tool), or the streak is still under the cap.
    BelowCap,
    /// The streak reached the cap.
    Tripped {
        /// Human-readable target extracted from the family key.
        target: String,
        /// System-facing reason describing why recovery was scheduled.
        block_reason: String,
        /// Model-facing error payload (serialized JSON).
        error_content: String,
    },
}

/// Extract a human-readable target from a read-family key.
///
/// Family keys look like:
///   - `read_file::<path>`
///   - `unified_file::read::<path>`
///   - `unified_file::read::<path>::off=N::lim=M::raw=bool`
///
/// The slice-suffix segments (`off=`, `lim=`, `raw=`) are stripped.
pub(crate) fn read_family_target(family_key: &str) -> String {
    let mut segments = family_key.split("::");
    // Skip the leading tool name (`read_file`/`unified_file`).
    segments.next();
    // The next segment is the action marker (`read`) for unified tools,
    // or the path itself for `read_file`. Skip it only if it is an action.
    let second = segments.next().unwrap_or("");
    if !matches!(second, "read" | "run") {
        // `read_file::<path>` — the second segment IS the target.
        if !second.is_empty()
            && !second.starts_with("off=")
            && !second.starts_with("lim=")
            && !second.starts_with("raw=")
        {
            return second.to_string();
        }
    }
    segments
        .filter(|segment| {
            !segment.is_empty()
                && !segment.starts_with("off=")
                && !segment.starts_with("lim=")
                && !segment.starts_with("raw=")
        })
        .next()
        .unwrap_or("current file")
        .to_string()
}

/// Pure decision: does this read-family streak trip the per-turn cap?
pub(crate) fn check_read_family_cap(
    canonical_tool_name: &str,
    effective_args: &Value,
    streak: usize,
    cap: usize,
    planning_active: bool,
) -> ReadFamilyCapDecision {
    let Some(family_key) = repeated_file_read_family_key(canonical_tool_name, effective_args) else {
        return ReadFamilyCapDecision::BelowCap;
    };
    if streak < cap {
        return ReadFamilyCapDecision::BelowCap;
    }
    let target = if canonical_tool_name == tool_names::CODE_SEARCH {
        normalised_code_search_path(effective_args).unwrap_or_else(|| "workspace".to_string())
    } else {
        read_family_target(&family_key)
    };
    let block_reason = if planning_active {
        format!(
            "Repeated read-only exploration of '{target}' hit the per-turn family cap ({cap}). Scheduling a final recovery pass without more tools; synthesize the `<proposed_plan>` from evidence already gathered."
        )
    } else {
        format!(
            "Repeated read-only exploration of '{target}' hit the per-turn family cap ({cap}). Scheduling a final recovery pass without more tools."
        )
    };
    let error_content = build_repeated_file_read_family_error_content_for_mode(&target, planning_active);
    ReadFamilyCapDecision::Tripped { target, block_reason, error_content }
}

/// Serve a cache/history reuse verdict for a read-only call.
///
/// `register_signature` distinguishes the exact-args path (signature already
/// recorded — no-op needed) from the read-target path (record it now so a
/// later identical call takes the cheaper exact-args lookup).
///
/// Content is served intact: issue #680 established that stripping the payload
/// from a reused read leaves the model with no data and makes it hallucinate or
/// spin, and the durable `tool_output` event shares this string, so events and
/// persistence must keep the full result.
fn reuse_read_only_result(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &Value,
    mut reused_value: Value,
    register_signature: bool,
) {
    if let Some(obj) = reused_value.as_object_mut() {
        super::super::apply_reused_read_only_loop_metadata(obj);
    }
    let content = maybe_inline_spooled(canonical_tool_name, &reused_value);
    ctx.push_reused_tool_response(tool_call_id, canonical_tool_name, effective_args, content);
    if register_signature {
        let signature = signature_key_for(canonical_tool_name, effective_args);
        ctx.harness_state.record_successful_readonly_signature(signature);
    }
    ctx.harness_state.record_reused_result();
}

/// Get the family key for a read-file call.
fn repeated_file_read_family_key(canonical_tool_name: &str, args: &Value) -> Option<String> {
    use super::super::looping::spool_chunk_read_path;

    if spool_chunk_read_path(canonical_tool_name, args).is_some() {
        return None;
    }

    match canonical_tool_name {
        tool_names::READ_FILE | tool_names::UNIFIED_FILE => low_signal_family_key(canonical_tool_name, args),
        tool_names::CODE_SEARCH => vtcode_core::tools::normalised_code_search_loop_identity(args)
            .map(|identity| format!("code_search::{identity}")),
        tool_names::UNIFIED_EXEC | tool_names::EXEC_COMMAND | "command_session" => {
            if let Some(exec_read) = vtcode_core::tools::parse_exec_read_target(args) {
                let slice_suffix = exec_read_slice_suffix(&exec_read);
                return Some(format!("unified_exec::read::{}{}", exec_read.path, slice_suffix));
            }
            // Track remaining file-reading shell commands in the family guard to
            // prevent bypass via unified_exec. Simple `sed -n` ranges and `awk`
            // `NR`-range measurements are handled above; only commands on the
            // is_readonly_unified_exec_command allowlist (tool_intent.rs)
            // reach this fallback — cat, head, tail, bat.
            let parts = vtcode_core::tools::command_args::command_words(args).ok()??;
            let command_name = parts.first()?.as_str();
            if !matches!(command_name, "cat" | "head" | "tail" | "bat") {
                return None;
            }
            // Use the full command as the family key so different files are tracked separately
            let command_str = parts.join(" ");
            Some(format!("unified_exec::run::{command_str}"))
        }
        _ => None,
    }
}

/// Family-key slice encoding shared with the core `ExecReadTarget` identity so
/// the guard and replay can never disagree on the shape.
fn exec_read_slice_suffix(exec_read: &vtcode_core::tools::ExecReadTarget) -> String {
    let limit = exec_read.end_line.saturating_sub(exec_read.start_line).saturating_add(1);
    format!("::off={}::lim={limit}", exec_read.start_line)
}

/// Binary-crate alias over the shared core parser so guard and evidence
/// consumers keep their stable import path without re-deriving the shape.
pub(crate) fn parse_simple_exec_read_target(args: &Value) -> Option<vtcode_core::tools::ExecReadTarget> {
    vtcode_core::tools::parse_exec_read_target(args)
}

fn repeated_read_path(canonical_tool_name: &str, effective_args: &Value) -> Option<String> {
    if let Some(target) = parse_simple_exec_read_target(effective_args) {
        if matches!(canonical_tool_name, tool_names::UNIFIED_EXEC | tool_names::EXEC_COMMAND | "command_session") {
            return Some(target.path);
        }
    }

    if is_read_action(canonical_tool_name, effective_args) {
        return extract_read_path(effective_args);
    }

    if canonical_tool_name == tool_names::CODE_SEARCH {
        // Distinct `code_search` queries scoped to the same path are diverse
        // research, not paginated re-reads: the family cap already guards
        // identical searches via the query-aware loop identity
        // (`normalised_code_search_loop_identity`), whose streak resets on a
        // new query. Counting every distinct query toward the per-path total
        // tripped the cap after a handful of legitimate planning queries on
        // one file (e.g. seven distinct queries on `benches/startup.rs`).
        return None;
    }

    None
}

fn normalised_code_search_path(effective_args: &Value) -> Option<String> {
    let path = effective_args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or(".");
    Some(vtcode_core::tools::normalised_code_search_path(path))
}

/// Planning-aware variant: in plan mode the model must finalize the
/// `<proposed_plan>` from evidence already gathered instead of starting more
/// research, otherwise the tool-free recovery synthesis emits prose and the
/// turn blocks with no approval-ready draft.
/// Planning-mode next step for family-cap rejections: once the same read
/// family repeats, more reads add no evidence.
const PLANNING_READ_CAP_NEXT_STEP: &str = "Synthesize the `<proposed_plan>` from the output already gathered; \
re-reading files already read this turn adds no new evidence.";

#[cold]
fn build_repeated_file_read_family_error_content_for_mode(target: &str, planning_active: bool) -> String {
    let next_step = if planning_active {
        PLANNING_READ_CAP_NEXT_STEP
    } else {
        "Reuse the output already gathered or try a different approach."
    };
    let guidance = format!(
        "Repeated exploration of the same file or path ('{target}') exceeded the per-turn cap, so further reads of it are blocked this turn. {next_step}"
    );
    super::super::super::execution_result::build_error_content(guidance, None, None, "repeated_read_family").to_string()
}

/// Returns the path if this is a read of a planning artifact (a runtime-owned
/// plan file or tracker) while planning mode is active.
///
/// Scoped to `.vtcode/plans/` and `.vtcode/tasks/` so ordinary markdown docs
/// and paths that merely contain `plan` (for example `planning_workflow`)
/// do not receive plan-specific reuse guidance.
fn is_plan_artifact_read(canonical_tool_name: &str, args: &Value) -> Option<String> {
    if !is_read_action(canonical_tool_name, args) {
        return None;
    }
    let path = extract_read_path(args)?;
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if lower.contains(".vtcode/plans/") || lower.contains(".vtcode/tasks/") || lower.ends_with(".tasks.md") {
        Some(path)
    } else {
        None
    }
}

/// Build the error content for a read-after-write guard trip.
#[cold]
fn build_read_after_write_error(path: &str) -> String {
    super::super::super::execution_result::build_error_content(
        format!(
            "File '{path}' was just written in this turn. The write response includes a diff preview. Reuse the diff output or specify offset/limit for a specific range."
        ),
        None,
        None,
        "read_after_write",
    )
    .to_string()
}

/// Enforce the read-after-write guard.
///
/// Blocks bare full reads of a path written this turn: the write response
/// already carries a diff preview, so a full re-read only duplicates context.
/// A bounded slice read (explicit offset/limit/page under the shared
/// alias vocabulary) is admitted — it is the deliberate targeted inspection
/// the block message directs the model toward, and repeated slice reads stay
/// bounded by the family/per-path caps enforced right after this guard.
/// A `raw` flag alone does not admit: an uncondensed full re-read is the most
/// wasteful variant the guard exists to stop.
///
/// Returns `Some(ValidationResult::Blocked)` when the guard trips,
/// or `None` when the guard passes.
pub(crate) fn enforce_read_after_write_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &Value,
) -> Option<ValidationResult> {
    if !is_read_action(canonical_tool_name, effective_args) {
        return None;
    }

    let path = extract_read_path(effective_args)?;

    // Both sides of the membership check are normalized against the workspace
    // root: patch payloads record workspace-relative targets while reads may
    // spell the same file absolutely (or vice versa).
    let normalized = normalize_workspace_path(&ctx.config.workspace, std::path::Path::new(&path));
    if !ctx.harness_state.was_recently_written(&normalized.to_string_lossy()) {
        return None;
    }

    if read_extent::args_have_bounded_extent(effective_args) {
        return None;
    }

    let content = build_read_after_write_error(&path);
    ctx.push_rejected_tool_response(tool_call_id, Some(canonical_tool_name), Some(effective_args), content);
    Some(ValidationResult::Blocked)
}

/// Enforce the repeated read-only call guard.
///
/// Uses a two-tier approach:
/// 1. Family cap: Catches identical slice retries (same path + same offset/limit)
/// 2. Per-file-path cap: Catches paginated reads of the same file
///
/// Returns a blocked validation result when either guard trips,
/// or `None` when both guards pass.
pub(crate) fn enforce_repeated_read_only_call_guard(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &Value,
    readonly_classification: bool,
) -> Option<ValidationResult> {
    // Tracker calls are read-only with respect to workspace files, but their
    // state changes independently of file-read signatures and history caches.
    // Both list and updates must execute against the current checklist.
    // Session waits likewise observe live process state; a yielded result is
    // not reusable evidence, even when its captured output is unchanged.
    if !readonly_classification
        || canonical_tool_name == tool_names::TASK_TRACKER
        || (vtcode_core::tools::tool_intent::is_command_session_tool(canonical_tool_name)
            && vtcode_core::tools::tool_intent::command_session_action_is(effective_args, "wait"))
        || ctx
            .tool_registry
            .is_completion_verifier_call(canonical_tool_name, effective_args)
    {
        return None;
    }

    // Planning doubles the read caps, mirroring the generous planning research
    // budget (120 calls/turn floor) and the wider blocked-call fuse in plan mode.
    // Execution mode keeps the strict caps so genuine loops still converge.
    let planning_active = ctx.tool_registry.is_planning_active();
    let family_cap = effective_read_family_cap(planning_active);
    let path_cap = effective_read_path_cap(planning_active);
    let signature = signature_key_for(canonical_tool_name, effective_args);

    // Plan-artifact fast path: serve runtime-owned plan/tracker re-reads
    // WITHOUT advancing the family/path counters. Planning synthesis
    // legitimately re-reads its own draft, so counting those toward loop caps
    // starves synthesis. This stays scoped to `.vtcode/plans/`,
    // `.vtcode/tasks/`, and `*.tasks.md` so ordinary docs never bypass caps.
    // All other reads fall through to the caps below first, so identical-slice
    // retry loops still trip instead of looping forever on cache hits.
    let plan_path = planning_active
        .then(|| is_plan_artifact_read(canonical_tool_name, effective_args))
        .flatten();
    let plan_lookup_done = plan_path.is_some();
    if let Some(plan_path) = plan_path.as_deref() {
        if let Some(mut reused_value) = ctx.tool_registry.find_recent_successful_by_read_target(
            canonical_tool_name,
            effective_args,
            ctx.harness_state.max_tool_wall_clock,
        ) {
            if let Some(obj) = reused_value.as_object_mut() {
                super::super::apply_reused_read_only_loop_metadata(obj);
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
            ctx.push_reused_tool_response(
                tool_call_id,
                canonical_tool_name,
                effective_args,
                maybe_inline_spooled(canonical_tool_name, &reused_value),
            );
            ctx.harness_state.record_successful_readonly_signature(signature);
            ctx.harness_state.record_reused_result();
            return Some(ValidationResult::Handled);
        }
    }

    // Reserve the single fresh mismatch read before either read cap. Otherwise
    // the identical-slice guard can block recovery before the path exception.
    let recovery_allowed = ctx
        .tool_registry
        .pending_patch_recovery_read_path(canonical_tool_name, effective_args)
        .is_some_and(|path| ctx.harness_state.claim_patch_recovery_path(path));

    if let Some(family_key) = repeated_file_read_family_key(canonical_tool_name, effective_args) {
        // The streak mutation is stateful and stays here; the cap *decision*
        // is delegated to the pure `check_read_family_cap` helper so it can be
        // tested without the full TurnProcessingContext harness.
        let streak = ctx.harness_state.record_file_read_family_call(family_key);
        if let ReadFamilyCapDecision::Tripped { target: _, block_reason, error_content } =
            check_read_family_cap(canonical_tool_name, effective_args, streak, family_cap, planning_active)
            && !recovery_allowed
        {
            ctx.activate_recovery(block_reason.clone());
            push_guard_failure_messages(ctx, tool_call_id, canonical_tool_name, error_content, &block_reason);
            return Some(ValidationResult::Blocked);
        }
    }

    // Per-file-path cap: catches paginated reads of the same file that the
    // slice-aware family key lets through (e.g., 8 reads of anthropic_types.rs
    // at different offsets each get a different family key and never collide).
    // `code_search` is excluded from this cap (see `repeated_read_path`):
    // distinct queries on one path are diverse research guarded by the
    // query-aware family cap above, not pagination.
    if let Some(path) = repeated_read_path(canonical_tool_name, effective_args) {
        // Count by workspace-normalized path so the same file read via
        // different spellings (relative vs absolute) shares one counter —
        // and so a later write, recorded normalized, resets this exact key.
        // The model-facing message keeps the raw spelling it used.
        let normalized = normalize_workspace_path(&ctx.config.workspace, std::path::Path::new(&path))
            .to_string_lossy()
            .to_string();
        let path_count = ctx.harness_state.record_file_read_path_call(normalized);
        if path_count > path_cap && !recovery_allowed {
            let block_reason = format!(
                "Repeated reads of '{path}' hit the per-file-path cap ({path_cap}), so further reads of this path are blocked for the rest of this turn. Prefer wider windows (e.g. sed -n '1,200p' or read_file) over overlapping slices and reuse spooled evidence; reads of other paths, edits, and other useful actions remain available."
            );
            let error_content = super::super::super::execution_result::build_error_content(
                block_reason.clone(),
                None,
                None,
                "repeated_read_path",
            )
            .to_string();
            push_guard_failure_messages(ctx, tool_call_id, canonical_tool_name, error_content, &block_reason);
            return Some(ValidationResult::ReadCapBlocked);
        }
    }

    // Recovery still advances family/path counters and grants only one exception.
    // The registry consumes the allowance only on execution, bypassing both
    // replay reuse here and its own caches without discarding loop history.
    if ctx.tool_registry.has_patch_recovery_read(canonical_tool_name, effective_args) {
        return None;
    }

    // Cap-first: exact duplicates, cross-turn TTL matches, and history
    // duplicates are served only after the counters above have advanced. This
    // preserves the identical-slice loop guard: serving a cached hit must not
    // hide a retry loop that should force tool-free recovery.
    if ctx.harness_state.has_successful_readonly_signature(signature.as_str())
        && let Some(reused_value) = ctx.tool_registry.find_recent_successful_output(
            canonical_tool_name,
            effective_args,
            ctx.harness_state.max_tool_wall_clock,
        )
    {
        reuse_read_only_result(ctx, tool_call_id, canonical_tool_name, effective_args, reused_value, false);
        return Some(ValidationResult::Handled);
    }

    // Cross-turn TTL-bounded cache (covers same-path different-offset supersets
    // via `read_extent_matches`). Plan artifacts already looked this up before
    // the caps above, so skip the duplicate lookup for them.
    if !plan_lookup_done
        && let Some(reused_value) = ctx.tool_registry.find_recent_successful_by_read_target(
            canonical_tool_name,
            effective_args,
            ctx.harness_state.max_tool_wall_clock,
        )
    {
        reuse_read_only_result(ctx, tool_call_id, canonical_tool_name, effective_args, reused_value, true);
        return Some(ValidationResult::Handled);
    }

    // Cross-turn duplicate: scan working history.
    if let Some(raw_output) = find_duplicate_in_history(
        ctx.working_history,
        canonical_tool_name,
        effective_args,
        ctx.tool_registry.workspace_root(),
    ) {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&raw_output) {
            if let Some(obj) = parsed.as_object_mut() {
                super::super::apply_reused_read_only_loop_metadata(obj);
            }
            ctx.push_reused_tool_response(
                tool_call_id,
                canonical_tool_name,
                effective_args,
                maybe_inline_spooled(canonical_tool_name, &parsed),
            );
        } else {
            ctx.push_reused_tool_response(tool_call_id, canonical_tool_name, effective_args, raw_output);
        }
        ctx.harness_state.record_reused_result();
        return Some(ValidationResult::Handled);
    }

    None
}

#[cfg(test)]
mod tests;
