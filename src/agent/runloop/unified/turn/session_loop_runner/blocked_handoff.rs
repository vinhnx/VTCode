use std::path::Path;

use vtcode_core::core::agent::blocked_handoff::{BlockedHandoffResume, write_blocked_handoff_with_resume};
use vtcode_core::core::agent::harness_artifacts::existing_harness_artifact_paths;
use vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics;
use vtcode_core::exec::events::HarnessEventKind;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::{
    SessionArchive, SessionProgressArgs, SessionProgressPersistenceStatus, VerifiedSessionArchiveIdentifier,
};

use vtcode_core::tools::tool_intent::{VERIFIER_SHELL_FORM_NOTE, verifier_reference};

use crate::agent::runloop::git::workspace_relative_display;
use crate::agent::runloop::unified::inline_events::harness::{HarnessEventEmitter, harness_event};
use crate::agent::runloop::unified::state::VerificationFailureSummary;

const NO_ARCHIVE_RESUME_EXPLANATION: &str = "Resume is unavailable because no session archive exists.";
const UNVERIFIED_RESUME_EXPLANATION: &str = "Resume is unavailable because the session archive could not be verified.";

/// Upper bound (in chars) for the block reason shown in transcript lines.
/// Provider errors can flood the transcript, so the renderer shows a bounded
/// prefix plus a pointer to the handoff file; the handoff markdown keeps the
/// full reason unchanged.
const TRANSCRIPT_BLOCK_REASON_LIMIT: usize = 600;

/// Upper bound (in chars) for the appended last-turn diagnostics footer in
/// the handoff markdown. The canonical `events.jsonl` remains the
/// full-fidelity source; the footer only carries the counts needed to
/// triage without opening the log.
const BLOCKED_DIAGNOSTICS_FOOTER_LIMIT: usize = 1200;

/// Plan-mode blocked-turn header: plan mode is read-only by design, so a
/// `Turn blocked` / `Mutation blocked` there is a policy stop, not a
/// transient failure. Retrying the same mutating tools re-blocks.
/// Folded into the single TUI action line (2-line diagnostic contract).
const PLAN_MODE_MUTATION_BLOCK_ACTION: &str =
    "Plan mode is read-only; `/mode build` to implement, or type 'continue' to keep planning.";
const PLAN_MODE_TURN_BLOCKED_ACTION: &str = "Type 'continue' to keep planning, or `/mode build` to implement.";

/// Returns true when a blocker summary describes a mutation/policy stop
/// rather than a generic turn-loop stop. Used to pick plan-mode copy:
/// mutation stops need the read-only explanation, generic stops need the
/// re-block warning. Only the two canonical markers are matched: the
/// verification-gate `"mutation blocked"` prefix and the planning-gate
/// `"tool denied by planning workflow"` context. Broader substrings like
/// `"mutating"` or `"read-only"` are intentionally excluded — they appear in
/// unrelated verifier output (e.g. `read-only file system`) and in fuse-trip
/// reasons, where the generic header is the correct choice.
pub(super) fn is_plan_mode_mutation_block(blocker_summary: &str) -> bool {
    let lowered = blocker_summary.to_ascii_lowercase();
    lowered.contains("mutation blocked") || lowered.contains("tool denied by planning workflow")
}

/// Single plan-mode action line for a blocked turn while planning is active.
/// Kept as a static string so transcript rendering stays bounded and
/// unit-testable. Build is the default destination (confirmation-aware);
/// Auto only changes confirmation policy, not authority or safety gates.
pub(super) fn plan_mode_switch_guidance_line(is_mutation_block: bool) -> &'static str {
    if is_mutation_block {
        PLAN_MODE_MUTATION_BLOCK_ACTION
    } else {
        PLAN_MODE_TURN_BLOCKED_ACTION
    }
}

/// Shell-form guidance for verifier commands the user is asked to run in a
/// blocked handoff. It states the same rule as [`VERIFIER_SHELL_FORM_NOTE`]
/// in a parenthetical that fits inside a handoff sentence.
const HANDOFF_VERIFIER_SHELL_FORM: &str = "standalone or as a pure `&&` chain (piping only into `head` or `tail` \
    also counts; cap output with `max_output_tokens`)";

/// Queued input for an autonomous cross-turn verification-recovery turn.
/// `verifier` is the resolved harness verifier; when none was resolved the
/// input names the generic build/test/lint description instead of presuming
/// a command that may not exist in this workspace. The leading sentence is
/// matched by `is_follow_up_prompt_like`.
/// Stable opening shared with `is_internal_harness_follow_up`, which keys the
/// quiet path off this constant instead of a duplicated literal.
pub(crate) const VERIFICATION_AUTO_RECOVERY_PREFIX: &str = "Continue autonomously from the last stalled turn.";

