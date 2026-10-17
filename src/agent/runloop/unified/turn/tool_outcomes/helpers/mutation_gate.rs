//! Mutation gate: docs-only/plan-artifact classification and verification blocking.

use super::*;

pub(crate) use vtcode_core::tools::tool_intent::{is_docs_only_write, is_plan_artifact_write};

fn is_execution_tool(name: &str) -> bool {
    use vtcode_core::config::constants::tools as tool_names;

    matches!(
        name,
        tool_names::UNIFIED_EXEC
            | tool_names::EXEC_COMMAND
            | tool_names::EXEC_PTY_CMD
            | tool_names::RUN_PTY_CMD
            | tool_names::EXECUTE_CODE
            | tool_names::SHELL
    )
}

/// Whether a Mutation-classified exec success should count toward
/// anti-blind-editing pressure. Admission stays fail-closed elsewhere;
/// this only prevents read-only-behaving dynamic inspections (e.g. `for`
/// loops over `grep` with `test -f`) from arming the gate when they exit 0
/// with no writes. Concrete `modified_files`, effective diffs, and shell
/// redirection still count; see
/// [`vtcode_core::tools::tool_intent::exec_mutation_should_incur_debt`].
fn exec_mutation_should_count(
    canonical_name: &str,
    executed: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
) -> bool {
    if !is_execution_tool(canonical_name) {
        return true;
    }
    let ToolExecutionStatus::Success { output, modified_files, .. } = &outcome.status else {
        return false;
    };
    if !modified_files.is_empty() {
        return true;
    }
    let command_text = vtcode_core::tools::command_args::raw_command_text(executed);
    let Some(command_text) = command_text else {
        return true;
    };
    vtcode_core::tools::tool_intent::exec_mutation_should_incur_debt(&command_text, output)
}

/// Return whether a tool call must wait for a successful verification step.
///
/// Reads, inspections, verification commands, task tracking, dedicated
/// plan-artifact writes, and docs-only prose writes remain available while the
/// checkpoint is pending.
/// A failed verifier grants a bounded fix-up window ([`FAILED_VERIFICATION_FIX_ALLOWANCE`])
/// so a broken build can be repaired, and piped verifier attempts
/// (e.g. `cargo check 2>&1 | grep error`) are admitted with pipefail when their
/// tails are statically read-only. A verifier piped only into `head`/`tail` runs
/// as a standalone verifier (see [`shell_args_as_executed`]).
pub(crate) fn mutation_blocked_until_verification(
    loop_tracker: &LoopTracker,
    name: &str,
    args: &serde_json::Value,
) -> bool {
    if !loop_tracker.verification_is_pending() || is_plan_artifact_write(name, args) || is_docs_only_write(name, args) {
        return false;
    }

    let canonical_name = canonical_tool_name(name);
    // Cleanup can only stop/release an owned exec session; it must remain
    // reachable when workspace edits await verification. Permissions still apply.
    if vtcode_core::tools::tool_intent::is_exec_session_cleanup_call(canonical_name, args) {
        return false;
    }
    if is_execution_tool(canonical_name) {
        // Classify the command the kernel will run: a verifier piped only
        // into `head`/`tail` executes standalone and is a verification.
        let executed = shell_args_as_executed(canonical_name, args);
        let activity = classify_shell_activity(canonical_name, &executed);
        // Other piped verifier attempts (`cargo check 2>&1 | grep error`)
        // run with pipefail when statically safe. The admission predicate
        // requires every shell segment to be verification-or-readonly, so a
        // smuggled mutation such as `cargo check && rm -rf target` stays
        // blocked.
        if !matches!(activity, ShellActivity::Mutation) || shell_command_is_admitted_verification_attempt(&executed) {
            return false;
        }
        // Fix-up window: allow bounded repair edits after a failed verifier.
        return loop_tracker.fix_edits_remaining == 0;
    }

    if !vtcode_core::tools::tool_intent::classify_tool_intent(canonical_name, args).mutating {
        return false;
    }
    loop_tracker.fix_edits_remaining == 0
}

