//! Planning-workflow facade.
//!
//! Single module boundary for the planning domain. The runloop must depend only
//! on this facade (the `pub(crate)` re-exports below), never on individual
//! submodule paths or on `vtcode-core`'s planning tool internals. Submodules are
//! `pub(crate)` internals so the domain stays cohesively isolated and each piece
//! remains independently testable (intent detection, HITL confirmation, tool
//! dispatch, truncation recovery).
//!
//! This is the interface guard rail for the next-generation planning refactor:
//! widening the public surface means editing the re-exports here, which makes
//! accidental cross-module coupling visible at review time.

pub(crate) mod confirmation;
pub(crate) mod events;
pub(crate) mod execution;
pub(crate) mod exit_trigger;
pub(crate) mod intent;
pub(crate) mod plan_approval;
pub(crate) mod recovery;
pub(crate) mod recovery_state;
pub(crate) mod start_confirmation;
pub(crate) mod task_tracker;
mod tracker_response;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PlanExecutionContext {
    #[default]
    Current,
    Fresh,
}

/// The destinations available to an approved plan handoff.
///
/// Auto is a confirmation-policy choice, not an authority upgrade. Both
/// destinations continue through the same runtime safety gates and tool
/// catalog refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanExecutionDestination {
    Build,
    Auto,
}

/// Immutable policy selected at the approval boundary and carried through the
/// rest of the handoff. Keeping these fields together prevents an agent name
/// from being restored independently of confirmation policy or context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlanExecutionTarget {
    pub(crate) destination: PlanExecutionDestination,
    pub(crate) skip_confirmations: bool,
    pub(crate) execution_context: PlanExecutionContext,
}

impl PlanExecutionTarget {
    pub(crate) const fn build(execution_context: PlanExecutionContext, skip_confirmations: bool) -> Self {
        Self {
            destination: PlanExecutionDestination::Build,
            skip_confirmations,
            execution_context,
        }
    }

    pub(crate) const fn auto(execution_context: PlanExecutionContext) -> Self {
        Self {
            destination: PlanExecutionDestination::Auto,
            skip_confirmations: true,
            execution_context,
        }
    }

    pub(crate) const fn agent_name(self) -> &'static str {
        match self.destination {
            PlanExecutionDestination::Build => "build",
            PlanExecutionDestination::Auto => "auto",
        }
    }
}

/// Resolve destination and confirmation policy exactly once for an approval.
/// Explicit destination selections win. Ordinary approvals use Build, even
/// when planning began from Auto; only an explicitly configured full-auto
/// policy selects Auto automatically.
pub(crate) const fn resolve_plan_execution_target(
    decision: PlanApprovalDecision,
    execution_context: PlanExecutionContext,
    skip_confirmations: bool,
    full_auto: bool,
) -> PlanExecutionTarget {
    match decision {
        PlanApprovalDecision::SwitchAuto => PlanExecutionTarget::auto(execution_context),
        PlanApprovalDecision::SwitchBuild => PlanExecutionTarget::build(execution_context, false),
        _ if full_auto => PlanExecutionTarget::auto(execution_context),
        _ => PlanExecutionTarget::build(execution_context, skip_confirmations),
    }
}

/// Map the target carried by the legacy inline interaction path back to the
/// stable approval event vocabulary. The target remains authoritative for
/// runtime behavior; this is only the telemetry representation.
pub(crate) const fn plan_approval_decision_for_target(target: PlanExecutionTarget) -> PlanApprovalDecision {
    match target.destination {
        PlanExecutionDestination::Auto => PlanApprovalDecision::SwitchAuto,
        PlanExecutionDestination::Build => match target.execution_context {
            PlanExecutionContext::Current => PlanApprovalDecision::Execute,
            PlanExecutionContext::Fresh => PlanApprovalDecision::FreshContext,
        },
    }
}

#[cfg(test)]
mod target_tests {
    use super::{PlanExecutionContext, PlanExecutionDestination, PlanExecutionTarget, resolve_plan_execution_target};
    use vtcode_core::exec::events::PlanApprovalDecision;

