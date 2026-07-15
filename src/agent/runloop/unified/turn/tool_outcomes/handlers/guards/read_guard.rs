//! Guards for file read operations.
//!
//! Contains two guards:
//! 1. **Read-after-write guard**: Prevents reading a file that was just written
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
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
    find_duplicate_in_history, signature_key_for,
};
use crate::agent::runloop::unified::turn::tool_outcomes::response_content::maybe_inline_spooled;

/// Maximum consecutive reads of the same file with the same slice (offset/limit/raw).
const MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS: usize = 4;

/// Per-file-path read cap, independent of slice (offset/limit/raw). Catches
/// paginated reads of the same file that the slice-aware family key lets
/// through. Set higher than the family cap to allow legitimate pagination
/// (e.g., reading a large file in 3-4 chunks) while stopping excessive
/// re-reads (8+ reads of the same file with different offsets).
const MAX_SAME_FILE_PATH_READ_CALLS: usize = 6;

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
) -> ReadFamilyCapDecision {
    let Some(family_key) = repeated_file_read_family_key(canonical_tool_name, effective_args)
    else {
        return ReadFamilyCapDecision::BelowCap;
    };
    if streak < cap {
        return ReadFamilyCapDecision::BelowCap;
    }
    let target = read_family_target(&family_key);
    let block_reason = format!(
        "Repeated read-only exploration of '{target}' hit the per-turn family cap ({cap}). Scheduling a final recovery pass without more tools."
    );
    let error_content = build_repeated_file_read_family_error_content(&target);
    ReadFamilyCapDecision::Tripped {
        target,
        block_reason,
        error_content,
    }
}