pub(super) fn verification_auto_recovery_follow_up(verifier: Option<&str>) -> String {
    let verifier = verifier_reference(verifier);
    format!(
        "{VERIFICATION_AUTO_RECOVERY_PREFIX} Verification is still pending; the request resumes once \
        {verifier} runs with `exec_command`, standalone or as a pure `&&` chain, and exits 0. {VERIFIER_SHELL_FORM_NOTE} \
        A text-only reply leaves the gate pending, so this turn would end blocked again."
    )
}

/// Transcript line announcing an autonomous verification-recovery turn.
pub(super) fn verification_auto_recovery_status_line(verifier: Option<&str>, attempt: u8, max: u8) -> String {
    match verifier.map(str::trim).filter(|command| !command.is_empty()) {
        Some(command) => format!(
            "[i] Verification gate auto-recovery turn {attempt}/{max}: retrying `{command}` without manual `continue`."
        ),
        None => format!(
            "[i] Verification gate auto-recovery turn {attempt}/{max}: asking for a project verifier run without manual `continue`."
        ),
    }
}

/// Blocked-handoff reason for a verification block whose autonomous recovery
/// ended. With an escalated failure the reason carries the failing command
/// and its output tail; otherwise it reports the spent (or disabled)
/// cross-turn budget and whether harness auto-verification ran at all, which
/// it only does when a verifier was resolved.
pub(super) fn verification_exhausted_handoff_reason(
    base: &str,
    verifier: Option<&str>,
    attempt: u8,
    max: u8,
    last_failure: Option<&VerificationFailureSummary>,
) -> String {
    if let Some(failure) = last_failure {
        let exit = failure
            .exit_code
            .map_or_else(|| "unobserved".to_string(), |code| code.to_string());
        return format!(
            "{base} The harness auto-verification `{}` failed {} time(s) consecutively, so autonomous recovery stopped. \
            Terminal exit: {exit}. Last output tail:\n{}\nFix the reported failure, then run `{}` {HANDOFF_VERIFIER_SHELL_FORM} and let it exit 0 \
            before typing `continue` to resume with the gate preserved.",
            failure.command, failure.consecutive_failures, failure.excerpt_tail, failure.command,
        );
    }
    let command = verifier.map(str::trim).filter(|command| !command.is_empty());
    let harness_note = match command {
        Some(command) => format!("harness auto-verification already tried `{command}`"),
        None => "no project verifier was detected, so harness auto-verification did not run".to_string(),
    };
    // `max == 0` disables cross-turn recovery via config: report it as
    // disabled rather than the confusing `0/0 turns`.
    let recovery_note = if max == 0 {
        format!("with cross-turn auto-recovery disabled ({harness_note})")
    } else {
        format!("after {attempt}/{max} auto-recovery turns ({harness_note})")
    };
    let verifier = verifier_reference(command);
    format!(
        "{base} Autonomous verification recovery was exhausted {recovery_note}. Run {verifier} \
        {HANDOFF_VERIFIER_SHELL_FORM} and let it exit 0, then type `continue` to resume with the gate preserved."
    )
}