    #[test]
    fn approval_target_matrix_keeps_destination_and_policy_together() {
        let cases = [
            (
                PlanApprovalDecision::Execute,
                PlanExecutionContext::Current,
                false,
                false,
                PlanExecutionTarget::build(PlanExecutionContext::Current, false),
            ),
            (
                PlanApprovalDecision::FreshContext,
                PlanExecutionContext::Fresh,
                false,
                false,
                PlanExecutionTarget::build(PlanExecutionContext::Fresh, false),
            ),
            (
                PlanApprovalDecision::SwitchAuto,
                PlanExecutionContext::Current,
                false,
                false,
                PlanExecutionTarget::auto(PlanExecutionContext::Current),
            ),
            (
                PlanApprovalDecision::SwitchAuto,
                PlanExecutionContext::Fresh,
                false,
                false,
                PlanExecutionTarget::auto(PlanExecutionContext::Fresh),
            ),
            (
                PlanApprovalDecision::Execute,
                PlanExecutionContext::Current,
                true,
                false,
                PlanExecutionTarget::build(PlanExecutionContext::Current, true),
            ),
        ];

        for (decision, context, skip, full_auto, expected) in cases {
            assert_eq!(resolve_plan_execution_target(decision, context, skip, full_auto), expected);
        }
    }

    #[test]
    fn explicit_destination_wins_over_full_auto_and_source_agent() {
        let build =
            resolve_plan_execution_target(PlanApprovalDecision::SwitchBuild, PlanExecutionContext::Current, true, true);
        assert_eq!(build.destination, PlanExecutionDestination::Build);
        assert!(!build.skip_confirmations);

        let auto = resolve_plan_execution_target(
            PlanApprovalDecision::SwitchAuto,
            PlanExecutionContext::Current,
            false,
            false,
        );
        assert_eq!(auto.destination, PlanExecutionDestination::Auto);
        assert!(auto.skip_confirmations);
    }

    #[test]
    fn default_approval_does_not_inherit_auto_without_full_auto_policy() {
        let ordinary =
            resolve_plan_execution_target(PlanApprovalDecision::AutoAccept, PlanExecutionContext::Current, true, false);
        assert_eq!(ordinary.destination, PlanExecutionDestination::Build);
        assert!(ordinary.skip_confirmations);

        let configured_full_auto =
            resolve_plan_execution_target(PlanApprovalDecision::AutoAccept, PlanExecutionContext::Current, false, true);
        assert_eq!(configured_full_auto.destination, PlanExecutionDestination::Auto);
        assert!(configured_full_auto.skip_confirmations);
    }
}

// --- Stable interface (the only planning symbols the runloop should name) ---

use std::path::PathBuf;

use anyhow::Context;
use thiserror::Error;
use vtcode_core::exec::events::PlanApprovalDecision;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;

// Keep vtcode-core's planning implementation behind this facade. Runloop
// modules should depend on these re-exports instead of reaching through the
// core tool-handler path directly.
pub(crate) use vtcode_core::tools::handlers::planning_workflow::{
    CANONICAL_STEP_FORMAT, PlanValidationReport, PlanningWorkflowState, allocate_plan_file_if_missing,
    merge_plan_content, persist_plan_draft, tracker_file_for_plan_file, validate_plan_content,
};

#[cfg(test)]
use vtcode_core::tools::handlers::planning_workflow::{
    PLANNING_VERIFY_INVALID_EXAMPLES, PLANNING_VERIFY_VALID_EXAMPLES,
};