/// Get the family key for a read-file call.
fn repeated_file_read_family_key(canonical_tool_name: &str, args: &Value) -> Option<String> {
    use super::super::looping::spool_chunk_read_path;

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

/// Build the error content for a repeated file read family guard trip.
#[cold]
fn build_repeated_file_read_family_error_content(target: &str) -> String {
    super::super::super::execution_result::build_error_content(
        format!(
            "Repeated exploration of the same file or path ('{target}') exceeded the per-turn cap. Reuse the output already gathered or try a different approach."
        ),
        None,
        None,
        "repeated_read_family",
    )
    .to_string()
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

    if !ctx.harness_state.was_recently_written(&path) {
        return None;
    }

    let content = build_read_after_write_error(&path);
    ctx.push_tool_response(tool_call_id, Some(canonical_tool_name), content);
    Some(ValidationResult::Blocked)
}

/// Enforce the repeated read-only call guard.
///
/// Uses a two-tier approach:
/// 1. Family cap: Catches identical slice retries (same path + same offset/limit)
/// 2. Per-file-path cap: Catches paginated reads of the same file
///
/// Returns `Some(ValidationResult::Blocked)` when either guard trips,
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

    if let Some(family_key) = repeated_file_read_family_key(canonical_tool_name, effective_args) {
        // The streak mutation is stateful and stays here; the cap *decision*
        // is delegated to the pure `check_read_family_cap` helper so it can be
        // tested without the full TurnProcessingContext harness.
        let streak = ctx.harness_state.record_file_read_family_call(family_key);
        if let ReadFamilyCapDecision::Tripped {
            target: _,
            block_reason,
            error_content,
        } = check_read_family_cap(
            canonical_tool_name,
            effective_args,
            streak,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        ) {
            ctx.activate_recovery(block_reason.clone());
            push_guard_failure_messages(
                ctx,
                tool_call_id,
                canonical_tool_name,
                error_content,
                &block_reason,
            );
            return Some(ValidationResult::Blocked);
        }
    }

    // Per-file-path cap: catches paginated reads of the same file that the
    // slice-aware family key lets through (e.g., 8 reads of anthropic_types.rs
    // at different offsets each get a different family key and never collide).
    if is_read_action(canonical_tool_name, effective_args) {
        if let Some(path) = extract_read_path(effective_args) {
            let path_count = ctx.harness_state.record_file_read_path_call(path.clone());
            if path_count > MAX_SAME_FILE_PATH_READ_CALLS {
                let block_reason = format!(
                    "Repeated reads of '{path}' hit the per-file-path cap ({MAX_SAME_FILE_PATH_READ_CALLS}). \
                     Read the file in full once and reuse the output."
                );
                let error_content = build_repeated_file_read_family_error_content(&path);
                ctx.activate_recovery(block_reason.clone());
                push_guard_failure_messages(
                    ctx,
                    tool_call_id,
                    canonical_tool_name,
                    error_content,
                    &block_reason,
                );
                return Some(ValidationResult::Blocked);
            }
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
                super::super::apply_reused_read_only_loop_metadata(obj);
            }
            ctx.push_tool_response(
                tool_call_id,
                Some(canonical_tool_name),
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
                ctx.push_tool_response(
                    tool_call_id,
                    Some(canonical_tool_name),
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
            super::super::apply_reused_read_only_loop_metadata(obj);
        }
        ctx.push_tool_response(
            tool_call_id,
            Some(canonical_tool_name),
            maybe_inline_spooled(canonical_tool_name, &reused_value),
        );
        ctx.harness_state
            .record_successful_readonly_signature(signature);
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
            ctx.push_tool_response(
                tool_call_id,
                Some(canonical_tool_name),
                maybe_inline_spooled(canonical_tool_name, &parsed),
            );
        } else {
            ctx.push_tool_response(tool_call_id, Some(canonical_tool_name), raw_output);
        }
        return Some(ValidationResult::Handled);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::constants::tools as tool_names;

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

    #[test]
    fn read_family_cap_decision_below_cap_for_non_read_tool() {
        let decision = check_read_family_cap(
            tool_names::UNIFIED_EXEC,
            &serde_json::json!({"command": "ls -la"}),
            99,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
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
        );
        match decision {
            ReadFamilyCapDecision::Tripped {
                target,
                block_reason,
                error_content,
            } => {
                assert_eq!(target, "src/lib.rs");
                assert!(block_reason.contains("per-turn family cap"));
                assert!(error_content.contains("repeated_read_family"));
            }
            ReadFamilyCapDecision::BelowCap => panic!("expected Tripped at cap"),
        }
    }

    #[test]
    fn read_family_target_strips_slice_suffix() {
        assert_eq!(
            read_family_target("unified_file::read::src/cli/update.rs::off=81::lim=229"),
            "src/cli/update.rs"
        );
        assert_eq!(
            read_family_target("read_file::src/main.rs::off=80::lim=200::raw=true"),
            "src/main.rs"
        );
        assert_eq!(
            read_family_target("unified_file::read::src/cli/update.rs"),
            "src/cli/update.rs"
        );
        assert_eq!(
            read_family_target("unified_exec::run::cat README.md"),
            "cat README.md"
        );
        assert_eq!(read_family_target("read_file::src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn read_family_cap_decision_tripped_above_cap() {
        let decision = check_read_family_cap(
            tool_names::READ_FILE,
            &serde_json::json!({"path": "src/main.rs"}),
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS + 5,
            MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
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
        assert!(is_read_action(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"path": "src/lib.rs"})
        ));
        assert!(!is_read_action(
            tool_names::UNIFIED_FILE,
            &serde_json::json!({"action": "write", "path": "src/lib.rs"})
        ));
    }

    #[test]
    fn extract_read_path_returns_path_from_args() {
        assert_eq!(
            extract_read_path(&serde_json::json!({"path": "src/lib.rs"})),
            Some("src/lib.rs".to_string())
        );
        assert_eq!(extract_read_path(&serde_json::json!({})), None);
    }

    #[test]
    fn max_same_file_path_read_calls_is_stricter_than_family_cap() {
        const _: () = assert!(
            MAX_SAME_FILE_PATH_READ_CALLS >= MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
            "per-file-path cap must be >= family cap"
        );
        const _: () = assert!(
            MAX_SAME_FILE_PATH_READ_CALLS < 10,
            "per-file-path cap must catch excessive reads"
        );
    }
}
