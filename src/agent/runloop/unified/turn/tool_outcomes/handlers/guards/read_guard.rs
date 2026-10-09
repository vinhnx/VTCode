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
            if let Some(exec_read) = parse_simple_exec_read_target(args) {
                return Some(format!("unified_exec::read::{}{}", exec_read.path, exec_read.slice_suffix));
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecReadTarget {
    pub(crate) path: String,
    pub(crate) start_line: usize,
    slice_suffix: String,
    /// Whether the command output is the file's own lines. Only a bare
    /// `sed -n '<range>p' <file>` (no pipe) is verbatim: piped stages and awk
    /// programs transform the output, so it must never be fingerprinted as
    /// positioned file evidence (see `navigation_evidence`). Loop guards use
    /// path/slice regardless of this flag.
    pub(crate) verbatim: bool,
}

/// Lexically normalize a shell-measurement target path so spelling variants
/// (`./README.md`, `docs/../README.md`) share one family key, path count, and
/// evidence identity. Reuses the shared code_search loop-identity normalizer:
/// leading `..` is preserved (fail-closed, never collides with in-workspace
/// entries), so workspace escape via `../` cannot masquerade as a local read.
fn normalised_shell_read_path(path: &str) -> String {
    vtcode_core::tools::normalised_code_search_path(path)
}

pub(crate) fn parse_simple_exec_read_target(args: &Value) -> Option<ExecReadTarget> {
    let parts = vtcode_core::tools::command_args::command_words(args).ok()??;
    // Measurement loops hide a simple read behind a pipe
    // (`sed -n '65,71p' README.md | awk ...`, `awk 'NR...' README.md | cat`):
    // only the leading pipeline stage determines what is read, so parse that
    // and ignore trailing pipe stages. Only `|` is split: `;`/`&&`/`||`
    // compounds keep their legacy untracked behavior (the full word list
    // fails the simple-shape parse below), which under-counts rather than
    // over-blocks multi-read one-liners.
    let leading = leading_shell_segment(parts.as_slice());
    let piped = leading.len() != parts.len();
    parse_simple_sed_read_target(leading)
        .or_else(|| parse_simple_awk_read_target(leading))
        .map(|mut target| {
            // A stripped pipe stage transforms the output downstream, so the
            // lines that reach the model are not the file's own lines.
            target.verbatim = target.verbatim && !piped;
            target
        })
}

/// Leading pipeline stage before the first `|` token. `command_words` splits
/// via `shell_words`, so a `|` inside a quoted awk program stays embedded in
/// the program word and never splits; only a standalone pipe operator cuts.
fn leading_shell_segment(parts: &[String]) -> &[String] {
    let end = parts.iter().position(|part| part.as_str() == "|").unwrap_or(parts.len());
    &parts[..end]
}

/// Parse `awk '<program>' <single-file>` ranged measurement reads.
///
/// KISS scope: exactly `awk`, one program word, one file word — no options
/// (`-F`, `-v` fail closed to `None`), no multi-file scans, and the program
/// must reference an `NR` line range (see [`awk_nr_line_range`]). Whole-file
/// scans stay untracked: distinct whole-file queries are diverse research, not
/// pagination (mirrors the `code_search` per-path exclusion), so counting them
/// would false-trip the path cap. The read guard only runs for
/// readonly-classified calls, so mutating programs (`>`, `|`, `system()`, `@`)
/// never reach here; no need to re-scan the program for writes. The file word
/// must not look like a flag. Awk output is always computed, never the file's
/// own lines, so `verbatim` is always false.
fn parse_simple_awk_read_target(parts: &[String]) -> Option<ExecReadTarget> {
    if parts.len() != 3 || parts.first().map(String::as_str) != Some("awk") {
        return None;
    }
    let program = parts.get(1)?.as_str();
    if program.trim().is_empty() {
        return None;
    }
    let path = parts.get(2)?.as_str();
    if path.starts_with('-') || path.is_empty() {
        return None;
    }

    let (start, end) = awk_nr_line_range(program)?;
    let limit = end.saturating_sub(start).saturating_add(1);
    Some(ExecReadTarget {
        path: normalised_shell_read_path(path),
        start_line: start,
        slice_suffix: format!("::off={start}::lim={limit}"),
        verbatim: false,
    })
}

/// Extract the line range referenced by `NR` comparisons in an awk program.
///
/// Scans for `NR` then an optional comparison operator (`>`, `<`, `=`, `!`
/// combos) then a decimal number. Only numbers in that position count, so
/// thresholds like `length > 120` (no `NR`) do not masquerade as line ranges.
/// Requires at least two numbers: a single `NR` reference (`NR>1` header-skip,
/// one `NR==67` probe, one `/NR==65/` regex hit) cannot delimit a range, and
/// grouping all such open-ended scans by path alone would trip the family cap
/// on diverse column queries over the same file. Returns `(min, max)` over
/// all matches.
fn awk_nr_line_range(program: &str) -> Option<(usize, usize)> {
    let bytes = program.as_bytes();
    let mut numbers: Vec<usize> = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'N' && bytes[index + 1] == b'R' {
            let mut cursor = index + 2;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            while cursor < bytes.len() && matches!(bytes[cursor], b'>' | b'<' | b'=' | b'!') {
                cursor += 1;
            }
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if start < cursor
                && let Ok(number) = program[start..cursor].parse::<usize>()
            {
                numbers.push(number);
            }
            index = cursor.max(index + 1);
        } else {
            index += 1;
        }
    }
    let min = *numbers.iter().min()?;
    let max = *numbers.iter().max()?;
    // A lone `NR` reference cannot delimit a range (see doc comment); without
    // this, every `NR>1` header-skip would collapse to `off=1::lim=1` and trip
    // the family cap on diverse same-file queries.
    if numbers.len() < 2 {
        return None;
    }
    // Sanity: line numbers, not byte counts. Ranges spanning millions of
    // lines are not real file slices; leave untracked.
    if max == 0 || max.saturating_sub(min) > 1_000_000 {
        return None;
    }
    Some((min, max))
}