pub(crate) async fn persisted_plan_is_ready(state: &PlanningWorkflowState) -> bool {
    let Some(plan_file) = state.get_plan_file().await else {
        return false;
    };
    let Ok(plan_text) = tokio::fs::read_to_string(&plan_file).await else {
        return false;
    };
    if !validate_plan_content(&plan_text).is_ready() {
        return false;
    }

    let Some(tracker_file) = tracker_file_for_plan_file(&plan_file) else {
        return false;
    };
    let Ok(tracker_text) = tokio::fs::read_to_string(&tracker_file).await else {
        return false;
    };
    if tracker_text.trim().is_empty() {
        return false;
    }

    match state.workspace_root() {
        Some(workspace_root) => {
            tokio::fs::read_to_string(workspace_root.join(".vtcode").join("tasks").join("current_task.md"))
                .await
                .ok()
                .is_some_and(|content| !content.trim().is_empty())
        }
        None => true,
    }
}

pub(crate) use super::planning_workflow_state::{PlanningFinishReason, finish_planning_workflow};
pub(crate) use confirmation::{StartPlanningDecision, execute_plan_approval, present_start_planning_confirmation};
pub(crate) use events::{emit_context_reset, emit_plan_approval_resolved, emit_plan_ready_events};
pub(crate) use execution::handle_start_planning;
pub(crate) use exit_trigger::{PlanningExitContext, maybe_handle_planning_exit_trigger};
pub(crate) use intent::{
    PlanningIntent, assistant_recently_prompted_implementation, detect_enter_planning_intent, detect_planning_intent,
};
pub(crate) use plan_approval::{
    PlanApprovalRequestContext, PlanApprovalRoute, PlanApprovalTelemetryContext, load_plan_text_for_approval,
    plan_approval_route,
};
pub(crate) use recovery::maybe_condense_truncated_plan;
pub(crate) use task_tracker::{TaskTrackerHandoff, create_task_tracker_from_active_plan};

const EXECUTION_MODE_RECOVERY_RESET_DIRECTIVE: &str = "Execution mode is active. Any earlier planning-only recovery, plan-synthesis, or tool-disabled instruction belongs to the completed planning transition and is stale. Tools are available again; continue the user's request with the selected execution agent. Do not emit a planning-only synthesis response.";

/// Remove turn-scoped recovery instructions before leaving planning for an
/// execution agent.
///
/// Recovery directives are intentionally stored in conversation history so the
/// next model pass can follow them. They must not survive a mode transition,
/// however: a later `auto`/`build` pass would otherwise inherit "tools are
/// disabled" or "synthesize the plan" instructions even though planning has
/// already been closed. Return the number removed so callers can include a
/// useful diagnostic without rendering extra user-facing noise.
pub(crate) fn clear_stale_recovery_directives_for_execution(history: &mut Vec<uni::Message>) -> usize {
    let history_len_before = history.len();
    history.retain(|message| !is_stale_recovery_directive(message));
    let removed = history_len_before.saturating_sub(history.len());
    if removed > 0 {
        history.push(uni::Message::system(EXECUTION_MODE_RECOVERY_RESET_DIRECTIVE.to_string()));
    }
    removed
}

fn is_stale_recovery_directive(message: &uni::Message) -> bool {
    if message.role != uni::MessageRole::System {
        return false;
    }

    let text = message.content.as_text().trim().to_ascii_lowercase();
    text.starts_with("planning recovery:")
        || text.starts_with("planning tool preview budget exhausted")
        || text.starts_with("planning navigation produced")
        || text.starts_with("planning research completed")
        || text.starts_with("navigation loop detected")
        || text.starts_with("repeated low-signal navigation calls")
        || text.starts_with("turn balancer detected repeated low-signal tool churn")
        || text.starts_with("your previous `<proposed_plan>` was cut off")
        || text.starts_with("recovery:")
}