/// Updates the tool repetition tracker based on the execution outcome.
///
/// Count completed attempts for repetition detection, but only successful
/// mutations contribute to anti-blind-editing verification pressure.
///
/// Returns `true` when this outcome freshly granted (or refreshed) the
/// failed-verifier fix-up window. Callers must reset the assistant text-response
/// streak so the model gets one diagnostic explanation before the
/// pending-verification text cap re-applies; otherwise a failed build blocks
/// the turn before the agent can describe the failure and use its fix-up edits.
///
/// A `true` return also covers *lost* verifier results: while the gate is
/// pending, a verifier-level Failure/Timeout (or a session follow-up
/// failure — `write_stdin`, a non-run `unified_exec` action, or a PTY
/// session follow-up — reporting a dead exec/PTY session) grants the same
/// bounded window because the verifier never produced an observable
/// verdict. In that case the tracker queues
/// [`VERIFICATION_RESULT_LOST_DIRECTIVE`] for the handlers to surface.
///
/// A `true` return also covers an admitted status-masking verifier success
/// while the gate is pending. Completed unverified checker sequences queue
/// [`PIPED_VERIFICATION_DIRECTIVE`] independently of gate state; that coaching
/// alone never changes this return value or grants fix-up edits.
#[cfg(test)]
pub(crate) fn update_repetition_tracker(
    loop_tracker: &mut LoopTracker,
    outcome: &ToolPipelineOutcome,
    name: &str,
    args: &serde_json::Value,
) -> bool {
    update_repetition_tracker_inner(loop_tracker, outcome, name, args, false)
}

pub(crate) fn update_repetition_tracker_for_registry(
    loop_tracker: &mut LoopTracker,
    outcome: &ToolPipelineOutcome,
    name: &str,
    args: &serde_json::Value,
    registry: &vtcode_core::tools::ToolRegistry,
) -> bool {
    let was_pending = loop_tracker.verification_is_pending();
    let attempts = loop_tracker.verification_auto_recovery_attempts;
    let executed = loop_tracker.auto_verification_executed;
    let failed = update_repetition_tracker_inner(
        loop_tracker,
        outcome,
        name,
        args,
        registry.is_completion_verifier_call(name, args),
    );
    if registry.completion_verification_pending() {
        // A partial or unrelated pass cannot refill the recovery episode.
        if was_pending {
            loop_tracker.mark_verification_pending();
        }
        loop_tracker.verification_auto_recovery_attempts = attempts;
        loop_tracker.auto_verification_executed = executed;
    }
    failed
}