/// Build the `# Last-Turn Diagnostics` footer from the turn snapshot and the
/// session tool set. Returns an empty string when there is nothing
/// meaningful to report, so callers can append unconditionally.
pub(super) fn blocked_diagnostics_footer(
    diagnostics: Option<&SnapshotTurnDiagnostics>,
    distinct_tools: &[String],
) -> String {
    let Some(diagnostics) = diagnostics else {
        return String::new();
    };
    let mut lines = Vec::with_capacity(6);
    lines.push(format!("Elapsed: {}ms", diagnostics.elapsed_ms));
    if !distinct_tools.is_empty() {
        let mut tools = distinct_tools.to_vec();
        tools.sort();
        tools.dedup();
        let mut joined = tools.join(", ");
        if joined.chars().count() > 300 {
            joined = format!("{}… ({} tools)", joined.chars().take(300).collect::<String>(), tools.len());
        }
        lines.push(format!("Tools used this session ({}): {joined}", tools.len()));
    }
    lines.push(format!(
        "Tool calls: requested={} admitted={} failed={} denied={} preflight_failures={} reused={}",
        diagnostics.requested_tool_calls,
        diagnostics.admitted_tool_calls,
        diagnostics.failed_tool_calls,
        diagnostics.denied_tool_calls,
        diagnostics.preflight_failures,
        diagnostics.reused_results,
    ));
    if diagnostics.model_visible_tool_preview_budget_exhausted || diagnostics.suppressed_tool_previews > 0 {
        lines.push(format!(
            "Preview budget exhausted: {} (suppressed previews: {})",
            diagnostics.model_visible_tool_preview_budget_exhausted, diagnostics.suppressed_tool_previews,
        ));
    }
    let usage = &diagnostics.usage;
    if usage.input_tokens > 0 || usage.output_tokens > 0 {
        lines.push(format!(
            "Turn usage: prompt={} cached={} completion={}",
            usage.input_tokens, usage.cached_input_tokens, usage.output_tokens,
        ));
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut footer = format!("\n\n# Last-Turn Diagnostics\n\n{}", lines.join("\n"));
    if footer.chars().count() > BLOCKED_DIAGNOSTICS_FOOTER_LIMIT {
        footer = footer.chars().take(BLOCKED_DIAGNOSTICS_FOOTER_LIMIT).collect();
    }
    footer
}

/// Append the diagnostics footer to a blocker summary, keeping the result
/// bounded. The transcript renderer truncates separately.
pub(super) fn blocker_summary_with_diagnostics(
    reason: &str,
    diagnostics: Option<&SnapshotTurnDiagnostics>,
    distinct_tools: &[String],
) -> String {
    let footer = blocked_diagnostics_footer(diagnostics, distinct_tools);
    if footer.is_empty() {
        return reason.to_string();
    }
    let mut summary = String::with_capacity(reason.len() + footer.len());
    summary.push_str(reason);
    summary.push_str(&footer);
    summary
}

/// True when the blocker summary is the anti-blind verification gate.
/// Shared by the status label and the action-line verb so the literal stays
/// in one place.
pub(super) fn is_verification_pending_block(blocker_summary: &str) -> bool {
    blocker_summary.to_ascii_lowercase().contains("verification is still pending")
}

const BLOCKED_STATUS_HEADLINE_LIMIT: usize = 80;

/// Short TUI stop marker for a blocked turn. Long recovery-fallback and
/// planning reasons are already published as assistant text in the same turn
/// (R4: do not repeat); the full reason stays in the handoff markdown.
pub(super) fn blocked_status_label(blocker_summary: &str) -> String {
    let lowered = blocker_summary.to_ascii_lowercase();
    if is_verification_pending_block(blocker_summary) {
        return "Turn blocked: verification pending".to_string();
    }
    if lowered.contains("planning turn ended via recovery fallback") {
        return "Turn blocked: planning recovery fallback".to_string();
    }
    if lowered.contains("recovery fallback") {
        return "Turn blocked: recovery fallback".to_string();
    }
    let headline = truncated_block_reason(blocker_summary, "");
    let headline = headline.strip_suffix("… — full reason: ").unwrap_or(headline.as_str());
    if headline.is_empty() {
        return "Turn blocked".to_string();
    }
    if headline.chars().count() <= BLOCKED_STATUS_HEADLINE_LIMIT {
        return format!("Turn blocked: {headline}");
    }
    let mut short: String = headline.chars().take(BLOCKED_STATUS_HEADLINE_LIMIT.saturating_sub(1)).collect();
    short.push('…');
    format!("Turn blocked: {short}")
}

/// Build the single TUI action line (2-line diagnostic contract). Combines the
/// verb phrase, optional resume id, and durable handoff pointers.
///
/// Both the live pointer and the timestamped archive are named: the live
/// `current_blocked.md` is deleted on recovery, so a transcript that only
/// names it becomes an orphan. The archive is the durable copy.
pub(super) fn blocked_action_line(
    workspace: &Path,
    handoff_path: &Path,
    archive_path: &Path,
    resume: Option<&str>,
    planning_active: bool,
    is_mutation_block: bool,
    is_verification_block: bool,
) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(4);
    if is_verification_block {
        parts.push(
            "Run the verifier standalone or as a pure `&&` chain, let it exit 0, then type 'continue'".to_string(),
        );
        if planning_active {
            parts.push(
                plan_mode_switch_guidance_line(is_mutation_block)
                    .trim_end_matches('.')
                    .to_string(),
            );
        }
    } else if planning_active {
        parts.push(
            plan_mode_switch_guidance_line(is_mutation_block)
                .trim_end_matches('.')
                .to_string(),
        );
    } else {
        parts.push("Type 'continue' to resume, or describe alternative instructions".to_string());
    }
    if let Some(id) = resume.map(str::trim).filter(|id| !id.is_empty()) {
        parts.push(format!("`vtcode --resume {id}`"));
    }
    let relative = workspace_relative_display(workspace, handoff_path);
    let archive = workspace_relative_display(workspace, archive_path);
    parts.push(format!("details: {relative}"));
    parts.push(format!("archive: {archive}"));
    format!("  • {}", parts.join(" · "))
}