/// Build a bounded repair directive from validator-owned feedback. The
/// feedback (produced by `PlanValidationReport::repair_feedback()`) is bounded
/// — no raw plan lines — and always includes the canonical step format so the
/// model knows the exact contract. The policy prose (bounded repair, no tool
/// calls, re-emit `<proposed_plan>`) is owned by this facade.
///
/// Both the initial-plan rejection path (`response_handling.rs`) and the
/// later-turn approval rejection path (`exit_trigger.rs`) use this helper so
/// the model receives consistent format guidance from every repair surface.
pub(crate) fn build_plan_repair_directive(feedback: &str) -> String {
    format!(
        "Planning recovery: the proposed plan was rejected. Repair the retained draft using existing evidence. \
         {feedback}\n\n\
         Re-emit one compact `<proposed_plan>` with Summary, Implementation Steps, Test Cases and Validation, \
         and Assumptions and Defaults. Step format: {canonical}. \
         Every verify command must exit 0 on success; grep/rg no-match exit 1 cannot satisfy completion. \
         Do not emit tool calls or approval prose. Resolve open decisions before approval.",
        canonical = CANONICAL_STEP_FORMAT,
    )
}

/// Resolve a [`PlanArtifactError`] into the bounded repair directive the model
/// should receive on each bounded repair pass. This is the single owner
/// of the error→feedback mapping so the initial-plan rejection path
/// (`response_handling.rs`) and the later-turn approval rejection path
/// (`exit_trigger.rs`) cannot diverge: invalid plans get their report-specific
/// feedback, every other error variant gets the safe generic feedback, and the
/// bounded policy prose is always applied by [`build_plan_repair_directive`].
pub(crate) fn plan_repair_directive_for_error(error: &PlanArtifactError) -> String {
    let feedback = match error {
        PlanArtifactError::Invalid { report, .. } => report.repair_feedback(),
        _ => PlanValidationReport::default().repair_feedback(),
    };
    build_plan_repair_directive(&feedback)
}