fn update_repetition_tracker_inner(
    loop_tracker: &mut LoopTracker,
    outcome: &ToolPipelineOutcome,
    name: &str,
    args: &serde_json::Value,
    registered_verifier: bool,
) -> bool {
    if matches!(&outcome.status, ToolExecutionStatus::Cancelled) {
        return false;
    }

    let canonical_name = canonical_tool_name(name);
    let signature_key = signature_key_for(canonical_name, args);
    loop_tracker.record(signature_key.clone());
    if canonical_name == vtcode_core::config::constants::tools::TASK_TRACKER {
        if let ToolExecutionStatus::Success { output, .. } = &outcome.status {
            if matches!(
                output.get("status").and_then(serde_json::Value::as_str),
                Some("created" | "replaced" | "updated" | "added")
            ) {
                loop_tracker.reset_navigation_window(true);
                loop_tracker.tracker_observation_warning_emitted = false;
                loop_tracker.record_required_verifier_progress(None);
            } else if args.get("action").and_then(serde_json::Value::as_str) == Some("list")
                || output.get("status").and_then(serde_json::Value::as_str) == Some("unchanged")
            {
                let state = output.get("checklist").unwrap_or(output);
                let observation = signature_key_for(canonical_name, state);
                loop_tracker.record_low_signal(observation.clone());
                loop_tracker.nav_signatures.insert(observation);
                loop_tracker.consecutive_navigations = loop_tracker.consecutive_navigations.saturating_add(1);
                loop_tracker.record_navigation_signal(true);
            } else {
                loop_tracker.reset_navigation_window(false);
            }
        } else if args.get("action").and_then(serde_json::Value::as_str) == Some("list") {
            loop_tracker.record_low_signal("task_tracker:rejected_observation".to_string());
            loop_tracker.consecutive_navigations = loop_tracker.consecutive_navigations.saturating_add(1);
            loop_tracker.record_navigation_signal(true);
        }
        return false;
    }
    let session_id = vtcode_core::tools::command_args::session_id_text(args);
    let targets_pending_verifier = loop_tracker
        .pending_verifier_session_id
        .as_deref()
        .is_some_and(|pending_id| session_id == Some(pending_id));
    let targets_diagnostic_checker = is_session_follow_up(canonical_name, args)
        && session_id.is_some_and(|id| loop_tracker.pending_checker_session_ids.contains(id));
    let session_result_lost = matches!(&outcome.status, ToolExecutionStatus::Failure { error }
        if error.is_exec_session_not_found() || error_text_indicates_lost_session(&error.message));
    // Session cleanup never represents a workspace edit or a verifier verdict.
    if vtcode_core::tools::tool_intent::is_exec_session_cleanup_call(canonical_name, args) {
        if matches!(&outcome.status, ToolExecutionStatus::Success { .. }) && targets_pending_verifier {
            loop_tracker.pending_verifier_session_id = None;
        }
        if (matches!(&outcome.status, ToolExecutionStatus::Success { .. }) || session_result_lost)
            && let Some(session_id) = session_id
        {
            loop_tracker.pending_checker_session_ids.remove(session_id);
        }
        return false;
    }
    if targets_diagnostic_checker {
        let completed = matches!(&outcome.status, ToolExecutionStatus::Success { output, .. }
            if output.get("exit_code").and_then(serde_json::Value::as_i64).is_some());
        if (completed || session_result_lost)
            && let Some(session_id) = session_id
        {
            loop_tracker.pending_checker_session_ids.remove(session_id);
        }
        if completed {
            loop_tracker.piped_verification_notice_pending = true;
        }
        // A diagnostic session's loss cannot activate the legacy fallback for
        // an unknown verifier identity or grant repair edits.
        if session_result_lost && !targets_pending_verifier {
            return false;
        }
    }
    if is_session_follow_up(canonical_name, args)
        && targets_pending_verifier
        && let ToolExecutionStatus::Success { output, .. } = &outcome.status
        && let Some(exit_code) = output.get("exit_code").and_then(serde_json::Value::as_i64)
    {
        if exit_code == 0 {
            loop_tracker.mark_verification_complete();
        } else {
            loop_tracker.record_failed_verification();
            loop_tracker.verifier_failed_notice_pending = true;
        }
        loop_tracker.reset_navigation_window(true);
        return exit_code != 0;
    }
    let navigation_family =
        crate::agent::runloop::unified::turn::tool_outcomes::handlers::low_signal_family_key(canonical_name, args);
    let low_signal_family = navigation_family
        .clone()
        .filter(|_| is_low_signal_outcome(outcome, canonical_name, args));
    let navigation_signature_key = navigation_family
        .map(|family| format!("navigation::{family}"))
        .unwrap_or_else(|| signature_key.clone());
    // Successful but redundant scans (e.g. three overlapping `find` calls)
    // never match the exact family key. Track a coarse inspection family so
    // the third repeat surfaces in diagnostics and in the turn balancer's
    // listing-loop tripwire without changing admission behavior.
    let coarse_family = coarse_inspection_family_key(canonical_name, args);
    let coarse_repeat = coarse_family.as_ref().map(|family| {
        let entry = loop_tracker
            .coarse_inspection_attempts
            .entry(family.clone())
            .or_insert((0, Instant::now()));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = Instant::now();
        entry.0
    });
    let is_coarse_duplicate =
        coarse_repeat.is_some_and(|count| count >= 3) && matches!(&outcome.status, ToolExecutionStatus::Success { .. });
    let mut low_signal_family = low_signal_family;
    if let Some(evidence) = navigation_evidence(outcome, canonical_name, args) {
        if loop_tracker.record_navigation_evidence(&evidence.signatures) {
            low_signal_family = Some("navigation::retained_evidence".to_string());
        }
        if let Some(path) = evidence.focused_search_path {
            loop_tracker.record_focused_search(path);
        }
    }
    if low_signal_family.is_none() && is_coarse_duplicate {
        low_signal_family = coarse_family.clone();
    }
    let is_low_signal_navigation = low_signal_family.is_some();
    if let Some(low_signal_family) = low_signal_family.as_ref() {
        loop_tracker.record_low_signal(low_signal_family.clone());
    }

    // Lost verifier results via session follow-ups: a `write_stdin`,
    // `unified_exec` poll/wait/inspect/continue, or PTY session follow-up
    // failure that reports a missing session means the session (and its
    // pending verifier output) died before the result was captured, so the
    // follow-up never classifies as ShellActivity::Verification. While the
    // gate is pending, treat it like a failed verifier so the model gets a
    // bounded fix/diagnostic window instead of deadlocking behind a gate
    // that can no longer observe a successful verifier. When its identity is
    // known, an unrelated missing session must not discard the live verifier.
    if is_session_follow_up(canonical_name, args)
        && loop_tracker.verification_is_pending()
        && (loop_tracker.pending_verifier_session_id.is_none() || targets_pending_verifier)
        && session_result_lost
    {
        loop_tracker.verification_result_lost_notice_pending = true;
        loop_tracker.record_failed_verification();
        loop_tracker.reset_navigation_window(low_signal_family.is_none());
        return true;
    }

    // Update NL2Repo-Bench metrics based on tool intent.
    //
    // IMPORTANT: Check execution tools FIRST. `classify_tool_intent` marks
    // `command_session(action=run)` as `mutating: true` because shell commands *can*
    // mutate state, but for the Edit-Test heuristic, any execution/verification
    // step (cargo check, cargo test, etc.) should RESET the mutation counter,
    // not increment it.
    if is_execution_tool(canonical_name) {
        // Classify the command the kernel ran, not the typed text: a verifier
        // piped only into `head`/`tail` executes standalone, so its truthful
        // exit status is the verifier's and it counts as verification.
        let executed = shell_args_as_executed(canonical_name, args);
        let concrete_changes = matches!(&outcome.status, ToolExecutionStatus::Success { output, modified_files, .. }
            if !modified_files.is_empty()
                || output.get("modified_files").and_then(serde_json::Value::as_array).is_some_and(|paths| !paths.is_empty())
                || vtcode_core::tools::file_ops::diff_output_has_effective_change(output) == Some(true));
        let activity = if concrete_changes {
            ShellActivity::Mutation
        } else if registered_verifier {
            ShellActivity::Verification
        } else {
            classify_shell_activity(canonical_name, &executed)
        };
        match activity {
            ShellActivity::Inspection => {
                loop_tracker.consecutive_navigations = loop_tracker.consecutive_navigations.saturating_add(1);
                loop_tracker.nav_signatures.insert(navigation_signature_key);
                loop_tracker.record_navigation_signal(is_low_signal_navigation);
            }
            ShellActivity::Verification => {
                loop_tracker.clear_navigation_evidence();
                if let ToolExecutionStatus::Success { output, .. } = &outcome.status
                    && output.get("exit_code").and_then(serde_json::Value::as_i64).is_none()
                    && let Some(session_id) = output.get("session_id").and_then(serde_json::Value::as_str)
                {
                    loop_tracker.mark_verification_pending();
                    loop_tracker.fix_edits_remaining = 0;
                    loop_tracker.pending_verifier_session_id = Some(session_id.to_owned());
                    loop_tracker.reset_navigation_window(true);
                    return false;
                }
                if matches!(&outcome.status, ToolExecutionStatus::Success { output, .. } if output.get("exit_code").and_then(serde_json::Value::as_i64) == Some(0))
                {
                    loop_tracker.mark_verification_complete();
                } else if matches!(&outcome.status, ToolExecutionStatus::Success { command_success: false, .. }) {
                    // Only a verifier that actually ran and reported non-zero
                    // opens the fix-up window. Tool-level Failure/Timeout (never
                    // executed, e.g. argument errors) must not grant edits.
                    loop_tracker.record_failed_verification();
                    loop_tracker.verifier_failed_notice_pending = matches!(&outcome.status,
                        ToolExecutionStatus::Success { output, .. }
                            if output.get("exit_code").and_then(serde_json::Value::as_i64).is_some_and(|code| code != 0));
                    loop_tracker.reset_navigation_window(low_signal_family.is_none());
                    return true;
                } else if loop_tracker.verification_is_pending()
                    && matches!(
                        &outcome.status,
                        ToolExecutionStatus::Failure { error } | ToolExecutionStatus::Timeout { error }
                            if !matches!(error.error_type, vtcode_core::tools::registry::ToolErrorType::InvalidParameters
                                | vtcode_core::tools::registry::ToolErrorType::PermissionDenied
                                | vtcode_core::tools::registry::ToolErrorType::PolicyViolation
                                | vtcode_core::tools::registry::ToolErrorType::ToolNotFound)
                    )
                {
                    // While the gate is pending, a verifier-level
                    // Failure/Timeout almost always means the result was lost
                    // (e.g. the exec session ended before the verifier
                    // finished) rather than a genuine non-zero exit: the
                    // verifier never produced an observable verdict. A bounded
                    // fix window that still requires a successful standalone
                    // verifier to clear is strictly better than a permanent
                    // stall. Without the gate pending this stays a no-grant
                    // arg-error path.
                    loop_tracker.verification_result_lost_notice_pending = true;
                    loop_tracker.record_failed_verification();
                    loop_tracker.reset_navigation_window(low_signal_family.is_none());
                    return true;
                }
                loop_tracker.reset_navigation_window(low_signal_family.is_none());
            }
            ShellActivity::Mutation => {
                // Diagnostic only: docs-only work may never arm the gate, and
                // `$?` tails cannot pass static admission. Still explain why
                // a completed checker was not recorded as verification.
                if shell_command_contains_verifier(&executed)
                    && let ToolExecutionStatus::Success { output, .. } = &outcome.status
                {
                    if output.get("exit_code").and_then(serde_json::Value::as_i64).is_some() {
                        loop_tracker.piped_verification_notice_pending = true;
                    } else if let Some(session_id) = vtcode_core::tools::command_args::session_id_text(output) {
                        loop_tracker.pending_checker_session_ids.insert(session_id.into());
                    }
                }
                // Static verifier attempts that retain status-masking joins
                // (e.g. `cargo check | grep error; true`) may be admitted
                // to run but never clear the gate: the exit status belongs to
                // another command, not the verifier. Don't count them as
                // blind edits; a failed piped attempt still
                // opens the fix window so the agent can repair and re-run a
                // standalone verifier. Chained mutations smuggled behind a
                // verifier prefix are rejected by the admission predicate and
                // take the blind-edit path below.
                if !concrete_changes && shell_command_is_admitted_verification_attempt(&executed) {
                    let ran_and_failed =
                        matches!(&outcome.status, ToolExecutionStatus::Success { command_success: false, .. });
                    if ran_and_failed {
                        loop_tracker.record_failed_verification();
                        loop_tracker.reset_navigation_window(low_signal_family.is_none());
                        return true;
                    }
                    // A piped verifier's exit status belongs to another
                    // command, so a success cannot clear the gate. While the
                    // gate is pending that silence reads as "verified" to
                    // the model (checkpoint session-vtcode-20260912T083718Z:
                    // a piped verifier exited 0 and the turn still
                    // deadlocked). Keep the existing pending-gate recovery
                    // signal; the notice above is queued only for terminal
                    // outcomes and is independently bounded per turn.
                    if loop_tracker.verification_is_pending()
                        && matches!(&outcome.status, ToolExecutionStatus::Success { command_success: true, .. })
                    {
                        loop_tracker.reset_navigation_window(low_signal_family.is_none());
                        return true;
                    }
                    loop_tracker.reset_navigation_window(low_signal_family.is_none());
                } else {
                    if concrete_changes
                        || (mutation_was_applied(outcome)
                            && exec_mutation_should_count(canonical_name, &executed, outcome))
                    {
                        loop_tracker.record_successful_mutation();
                    }
                    loop_tracker.reset_navigation_window(low_signal_family.is_none());
                }
            }
        }
    } else if is_plan_artifact_write(canonical_name, args) || is_docs_only_write(canonical_name, args) {
        if is_docs_only_write(canonical_name, args) && mutation_was_applied(outcome) {
            loop_tracker.clear_navigation_evidence();
        }
        // Plan artifact writes in dedicated plan storage and docs-only prose
        // writes are allowed while pending and should not trigger
        // anti-blind-editing verification pressure.
        // Low-signal repetition history is preserved: plan/docs writes are not
        // navigation, so they neither advance nor clear that window.
        loop_tracker.reset_navigation_window(false);
    } else {
        let intent = vtcode_core::tools::tool_intent::classify_tool_intent(canonical_name, args);
        if intent.mutating {
            if mutation_was_applied(outcome) {
                loop_tracker.record_successful_mutation();
            }
            loop_tracker.reset_navigation_window(low_signal_family.is_none());
        } else {
            // Read-only / navigation tool
            loop_tracker.consecutive_navigations += 1;
            loop_tracker.nav_signatures.insert(navigation_signature_key);
            loop_tracker.record_navigation_signal(is_low_signal_navigation);
        }
    }
    false
}

fn mutation_was_applied(outcome: &ToolPipelineOutcome) -> bool {
    match &outcome.status {
        ToolExecutionStatus::Success { output, command_success, modified_files, .. } => {
            if let Some(effective_change) = vtcode_core::tools::file_ops::diff_output_has_effective_change(output) {
                return effective_change;
            }
            *command_success || !modified_files.is_empty()
        }
        ToolExecutionStatus::Failure { .. } | ToolExecutionStatus::Timeout { .. } | ToolExecutionStatus::Cancelled => {
            false
        }
    }
}
pub(crate) fn serialize_output(output: &serde_json::Value) -> String {
    if let Some(s) = output.as_str() {
        s.to_string()
    } else {
        serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())
    }
}

pub(crate) fn check_is_argument_error(error_str: &str) -> bool {
    error_str.contains("Missing required")
        || error_str.contains("Invalid arguments")
        || error_str.contains("Tool argument validation failed")
        || error_str.contains("required path parameter")
        || error_str.contains("is required for '")
        || error_str.contains("is required for \"")
        || error_str.contains("'index' is required")
        || error_str.contains("'index_path' is required")
        || error_str.contains("'status' is required")
        || error_str.contains("expected ")
        || error_str.contains("Expected:")
}