fn parse_simple_sed_read_target(parts: &[String]) -> Option<ExecReadTarget> {
    if parts.first().map(String::as_str) != Some("sed") {
        return None;
    }

    let mut cursor = 1usize;
    if parts.get(cursor).map(String::as_str) != Some("-n") {
        return None;
    }
    cursor += 1;

    let script = parts.get(cursor)?.as_str();
    cursor += 1;

    let path = parts.get(cursor)?.as_str();
    if cursor + 1 != parts.len() {
        return None;
    }
    // Fail closed on flag-like paths (`sed -n '1p' --help`); mirrors the awk
    // parser below. `-` (stdin) is not a file read either.
    if path.starts_with('-') {
        return None;
    }

    let (start, end) = parse_simple_sed_print_range(script)?;
    let limit = end.saturating_sub(start).saturating_add(1);
    Some(ExecReadTarget {
        path: normalised_shell_read_path(path),
        start_line: start,
        slice_suffix: format!("::off={start}::lim={limit}"),
        // Bare `sed -n '<range>p' <file>` prints the file's own lines. The
        // caller clears this when a pipe stage was stripped.
        verbatim: true,
    })
}

fn parse_simple_sed_print_range(script: &str) -> Option<(usize, usize)> {
    let range = script.strip_suffix('p')?;
    let (start, end) = match range.split_once(',') {
        Some((start, end)) => (start, end),
        None => (range, range),
    };

    let start = start.parse::<usize>().ok()?;
    let end = end.parse::<usize>().ok()?;
    (start <= end).then_some((start, end))
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
    if !readonly_classification {
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
        let path_count = ctx.harness_state.record_file_read_path_call(path.clone());
        if path_count > path_cap && !recovery_allowed {
            let block_reason = format!(
                "Repeated reads of '{path}' hit the per-file-path cap ({path_cap}), so further reads of this path are blocked for the rest of this turn. Reads of other paths, edits, and other useful actions remain available; continue from the evidence already gathered."
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
        && let Some(mut reused_value) = ctx.tool_registry.find_recent_successful_output(
            canonical_tool_name,
            effective_args,
            ctx.harness_state.max_tool_wall_clock,
        )
    {
        if let Some(obj) = reused_value.as_object_mut() {
            super::super::apply_reused_read_only_loop_metadata(obj);
        }
        ctx.push_reused_tool_response(
            tool_call_id,
            canonical_tool_name,
            effective_args,
            maybe_inline_spooled(canonical_tool_name, &reused_value),
        );
        ctx.harness_state.record_reused_result();
        return Some(ValidationResult::Handled);
    }

    // Cross-turn TTL-bounded cache (covers same-path different-offset supersets
    // via `read_extent_matches`). Plan artifacts already looked this up before
    // the caps above, so skip the duplicate lookup for them.
    if !plan_lookup_done
        && let Some(mut reused_value) = ctx.tool_registry.find_recent_successful_by_read_target(
            canonical_tool_name,
            effective_args,
            ctx.harness_state.max_tool_wall_clock,
        )
    {
        if let Some(obj) = reused_value.as_object_mut() {
            super::super::apply_reused_read_only_loop_metadata(obj);
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
mod tests {
    use vtcode_core::config::constants::tools as tool_names;

    use super::*;

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
        assert_eq!(key, Some("unified_exec::run::head -n 10 file.txt".to_string()));
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
    fn repeated_file_read_family_key_tracks_simple_sed_ranges_via_unified_exec() {
        let args = serde_json::json!({"command": "sed -n '440,520p' Cargo.toml"});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, Some("unified_exec::read::Cargo.toml::off=440::lim=81".to_string()));
    }

    #[test]
    fn repeated_file_read_family_key_tracks_simple_sed_ranges_via_public_exec_command() {
        let args = serde_json::json!({"cmd": "sed -n '440,520p' Cargo.toml"});
        let key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args);
        assert_eq!(key, Some("unified_exec::read::Cargo.toml::off=440::lim=81".to_string()));
    }

    #[test]
    fn repeated_file_read_family_key_ignores_complex_sed_commands() {
        let args = serde_json::json!({"command": "sed -n '440,520p' Cargo.toml extra.txt"});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, None);
    }

    #[test]
    fn piped_sed_measurement_shares_family_with_bare_sed_same_range() {
        // Blocked session 20261008T094713Z: `sed -n '65,71p' README.md | awk ...`
        // measured the same table as the bare sed. Both must share one family
        // key so the consecutive family cap converges the loop.
        let bare = serde_json::json!({"cmd": "sed -n '65,71p' README.md"});
        let piped = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{n=length($0); print n}'"});
        let bare_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &bare);
        let piped_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &piped);
        assert_eq!(bare_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
        assert_eq!(piped_key, bare_key);
    }

    #[test]
    fn piped_sed_different_ranges_stay_distinct_families() {
        // Asymmetric side: diverse pagination must NOT group. Same shape as
        // above but disjoint ranges → distinct keys, no false cap trip.
        let first = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
        let second = serde_json::json!({"cmd": "sed -n '154,180p' README.md | awk '{print n}'"});
        let first_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &first);
        let second_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &second);
        assert_eq!(first_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
        assert_eq!(second_key, Some("unified_exec::read::README.md::off=154::lim=27".to_string()));
        assert_ne!(first_key, second_key);
    }

    #[test]
    fn semicolon_compound_keeps_legacy_untracked_behavior() {
        // `;` compounds carry multiple reads in one call. They keep the legacy
        // `None` (under-count rather than over-block); pipe stages above are
        // the measurement-loop shape this fix covers.
        let args = serde_json::json!({"cmd": "sed -n '65,71p' README.md; echo ---; sed -n '202,260p' README.md"});
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args), None);
        assert_eq!(parse_simple_exec_read_target(&args), None);
    }

    #[test]
    fn awk_nr_range_measurement_groups_same_slice() {
        // The late-turn loop ran `awk 'NR>=65 && NR<=71 ...' README.md` with
        // varying bodies. Same NR range → same family regardless of body.
        let first = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print NR\": \"$0}' README.md"});
        let second = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {n=length($0); print NR\": len=\"n}' README.md"});
        let first_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &first);
        let second_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &second);
        assert_eq!(first_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
        assert_eq!(second_key, first_key);
    }

    #[test]
    fn awk_nr_equality_alternatives_extract_min_max() {
        // `NR==67 || NR==68 || ... || NR==71` targets lines 67-71.
        let args = serde_json::json!({"cmd": "awk 'NR==67 || NR==68 || NR==69 || NR==70 || NR==71 {print}' README.md"});
        let key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args);
        assert_eq!(key, Some("unified_exec::read::README.md::off=67::lim=5".to_string()));
    }

    #[test]
    fn awk_whole_file_scan_stays_untracked() {
        // Asymmetric side: `length > 120` names a byte threshold, not a line
        // range (no NR) → untracked, so diverse whole-file queries never trip
        // the read caps (mirrors the `code_search` per-path exclusion).
        let whole = serde_json::json!({"cmd": "awk 'length > 120 {print}' README.md"});
        let ranged = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md"});
        assert_eq!(parse_simple_exec_read_target(&whole), None);
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &whole), None);
        assert_eq!(
            repeated_file_read_family_key(tool_names::EXEC_COMMAND, &ranged),
            Some("unified_exec::read::README.md::off=65::lim=7".to_string())
        );
    }

    #[test]
    fn awk_piped_measurement_still_counts_leading_read() {
        let args = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md | cat"});
        let key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args);
        assert_eq!(key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
    }

    #[test]
    fn only_bare_sed_is_verbatim_for_positioned_evidence() {
        // Guards count pipes and awk via path/slice, but positioned evidence
        // needs the file's own lines: only a bare `sed -n` range print is
        // verbatim. Piped and awk outputs are transformed downstream.
        let bare = serde_json::json!({"cmd": "sed -n '65,71p' README.md"});
        assert!(parse_simple_exec_read_target(&bare).is_some_and(|target| target.verbatim));
        let piped = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
        assert!(parse_simple_exec_read_target(&piped).is_some_and(|target| !target.verbatim));
        let awk = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md"});
        assert!(parse_simple_exec_read_target(&awk).is_some_and(|target| !target.verbatim));
    }

    #[test]
    fn awk_single_nr_reference_stays_untracked() {
        // One `NR` number cannot delimit a range: `NR>1` header-skips with
        // different bodies are diverse queries, not a retry loop. Grouping
        // them would trip the family cap on legitimate research.
        let first = serde_json::json!({"cmd": "awk 'NR>1 {print $1}' README.md"});
        let second = serde_json::json!({"cmd": "awk 'NR>1 {print $2}' README.md"});
        assert_eq!(parse_simple_exec_read_target(&first), None);
        assert_eq!(parse_simple_exec_read_target(&second), None);
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &first), None);
    }

    #[test]
    fn sed_flag_like_path_stays_untracked() {
        let args = serde_json::json!({"cmd": "sed -n '1p' --help"});
        assert_eq!(parse_simple_exec_read_target(&args), None);
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args), None);
    }

    #[test]
    fn awk_with_options_or_multiple_files_stays_untracked() {
        // Fail closed: options and multi-file scans are not simple reads.
        let flagged = serde_json::json!({"cmd": "awk -F: '{print $1}' README.md"});
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &flagged), None);
        let multi = serde_json::json!({"cmd": "awk '{print}' README.md CHANGELOG.md"});
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &multi), None);
    }

    #[test]
    fn grep_measurement_probe_is_not_a_read_target() {
        // `grep -c` is search, not a positioned read: distinct queries are
        // diverse research (mirrors the code_search per-path exclusion), so it
        // must stay out of the read family/path caps.
        let args = serde_json::json!({"cmd": "grep -c '—' README.md"});
        assert_eq!(parse_simple_exec_read_target(&args), None);
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args), None);
    }

    #[test]
    fn repeated_read_path_counts_piped_sed_and_awk_measurements() {
        // Per-path total (not just family streak) must see the bypass shapes,
        // so 7+ README measurements trip the path cap even with mixed bodies.
        let piped_sed = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
        let awk = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md"});
        assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &piped_sed), Some("README.md".to_string()));
        assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &awk), Some("README.md".to_string()));
    }

    #[test]
    fn spelling_variants_share_shell_family_and_path_count() {
        // `./` and interior `..` spellings are the same file: one family key
        // and one path bucket. A leading `..` escapes and stays distinct.
        let bare = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
        let dotted = serde_json::json!({"cmd": "sed -n '65,71p' ./README.md | awk '{print n}'"});
        let escaping = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' ../README.md"});
        let bare_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &bare);
        assert_eq!(bare_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
        assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &dotted), bare_key);
        assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &dotted), Some("README.md".to_string()));
        assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &escaping), Some("../README.md".to_string()));
    }

    #[test]
    fn blocked_session_measurement_loop_trips_path_cap() {
        // Incident-shaped replay: session 20261008T094713Z burned 32 tool calls
        // on consecutive `README.md:65-71` table-width probes. The per-path cap
        // (`MAX_SAME_FILE_PATH_READ_CALLS`) must trip on this sequence even
        // though the bodies vary per call. `;`-compounds, `grep -c` searches,
        // and whole-file `length` scans stay untracked by design, so the hit
        // count is below the raw call count — but still above the cap.
        let probe_shapes = [
            r#"sed -n '65,71p' README.md | cat -A | head -20"#,
            r#"awk 'NR>=65 && NR<=71 {print NR": ["$0"]"}' README.md"#,
            r#"awk 'NR>=65 && NR<=71 {print NR": "$0}' README.md | awk '{print length($0), $0}'"#,
            r#"awk 'NR==67 || NR==68 || NR==69 || NR==70 || NR==71 {print}' README.md"#,
            r#"awk 'NR>=65 && NR<=71 {print}' README.md; sed -n '65p' README.md | wc -c"#,
            r#"awk 'NR>=65 && NR<=71 {n=length($0); print NR": len="n}' README.md"#,
            r#"awk 'NR>=65 && NR<=71 {n=length($0); print NR": c="n}' README.md"#,
            r#"sed -n '65,71p' README.md | awk '{n=length($0); print n}'"#,
            r#"grep -c 'x' README.md"#,
            r#"awk 'length > 120 {print}' README.md"#,
            r#"awk 'NR==67 {print $0} NR==68 {print $0}' README.md | cat"#,
            r#"sed -n '65,71p' README.md | awk '{n=length($0); print n}'"#,
        ];
        let hits = probe_shapes
            .iter()
            .filter(|command| {
                repeated_read_path(tool_names::EXEC_COMMAND, &serde_json::json!({"cmd": command}))
                    == Some("README.md".to_string())
            })
            .count();
        assert!(
            hits > MAX_SAME_FILE_PATH_READ_CALLS,
            "incident loop must exceed the per-path cap: {hits} hits, cap {MAX_SAME_FILE_PATH_READ_CALLS}"
        );
    }

    #[test]
    fn repeated_file_read_family_key_returns_none_for_missing_command() {
        let args = serde_json::json!({});
        let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
        assert_eq!(key, None);
    }

    #[test]
    fn repeated_file_read_family_key_tracks_code_search_identity() {
        let args = serde_json::json!({"query": "HarnessTurnState", "path": "README.md"});
        let key = repeated_file_read_family_key(tool_names::CODE_SEARCH, &args)
            .expect("valid code_search should have a loop identity");
        assert!(key.starts_with("code_search::"));
        assert!(key.contains("HarnessTurnState"));
    }

    #[test]
    fn repeated_read_path_excludes_code_search_distinct_queries() {
        // Distinct `code_search` queries scoped to one path are diverse
        // research, not pagination: the query-aware family cap guards
        // identical searches, so the per-path total must not count them.
        // This is the `benches/startup.rs` regression: seven distinct queries
        // tripped the per-turn cap and blocked planning with no draft.
        assert_eq!(
            repeated_read_path(tool_names::CODE_SEARCH, &serde_json::json!({"query": "fn", "path": "README.md"})),
            None
        );
        assert_eq!(repeated_read_path(tool_names::CODE_SEARCH, &serde_json::json!({"query": "fn"})), None);
        assert_eq!(
            repeated_read_path(
                tool_names::CODE_SEARCH,
                &serde_json::json!({"query": "fn", "path": "./docs/../README.md"})
            ),
            None
        );
    }

    #[test]
    fn code_search_family_cap_reports_path_not_serialized_identity() {
        let args = serde_json::json!({"query": "a very long query", "path": "./docs/../README.md"});
        let decision = check_read_family_cap(
            tool_names::CODE_SEARCH,
            &args,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            false,
        );

        let ReadFamilyCapDecision::Tripped { target, block_reason, .. } = decision else {
            panic!("expected code_search family cap to trip");
        };
        assert_eq!(target, "README.md");
        assert!(block_reason.contains("'README.md'"));
        assert!(!block_reason.contains("a very long query"));
    }

    #[test]
    fn read_family_cap_decision_below_cap_for_non_read_tool() {
        let decision = check_read_family_cap(
            tool_names::UNIFIED_EXEC,
            &serde_json::json!({"command": "ls -la"}),
            99,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            false,
        );
        assert_eq!(decision, ReadFamilyCapDecision::BelowCap);
    }

    #[test]
    fn read_family_cap_decision_below_cap_when_streak_under_cap() {
        let decision = check_read_family_cap(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"action": "read", "path": "src/lib.rs", "offset": 0, "limit": 100}),
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS - 1,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            false,
        );
        assert_eq!(decision, ReadFamilyCapDecision::BelowCap);
    }

    #[test]
    fn read_family_cap_decision_tripped_at_cap() {
        let decision = check_read_family_cap(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"action": "read", "path": "src/lib.rs", "offset": 0, "limit": 100}),
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            false,
        );
        match decision {
            ReadFamilyCapDecision::Tripped { target, block_reason, error_content } => {
                assert_eq!(target, "src/lib.rs");
                assert!(block_reason.contains("per-turn family cap"));
                assert!(error_content.contains("repeated_read_family"));
            }
            ReadFamilyCapDecision::BelowCap => panic!("expected Tripped at cap"),
        }
    }

    #[test]
    fn read_family_cap_planning_guidance_directs_plan_synthesis() {
        let decision = check_read_family_cap(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"action": "read", "path": "src/lib.rs"}),
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            true,
        );
        match decision {
            ReadFamilyCapDecision::Tripped { block_reason, error_content, .. } => {
                assert!(block_reason.contains("<proposed_plan>"));
                assert!(error_content.contains("<proposed_plan>"));
            }
            ReadFamilyCapDecision::BelowCap => panic!("expected Tripped at cap"),
        }
    }

    #[test]
    fn read_family_target_strips_slice_suffix() {
        assert_eq!(read_family_target("unified_file::read::src/cli/update.rs::off=81::lim=229"), "src/cli/update.rs");
        assert_eq!(read_family_target("read_file::src/main.rs::off=80::lim=200::raw=true"), "src/main.rs");
        assert_eq!(read_family_target("unified_file::read::src/cli/update.rs"), "src/cli/update.rs");
        assert_eq!(read_family_target("unified_exec::run::cat README.md"), "cat README.md");
        assert_eq!(read_family_target("unified_exec::read::Cargo.toml::off=440::lim=81"), "Cargo.toml");
        assert_eq!(read_family_target("read_file::src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn read_family_cap_decision_tripped_above_cap() {
        let decision = check_read_family_cap(
            tool_names::READ_FILE,
            &serde_json::json!({"path": "src/main.rs"}),
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS + 5,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            false,
        );
        assert!(matches!(decision, ReadFamilyCapDecision::Tripped { .. }));
    }

    #[test]
    fn read_family_cap_decision_uses_bare_path_target_when_unpaginated() {
        let decision = check_read_family_cap(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"action": "read", "path": "src/cli/update.rs"}),
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            false,
        );
        match decision {
            ReadFamilyCapDecision::Tripped { target, .. } => {
                assert_eq!(target, "src/cli/update.rs");
            }
            ReadFamilyCapDecision::BelowCap => panic!("expected Tripped at cap"),
        }
    }

    #[test]
    fn is_read_action_returns_true_for_unified_file_read() {
        assert!(is_read_action(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"action": "read", "path": "src/lib.rs"})
        ));
        assert!(is_read_action(tool_names::UNIFIED_FILE, &serde_json::json!({"path": "src/lib.rs"})));
        assert!(!is_read_action(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"action": "write", "path": "src/lib.rs"})
        ));
    }

    #[test]
    fn extract_read_path_returns_path_from_args() {
        assert_eq!(extract_read_path(&serde_json::json!({"path": "src/lib.rs"})), Some("src/lib.rs".to_string()));
        assert_eq!(extract_read_path(&serde_json::json!({})), None);
    }

    #[test]
    fn max_same_file_path_read_calls_is_stricter_than_family_cap() {
        const _: () = assert!(
            MAX_SAME_FILE_PATH_READ_CALLS >= MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            "per-file-path cap must be >= family cap"
        );
        const _: () = assert!(MAX_SAME_FILE_PATH_READ_CALLS < 10, "per-file-path cap must catch excessive reads");
    }

    #[test]
    fn planning_doubles_both_read_caps() {
        assert_eq!(effective_read_family_cap(false), MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS);
        assert_eq!(effective_read_path_cap(false), MAX_SAME_FILE_PATH_READ_CALLS);
        assert_eq!(effective_read_family_cap(true), MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS.saturating_mul(2));
        assert_eq!(effective_read_path_cap(true), MAX_SAME_FILE_PATH_READ_CALLS.saturating_mul(2));
    }

    #[test]
    fn plan_artifact_read_is_scoped_to_runtime_owned_paths() {
        let plan_file = serde_json::json!({"action": "read", "path": ".vtcode/plans/session-123.md"});
        assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &plan_file).is_some());

        let tracker = serde_json::json!({"action": "read", "path": ".vtcode/tasks/current_task.md"});
        assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &tracker).is_some());

        // Ordinary markdown docs must not receive plan-specific guidance.
        let doc = serde_json::json!({"action": "read", "path": "docs/guides/planning-workflow.md"});
        assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &doc).is_none());

        // Paths that merely contain `plan` are not plan artifacts.
        let code =
            serde_json::json!({"action": "read", "path": "src/agent/runloop/unified/planning_workflow_state.rs"});
        assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &code).is_none());
    }
}