/// Validator-owned feedback for a **terminal** plan rejection that is stored
/// in assistant history (as opposed to a system repair directive).
///
/// Terminal paths previously published only a fixed rejection sentence, so the
/// TUI showed `missing sections: …` while the model saw no reason and a later
/// `continue` resubmitted the same invalid shape (session-vtcode-20260924T133543Z).
/// Uses the same `repair_feedback()` source as [`plan_repair_directive_for_error`]
/// for `Invalid` errors so both surfaces report the same validation contract.
/// Non-`Invalid` variants fall back to their Display text here (terminal) while
/// the repair directive uses a generic default report.
pub(crate) fn plan_rejection_history_feedback(error: &PlanArtifactError) -> String {
    match error {
        PlanArtifactError::Invalid { report, .. } => report.repair_feedback(),
        other => format!("Rejection detail: {other}"),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedPlanArtifact {
    pub(crate) plan_file: PathBuf,
    pub(crate) text: String,
    pub(crate) validation: PlanValidationReport,
}

#[derive(Debug, Error)]
pub(crate) enum PlanArtifactError {
    #[error("the planning workflow has no persisted plan draft")]
    Missing,
    #[error("failed to read persisted plan {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("invalid plan artifact: {reasons}")]
    Invalid {
        reasons: String,
        // Boxed to keep `PlanArtifactError` under the `result_large_err`
        // threshold — the report carries four Vecs and is only needed on the
        // error path, not the hot path.
        report: Box<PlanValidationReport>,
    },
    #[error("failed to persist plan draft: {reason}")]
    Persistence { reason: String },
}

impl ValidatedPlanArtifact {
    pub(crate) fn from_text(plan_file: PathBuf, text: String) -> Result<Self, PlanArtifactError> {
        let validation = validate_plan_content(&text);
        if !validation.is_ready() {
            return Err(PlanArtifactError::Invalid {
                reasons: validation.reasons().join("; "),
                report: Box::new(validation),
            });
        }
        Ok(Self { plan_file, text, validation })
    }

    /// Construct a `ValidatedPlanArtifact` from an already-validated report,
    /// skipping a redundant revalidation of the same immutable text. The caller
    /// must guarantee `validation.is_ready()` (enforced by debug_assert in
    /// non-release builds and by the persistence layer in all builds).
    pub(crate) fn from_validated(plan_file: PathBuf, text: String, validation: PlanValidationReport) -> Self {
        debug_assert!(validation.is_ready(), "from_validated called with a non-ready validation report");
        Self { plan_file, text, validation }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ApprovedPlanHandoff {
    pub(crate) plan: ValidatedPlanArtifact,
    pub(crate) tracker: TaskTrackerHandoff,
    pub(crate) target: PlanExecutionTarget,
}

pub(crate) async fn complete_approved_plan_handoff(
    tool_registry: &ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    plan: ValidatedPlanArtifact,
    target: PlanExecutionTarget,
) -> anyhow::Result<ApprovedPlanHandoff> {
    let tracker = finish_planning_workflow(tool_registry, plan_session, handle, PlanningFinishReason::Approved)
        .await?
        .context("approved-plan handoff completed without a task tracker")?;
    handle.set_skip_confirmations(target.skip_confirmations);
    let handoff = ApprovedPlanHandoff { plan, tracker, target };
    tracing::info!(
        target: "vtcode.planning_workflow",
        plan_file = %handoff.plan.plan_file.display(),
        implementation_steps = handoff.plan.validation.implementation_step_count,
        tracker_plan_file = %handoff.tracker.plan_file.display(),
        tracker_file = %handoff.tracker.tracker_file.display(),
        tracker_items = handoff.tracker.item_count,
        destination = ?handoff.target.destination,
        skip_confirmations = handoff.target.skip_confirmations,
        execution_context = ?handoff.target.execution_context,
        "approved-plan handoff completed"
    );
    Ok(handoff)
}

/// Resolve the current approval request using its original telemetry identity.
/// The fallback IDs are used only for legacy callers that resolve an approval
/// before a request was recorded.
pub(crate) fn resolve_plan_approval(
    plan_session: &mut PlanningWorkflowSessionState,
    emitter: Option<&crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    fallback_thread_id: &str,
    fallback_turn_id: &str,
    decision: PlanApprovalDecision,
    automatic: bool,
) {
    let Some(pending) = plan_session.take_pending_plan_approval() else {
        return;
    };
    emit_plan_approval_resolved(
        emitter,
        if pending.thread_id.is_empty() {
            fallback_thread_id.to_owned()
        } else {
            pending.thread_id
        },
        if pending.turn_id.is_empty() {
            fallback_turn_id.to_owned()
        } else {
            pending.turn_id
        },
        decision,
        automatic,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const VALID_PLAN: &str = r#"# Plan

## Summary
A valid plan.

## Implementation Steps
1. Do it -> files: [src/lib.rs] -> verify: [cargo check]

## Test Cases and Validation
1. Run cargo check.

## Assumptions and Defaults
1. Keep existing behavior.
"#;

    const INVALID_PROSE_PLAN: &str = r#"# Plan

## Summary
Improve launch time.

## Implementation Steps
1. Profile actual startup first
2. Make startup lazy where possible

## Test Cases and Validation
1. Track the same startup marker.

## Assumptions and Defaults
1. Keep existing behavior.
"#;

    // --- plan_rejection_history_feedback tests ---

    #[test]
    fn plan_rejection_history_feedback_carries_validator_contract() {
        // Terminal rejections store this string in assistant history. It must
        // name the concrete validation failures and the canonical step format
        // so a later user `continue` can repair instead of resubmitting.
        let report = validate_plan_content(INVALID_PROSE_PLAN);
        assert!(!report.is_ready());
        let error = PlanArtifactError::Invalid {
            reasons: report.reasons().join("; "),
            report: Box::new(report),
        };
        let feedback = plan_rejection_history_feedback(&error);
        assert!(
            feedback.contains("Plan validation issues:") && feedback.contains("2 of 2 implementation step(s)"),
            "feedback must name the invalid steps: {feedback}"
        );
        assert!(
            feedback.contains(CANONICAL_STEP_FORMAT),
            "feedback must include the canonical step format: {feedback}"
        );
    }

    #[test]
    fn plan_rejection_history_feedback_uses_display_for_non_invalid_errors() {
        let error = PlanArtifactError::Persistence { reason: "disk full".to_string() };
        let feedback = plan_rejection_history_feedback(&error);
        assert!(
            feedback.contains("failed to persist plan draft: disk full"),
            "non-invalid errors must surface their Display text: {feedback}"
        );
    }

    // --- plan_repair_directive_for_error tests ---

    #[test]
    fn repair_directive_for_invalid_error_uses_report_specific_feedback() {
        // An Invalid error carries a report with the exact step count and
        // target issue. The directive must surface that report-specific
        // feedback, not the generic fallback.
        let report = validate_plan_content(INVALID_PROSE_PLAN);
        assert!(!report.is_ready());
        let error = PlanArtifactError::Invalid {
            reasons: report.reasons().join("; "),
            report: Box::new(report),
        };
        let directive = plan_repair_directive_for_error(&error);
        assert!(directive.contains("must exit 0 on success"));
        assert!(directive.contains("no-match exit 1 cannot satisfy completion"));
        assert!(
            directive.contains("2 of 2 implementation step(s)"),
            "invalid-error directive must include report-specific step count: {directive}"
        );
        assert!(
            directive.contains("Action -> files: [path/to/file.rs] -> verify: [cargo check]"),
            "directive must include the canonical step format: {directive}"
        );
        assert!(
            directive.contains("<proposed_plan>"),
            "directive must instruct re-emitting the proposed_plan block: {directive}"
        );
        assert!(
            directive.contains("Do not emit tool calls"),
            "directive must enforce the no-tool-call bounded-repair policy: {directive}"
        );
    }

    #[test]
    fn repair_directive_for_non_invalid_error_uses_safe_generic_feedback() {
        // Missing / Read / Persistence errors have no report. The directive
        // must fall back to the generic default feedback, which still includes
        // the canonical format and policy prose — never empty or unbounded.
        let error = PlanArtifactError::Missing;
        let directive = plan_repair_directive_for_error(&error);
        assert!(directive.contains("must exit 0 on success"));
        assert!(directive.contains("no-match exit 1 cannot satisfy completion"));
        assert!(
            directive.contains("Action -> files: [path/to/file.rs] -> verify: [cargo check]"),
            "generic fallback must still include the canonical step format: {directive}"
        );
        assert!(
            directive.contains("<proposed_plan>"),
            "generic fallback must still instruct re-emitting the plan: {directive}"
        );
        assert!(
            directive.contains("Do not emit tool calls"),
            "generic fallback must still enforce the no-tool-call policy: {directive}"
        );
        // The generic fallback must NOT claim a specific step count, since the
        // error variant carries no such information.
        assert!(
            !directive.contains("implementation step(s) lack"),
            "generic fallback must not fabricate step-specific diagnostics: {directive}"
        );
    }

    #[test]
    fn repair_directive_does_not_echo_raw_plan_text() {
        // The invalid plan's prose steps are model-controlled text. The
        // directive must only carry validator-owned summaries (counts and
        // canonical format), never the raw step prose.
        let report = validate_plan_content(INVALID_PROSE_PLAN);
        let error = PlanArtifactError::Invalid {
            reasons: report.reasons().join("; "),
            report: Box::new(report),
        };
        let directive = plan_repair_directive_for_error(&error);
        assert!(
            !directive.contains("Profile actual startup first"),
            "directive must NOT echo raw plan step prose: {directive}"
        );
        assert!(!directive.contains("Make startup lazy"), "directive must NOT echo raw plan step prose: {directive}");
    }

    #[test]
    fn clear_stale_recovery_directives_resets_execution_mode() {
        let mut history = vec![
            uni::Message::user("make a plan".to_string()),
            uni::Message::system(
                "Planning tool preview budget exhausted the model-visible allowance; tools are disabled on the next pass; synthesize the plan."
                    .to_string(),
            ),
            uni::Message::system("Recovery: tools are disabled, so respond with plain text only.".to_string()),
            uni::Message::system("Keep this ordinary context note.".to_string()),
            uni::Message::assistant("The planning pass ended.".to_string()),
        ];

        assert_eq!(clear_stale_recovery_directives_for_execution(&mut history), 2);
        assert_eq!(history.len(), 4);
        assert!(history.iter().any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == "Keep this ordinary context note."
        }));
        assert!(history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == EXECUTION_MODE_RECOVERY_RESET_DIRECTIVE
        }));
        assert!(!history.iter().any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains("tools are disabled")
        }));
        assert_eq!(clear_stale_recovery_directives_for_execution(&mut history), 0);
        assert_eq!(history.len(), 4);
    }

    #[test]
    fn clear_stale_recovery_directives_preserves_non_system_messages() {
        let mut history = vec![
            uni::Message::assistant(
                "A user-facing recovery explanation: tools are disabled for this pass.".to_string(),
            ),
            uni::Message::system("Recovery: summarize the failed tool call.".to_string()),
        ];

        assert_eq!(clear_stale_recovery_directives_for_execution(&mut history), 1);
        assert!(history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant && message.content.as_text().contains("tools are disabled")
        }));
    }

    // --- ValidatedPlanArtifact::from_validated tests ---

    #[test]
    fn from_validated_preserves_supplied_fields_without_revalidation() {
        // from_validated trusts the caller's report and must NOT re-parse the
        // text. We verify this by supplying a ready report alongside text that
        // would NOT validate on its own — if from_validated revalidated, the
        // resulting artifact's validation would not be ready.
        let ready_report = validate_plan_content(VALID_PLAN);
        assert!(ready_report.is_ready());

        let artifact = ValidatedPlanArtifact::from_validated(
            PathBuf::from("/tmp/plan.md"),
            "this text would not pass validation on its own".to_string(),
            ready_report.clone(),
        );

        assert_eq!(artifact.plan_file, PathBuf::from("/tmp/plan.md"));
        assert_eq!(artifact.text, "this text would not pass validation on its own");
        assert_eq!(artifact.validation, ready_report);
        assert!(
            artifact.validation.is_ready(),
            "from_validated must preserve the supplied ready report without revalidation"
        );
    }

    #[test]
    #[should_panic(expected = "from_validated called with a non-ready validation report")]
    fn from_validated_debug_asserts_on_non_ready_report() {
        // The debug_assert enforces the caller invariant in dev builds: a
        // non-ready report must never be passed. This is the guardrail that
        // keeps the skip-revalidation path safe.
        let non_ready = validate_plan_content(INVALID_PROSE_PLAN);
        assert!(!non_ready.is_ready());
        let _ = ValidatedPlanArtifact::from_validated(PathBuf::from("/tmp/plan.md"), "unused".to_string(), non_ready);
    }

    #[test]
    fn repair_directive_reuses_canonical_contract_without_repeated_example_walls() {
        let directive = build_plan_repair_directive("step 7: missing observable expectation");
        assert!(directive.contains(CANONICAL_STEP_FORMAT));
        assert!(directive.contains("step 7: missing observable expectation"));
        assert!(directive.len() < 1000);
        assert_eq!(directive.matches("step 7:").count(), 1);
    }

    #[test]
    fn quality_line_and_repair_feedback_keep_inspection_verify_examples() {
        let repair_feedback = PlanValidationReport::default().repair_feedback();
        assert!(
            vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_QUALITY_LINE.contains("sed -n")
                && vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_QUALITY_LINE.contains("grep -n"),
            "quality line missing inspection examples"
        );
        assert!(repair_feedback.contains(CANONICAL_STEP_FORMAT));
        assert!(
            PLANNING_VERIFY_VALID_EXAMPLES.contains("sed -n") && PLANNING_VERIFY_VALID_EXAMPLES.contains("grep -n")
        );
        assert!(PLANNING_VERIFY_INVALID_EXAMPLES.contains("git diff --check"));
    }
}