/// Bound the block reason for transcript rendering. When the summary exceeds
/// [`TRANSCRIPT_BLOCK_REASON_LIMIT`] chars it is truncated and suffixed with
/// an ellipsis plus the handoff path that holds the full text.
///
/// User-facing transcripts stay concise: the `# Last-Turn Diagnostics` footer
/// (elapsed/tools/token counts for agent forensics) is stripped here so it
/// only lives in the handoff markdown + `events.jsonl`. Only the first
/// non-empty line (headline) is shown; multi-line reasons keep their full
/// text in the handoff file.
fn truncated_block_reason(summary: &str, full_reason_path: &str) -> String {
    let without_footer = summary.split("\n\n# Last-Turn Diagnostics").next().unwrap_or(summary);
    let headline = without_footer
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let summary = if headline.is_empty() {
        without_footer.trim()
    } else {
        headline
    };
    let suffix = format!("… — full reason: {full_reason_path}");
    let suffix_len = suffix.chars().count();
    let summary_len = summary.chars().count();
    if summary_len + suffix_len <= TRANSCRIPT_BLOCK_REASON_LIMIT {
        return summary.to_string();
    }
    let keep = TRANSCRIPT_BLOCK_REASON_LIMIT.saturating_sub(suffix_len);
    let mut bounded: String = summary.chars().take(keep).collect();
    bounded.push_str(&suffix);
    bounded
}

#[derive(Debug)]
enum ResumeAvailability {
    Available(VerifiedSessionArchiveIdentifier),
    Unavailable(String),
}

impl ResumeAvailability {
    fn as_handoff_resume(&self) -> BlockedHandoffResume<'_> {
        match self {
            Self::Available(identifier) => BlockedHandoffResume::Available(identifier),
            Self::Unavailable(explanation) => BlockedHandoffResume::Unavailable(explanation),
        }
    }
}

pub(super) struct SessionCheckpointOutcome {
    history_checkpoint_succeeded: bool,
    history_persistence_disabled: bool,
    blocked_resume: Option<ResumeAvailability>,
}

impl SessionCheckpointOutcome {
    fn new(blocked_turn: bool) -> Self {
        Self {
            history_checkpoint_succeeded: false,
            history_persistence_disabled: false,
            blocked_resume: blocked_turn.then(|| ResumeAvailability::Unavailable(UNVERIFIED_RESUME_EXPLANATION.into())),
        }
    }

    pub(super) fn without_archive(blocked_turn: bool) -> Self {
        let mut outcome = Self::new(blocked_turn);
        if blocked_turn {
            outcome.blocked_resume = Some(ResumeAvailability::Unavailable(NO_ARCHIVE_RESUME_EXPLANATION.into()));
        }
        outcome
    }

    pub(super) fn history_checkpoint_succeeded(&self) -> bool {
        self.history_checkpoint_succeeded
    }

    pub(super) fn history_persistence_disabled(&self) -> bool {
        self.history_persistence_disabled
    }

    pub(super) fn blocked_handoff_resume(&self) -> BlockedHandoffResume<'_> {
        self.blocked_resume.as_ref().map_or(
            BlockedHandoffResume::Unavailable(UNVERIFIED_RESUME_EXPLANATION),
            ResumeAvailability::as_handoff_resume,
        )
    }
}

pub(super) async fn persist_session_checkpoint(
    archive: &SessionArchive,
    args: SessionProgressArgs,
    blocked_turn: bool,
) -> SessionCheckpointOutcome {
    let mut outcome = SessionCheckpointOutcome::new(blocked_turn);
    let checkpoint_status = if blocked_turn {
        archive.persist_progress_async_with_status_forced(args).await
    } else {
        archive.persist_progress_async_with_status(args).await
    };

    match checkpoint_status {
        Ok(SessionProgressPersistenceStatus::Persisted(path)) => {
            outcome.history_checkpoint_succeeded = true;
            if blocked_turn {
                outcome.blocked_resume = Some(match archive.verify_persisted_resume_identifier(&path).await {
                    Ok(Some(identifier)) => ResumeAvailability::Available(identifier),
                    Ok(None) => ResumeAvailability::Unavailable(UNVERIFIED_RESUME_EXPLANATION.into()),
                    Err(err) => {
                        tracing::warn!(error = %err, "Failed to verify persisted session archive for blocked handoff");
                        ResumeAvailability::Unavailable(format!(
                            "Resume is unavailable because the persisted session archive could not be resolved: {err}"
                        ))
                    }
                });
            }
        }
        Ok(SessionProgressPersistenceStatus::Throttled(path)) => {
            tracing::debug!(
                path = %path.display(),
                "Session progress checkpoint throttled; retaining in-flight steering intents"
            );
            if blocked_turn {
                outcome.blocked_resume = Some(ResumeAvailability::Unavailable(
                    "Resume is unavailable because the blocked-turn checkpoint was throttled.".to_owned(),
                ));
            }
        }
        Ok(SessionProgressPersistenceStatus::Disabled(path)) => {
            outcome.history_persistence_disabled = true;
            tracing::debug!(
                path = %path.display(),
                "Session progress checkpoint skipped because history persistence is disabled"
            );
            if blocked_turn {
                outcome.blocked_resume = Some(ResumeAvailability::Unavailable(
                    "Resume is unavailable because history persistence is disabled.".to_owned(),
                ));
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "Failed to persist session progress");
            if blocked_turn {
                outcome.blocked_resume = Some(ResumeAvailability::Unavailable(format!(
                    "Resume is unavailable because the blocked-turn checkpoint failed: {err}"
                )));
            }
        }
    }

    outcome
}

pub(super) fn write_blocked_handoff_after_checkpoint(
    workspace: &Path,
    session_id: &str,
    blocker_summary: &str,
    resume: BlockedHandoffResume<'_>,
    renderer: &mut AnsiRenderer,
    harness_emitter: Option<&HarnessEventEmitter>,
    handle: Option<&vtcode_ui::tui::app::InlineHandle>,
    planning_active: bool,
) {
    match write_blocked_handoff_with_resume(
        workspace,
        session_id,
        "blocked",
        blocker_summary,
        &existing_harness_artifact_paths(workspace),
        resume,
        planning_active,
    ) {
        Ok(artifacts) => {
            // Strict 2-line TUI diagnostic contract (spec tui-diagnostics-cleanup):
            // line 1 = short stop marker (no repeat of an assistant-published
            // reason), line 2 = one combined action including durable archive
            // pointer (live current_blocked.md is cleared on recovery).
            let status = blocked_status_label(blocker_summary);
            let _ = renderer.line(MessageStyle::Warning, &status);
            let is_verification_block = is_verification_pending_block(blocker_summary);
            let resume_id = match resume {
                BlockedHandoffResume::Available(id) => Some(id.as_str()),
                BlockedHandoffResume::Unavailable(_) => None,
            };
            let action = blocked_action_line(
                workspace,
                &artifacts.current_path,
                &artifacts.archive_path,
                resume_id,
                planning_active,
                is_plan_mode_mutation_block(blocker_summary),
                is_verification_block,
            );
            let _ = renderer.line(MessageStyle::Info, &action);

            if let Some(handle) = handle {
                handle.set_activity_state(vtcode_commons::ui_protocol::ActivityState::Blocked);
            }

            if let Some(emitter) = harness_emitter {
                let _ = emitter.emit(harness_event(
                    HarnessEventKind::TurnBlocked,
                    Some(blocker_summary.to_string()),
                    None,
                    None,
                    None,
                ));
                for path in [&artifacts.current_path, &artifacts.archive_path] {
                    let path_text = path.display().to_string();
                    let _ = emitter.emit(harness_event(
                        HarnessEventKind::BlockedHandoffWritten,
                        Some("Blocked handoff written".to_string()),
                        Some(path_text),
                        None,
                        None,
                    ));
                }
            }
        }
        Err(err) => tracing::warn!(error = %err, "Failed to persist blocked handoff"),
    }
}

/// Persist blocked-handoff artifacts without renderer nudge lines or
/// blocked UI state. Used when tracker/plan auto-continue already printed
/// the single exhausted-path info line and the generic "Type continue"
/// stack must not be duplicated.
pub(super) fn persist_blocked_handoff_quiet(
    workspace: &Path,
    session_id: &str,
    blocker_summary: &str,
    resume: BlockedHandoffResume<'_>,
    planning_active: bool,
) {
    match write_blocked_handoff_with_resume(
        workspace,
        session_id,
        "blocked",
        blocker_summary,
        &existing_harness_artifact_paths(workspace),
        resume,
        planning_active,
    ) {
        Ok(_) => {}
        Err(err) => tracing::warn!(error = %err, "Failed to persist quiet blocked handoff"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NO_ARCHIVE_RESUME_EXPLANATION, TRANSCRIPT_BLOCK_REASON_LIMIT, blocked_action_line, blocked_status_label,
        blocker_summary_with_diagnostics, is_plan_mode_mutation_block, is_verification_pending_block,
        plan_mode_switch_guidance_line, truncated_block_reason, verification_auto_recovery_follow_up,
        verification_auto_recovery_status_line, verification_exhausted_handoff_reason,
    };
    use crate::agent::runloop::unified::state::{VerificationFailureSummary, is_follow_up_prompt_like};
    use std::path::Path;
    use vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics;
    use vtcode_core::tools::tool_intent::{GENERIC_VERIFIER_DESCRIPTION, VERIFIER_SHELL_FORM_NOTE};

    const BASE: &str = "Turn blocked after repeated unverified assistant responses; verification is still pending.";

    #[test]
    fn auto_recovery_follow_up_names_resolved_verifier_in_calm_prose() {
        let follow_up = verification_auto_recovery_follow_up(Some("go test ./..."));

        assert!(is_follow_up_prompt_like(&follow_up));
        assert!(follow_up.contains("`go test ./...` runs with `exec_command`"));
        assert!(follow_up.contains(VERIFIER_SHELL_FORM_NOTE));
        assert!(!follow_up.contains("no pipes"), "piping into head/tail alone counts: {follow_up}");
        assert!(!follow_up.contains("Do not"), "states the consequence instead: {follow_up}");
    }

    #[test]
    fn auto_recovery_follow_up_without_verifier_names_generic_description() {
        let follow_up = verification_auto_recovery_follow_up(None);

        assert!(follow_up.contains(&format!("once {GENERIC_VERIFIER_DESCRIPTION} runs")));
        assert!(!follow_up.contains("once `cargo check --locked`"), "no single command is presumed: {follow_up}");
    }

    #[test]
    fn auto_recovery_status_line_does_not_claim_a_retry_without_verifier() {
        assert_eq!(
            verification_auto_recovery_status_line(Some("npm test"), 1, 2),
            "[i] Verification gate auto-recovery turn 1/2: retrying `npm test` without manual `continue`."
        );
        let generic = verification_auto_recovery_status_line(None, 2, 2);
        assert!(generic.contains("2/2"));
        assert!(!generic.contains("retrying `"), "{generic}");
    }

    #[test]
    fn exhausted_handoff_with_verifier_reports_harness_attempt() {
        let reason = verification_exhausted_handoff_reason(BASE, Some("pytest -q"), 2, 2, None);

        assert!(reason.starts_with(BASE));
        assert!(reason.contains("recovery was exhausted after 2/2 auto-recovery turns"));
        assert!(reason.contains("harness auto-verification already tried `pytest -q`"));
        assert!(reason.contains("Run `pytest -q` standalone or as a pure `&&` chain"));
        assert!(!reason.contains("no pipes"));
    }

    #[test]
    fn exhausted_handoff_without_verifier_does_not_claim_harness_attempt() {
        let reason = verification_exhausted_handoff_reason(BASE, None, 2, 2, None);

        assert!(reason.contains("no project verifier was detected, so harness auto-verification did not run"));
        assert!(!reason.contains("already tried"));
        assert!(reason.contains(&format!("Run {GENERIC_VERIFIER_DESCRIPTION} standalone")));
        assert!(!reason.contains("`cargo check --locked` standalone"));

        let disabled = verification_exhausted_handoff_reason(BASE, None, 0, 0, None);
        assert!(disabled.contains("with cross-turn auto-recovery disabled"));
        assert!(!disabled.contains("0/0"));
    }

    #[test]
    fn exhausted_handoff_after_escalation_carries_failure_tail() {
        let failure = VerificationFailureSummary {
            exit_code: Some(1),
            command: "cargo nextest run".to_string(),
            excerpt_tail: "error[E0308]: mismatched types".to_string(),
            consecutive_failures: 3,
        };
        let reason = verification_exhausted_handoff_reason(BASE, Some("cargo nextest run"), 1, 2, Some(&failure));

        assert!(reason.contains("`cargo nextest run` failed 3 time(s) consecutively"));
        assert!(reason.contains("Last output tail:\nerror[E0308]: mismatched types\n"));
        assert!(reason.contains("then run `cargo nextest run` standalone or as a pure `&&` chain"));
    }

    #[test]
    fn quiet_handoff_resume_explanation_is_non_empty() {
        assert!(!NO_ARCHIVE_RESUME_EXPLANATION.trim().is_empty());
        assert!(NO_ARCHIVE_RESUME_EXPLANATION.contains("Resume is unavailable"));
    }

    #[test]
    fn short_block_reason_is_rendered_verbatim() {
        assert_eq!(
            truncated_block_reason("provider 429 rate limited", ".vtcode/tasks/current_blocked.md"),
            "provider 429 rate limited"
        );
    }

    #[test]
    fn long_block_reason_is_truncated_with_full_reason_pointer() {
        let path = ".vtcode/tasks/current_blocked.md";
        let summary = "x".repeat(2000);
        let truncated = truncated_block_reason(&summary, path);
        assert!(
            truncated.chars().count() <= TRANSCRIPT_BLOCK_REASON_LIMIT,
            "transcript reason must stay bounded: {} chars",
            truncated.chars().count()
        );
        assert!(truncated.ends_with(&format!("… — full reason: {path}")));
        assert!(truncated.starts_with("xxx"), "truncation must keep the reason prefix");
        assert!(truncated.contains("xxx…"), "the ellipsis must mark the elided middle");
    }

    #[test]
    fn truncation_respects_multibyte_char_boundaries() {
        let summary = "é".repeat(1500);
        let truncated = truncated_block_reason(&summary, "h.md");
        assert!(truncated.chars().count() <= TRANSCRIPT_BLOCK_REASON_LIMIT);
        assert!(truncated.ends_with("full reason: h.md"));
        assert!(truncated.contains('é'), "multi-byte chars must survive intact");
    }

    #[test]
    fn transcript_reason_strips_diagnostics_footer_keeps_headline() {
        let summary = "Turn ended with a recovery fallback; the requested work was not confirmed.\n\n# Last-Turn Diagnostics\n\nElapsed: 12916ms\nTools used this session (8): apply_patch, code_search\nTurn usage: prompt=85173 cached=0 completion=542";
        let truncated = truncated_block_reason(summary, ".vtcode/tasks/current_blocked.md");
        assert_eq!(truncated, "Turn ended with a recovery fallback; the requested work was not confirmed.");
        assert!(!truncated.contains("Elapsed:"), "agent forensics stay file-only: {truncated}");
        assert!(!truncated.contains("Tools used"), "agent forensics stay file-only: {truncated}");
        assert!(!truncated.contains("Turn usage"), "agent forensics stay file-only: {truncated}");
        assert!(!truncated.contains("# Last-Turn Diagnostics"), "footer marker leaks: {truncated}");
    }

    #[test]
    fn transcript_reason_uses_headline_for_multiline_reason() {
        let single = truncated_block_reason("provider 429 rate limited", "h.md");
        assert_eq!(single, "provider 429 rate limited");

        let multi = truncated_block_reason(
            "Turn blocked: verification is still pending.\nSecond line with verifier detail.\nThird line.",
            "h.md",
        );
        assert_eq!(multi, "Turn blocked: verification is still pending.");
        assert!(!multi.contains("Second line"), "only headline shows in TUI: {multi}");
    }

    #[test]
    fn blocker_summary_without_diagnostics_is_verbatim() {
        let summary = blocker_summary_with_diagnostics("stalled", None, &[]);
        assert_eq!(summary, "stalled");
    }

    #[test]
    fn blocker_summary_appends_bounded_diagnostics_footer() {
        let diagnostics = SnapshotTurnDiagnostics {
            elapsed_ms: 12_345,
            requested_tool_calls: 32,
            admitted_tool_calls: 28,
            failed_tool_calls: 3,
            denied_tool_calls: 1,
            preflight_failures: 2,
            model_visible_tool_preview_budget_exhausted: true,
            suppressed_tool_previews: 5,
            ..Default::default()
        };
        let summary = blocker_summary_with_diagnostics(
            "repeated unverified responses",
            Some(&diagnostics),
            &["exec_command".to_string(), "code_search".to_string()],
        );
        assert!(summary.starts_with("repeated unverified responses\n\n# Last-Turn Diagnostics"));
        assert!(summary.contains("requested=32 admitted=28 failed=3 denied=1 preflight_failures=2"));
        assert!(summary.contains("Preview budget exhausted: true"));
        assert!(summary.contains("exec_command"));
    }

    #[test]
    fn mutation_block_detection_is_case_insensitive_and_asymmetric() {
        assert!(is_plan_mode_mutation_block(
            "Mutation blocked until verification: 2 mutating command(s) await a verifier."
        ));
        assert!(is_plan_mode_mutation_block("Tool 'apply_patch' execution failed: tool denied by planning workflow"));
        assert!(!is_plan_mode_mutation_block(
            "Turn blocked after repeated assistant responses reached the safety cap; the latest response was preserved."
        ));
        assert!(!is_plan_mode_mutation_block("provider 429 rate limited"));
        // Regression guard: verifier output mentioning a read-only filesystem
        // must not select the mutation header — the generic plan-mode header
        // is correct there.
        assert!(!is_plan_mode_mutation_block(
            "Turn blocked waiting for verification; auto-recovery turn scheduled. Last output tail:\nerror: read-only file system"
        ));
    }

    #[test]
    fn plan_mode_guidance_is_one_action_line_splitting_mutation_from_generic() {
        let mutation = plan_mode_switch_guidance_line(true);
        let generic = plan_mode_switch_guidance_line(false);
        assert!(mutation.contains("read-only"));
        assert!(mutation.contains("/mode build"));
        assert!(generic.contains("continue"));
        assert!(generic.contains("/mode build"));
        assert_ne!(mutation, generic, "mutation vs generic actions must differ");
        assert!(!mutation.contains('\n'), "stays one line: {mutation}");
        assert!(!generic.contains('\n'), "stays one line: {generic}");
    }

    #[test]
    fn blocked_status_label_shortens_recovery_fallback_reasons() {
        let fallback = "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained.";
        assert_eq!(blocked_status_label(fallback), "Turn blocked: recovery fallback");
        assert!(
            !blocked_status_label(fallback).contains("was not confirmed"),
            "R4: do not repeat the assistant-published reason"
        );

        let planning = "Planning turn ended via recovery fallback without confirming an approval-ready plan; planning remains active.";
        assert_eq!(blocked_status_label(planning), "Turn blocked: planning recovery fallback");

        assert_eq!(blocked_status_label(BASE), "Turn blocked: verification pending");
        assert_eq!(blocked_status_label("provider 429 rate limited"), "Turn blocked: provider 429 rate limited");
    }

    #[test]
    fn blocked_status_label_truncates_long_headlines_instead_of_dropping() {
        let long = format!("provider rejected the request: {}", "x".repeat(200));
        let label = blocked_status_label(&long);
        assert!(label.starts_with("Turn blocked: provider rejected"));
        assert!(label.ends_with('…'), "long reasons keep a truncated signal: {label}");
        assert!(label.chars().count() <= "Turn blocked: ".len() + 80);
    }

    #[test]
    fn blocked_action_line_is_one_line_with_relative_details_resume_and_archive() {
        let workspace = Path::new("/work/proj");
        let handoff = Path::new("/work/proj/.vtcode/tasks/current_blocked.md");
        let archive = Path::new("/work/proj/.vtcode/tasks/blockers/session-vtcode-20260924t041604z-deadbeef.md");
        let action = blocked_action_line(
            workspace,
            handoff,
            archive,
            Some("session-vtcode-20260924T040254Z_398045-69092"),
            false,
            false,
            false,
        );

        assert!(action.starts_with("  • "));
        assert!(!action.contains('\n'), "2-line contract: action stays single-line: {action}");
        assert!(action.contains("`vtcode --resume session-vtcode-20260924T040254Z_398045-69092`"));
        assert!(action.contains("details: .vtcode/tasks/current_blocked.md"));
        assert!(
            action.contains("archive: .vtcode/tasks/blockers/session-vtcode-20260924t041604z-deadbeef.md"),
            "durable archive pointer must survive live-pointer clear: {action}"
        );
        assert!(!action.contains("/work/proj/.vtcode"), "no absolute workspace path: {action}");
    }

    #[test]
    fn blocked_action_line_verification_leads_with_verifier_step() {
        let workspace = Path::new("/work/proj");
        let handoff = Path::new("/work/proj/.vtcode/tasks/current_blocked.md");
        let archive = Path::new("/work/proj/.vtcode/tasks/blockers/a.md");
        let action = blocked_action_line(workspace, handoff, archive, None, false, false, true);
        assert!(action.contains("Run the verifier standalone"));
        assert!(action.contains("details: .vtcode/tasks/current_blocked.md"));
        assert!(action.contains("archive: .vtcode/tasks/blockers/a.md"));
        assert!(!action.contains('\n'));
    }

    #[test]
    fn blocked_action_line_plan_mode_folds_switch_guidance() {
        let workspace = Path::new("/work/proj");
        let handoff = Path::new("/work/proj/.vtcode/tasks/current_blocked.md");
        let archive = Path::new("/work/proj/.vtcode/tasks/blockers/a.md");
        let action = blocked_action_line(workspace, handoff, archive, None, true, true, false);
        assert!(action.contains("/mode build"));
        assert!(action.contains("read-only"));
        assert!(!action.contains('\n'), "R7: plan-mode guidance folds into the one action line");
    }

    #[test]
    fn blocked_action_line_keeps_plan_verb_when_verification_also_pending() {
        let workspace = Path::new("/work/proj");
        let handoff = Path::new("/work/proj/.vtcode/tasks/current_blocked.md");
        let archive = Path::new("/work/proj/.vtcode/tasks/blockers/a.md");
        let action = blocked_action_line(workspace, handoff, archive, None, true, true, true);
        assert!(action.contains("Run the verifier standalone"));
        assert!(action.contains("/mode build"), "plan guidance must not be dropped: {action}");
        assert!(!action.contains('\n'));
    }

    #[test]
    fn verification_matcher_is_shared() {
        assert!(is_verification_pending_block(BASE));
        assert!(is_verification_pending_block("Turn blocked: verification is still pending."));
        assert!(!is_verification_pending_block("provider 429 rate limited"));
    }
}
