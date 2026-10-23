//! Completion acceptance and verification outcome projection for headless tasks.
//!
//! The execution loop owns verification admission and budgets. Both pre- and
//! post-verification assessments use this one acceptance path.

use anyhow::Result;

use super::AgentRunner;
use super::continuation::{CompletionAssessment, VerificationResult};
use super::orchestration::EvaluatorGateOutcome;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::{Task, TaskOutcome};
use crate::exec::events::HarnessEventKind;

/// Outcome of [`AgentRunner::resolve_completion_assessment`].
///
/// Collapses the duplicated `CompletionAssessment` handling (pre- and
/// post-verification) into a single signal the turn loop reacts to.
pub(super) enum AssessmentResolution {
    /// Assessment resolved to acceptance or skip — the turn loop should break.
    Break,
    /// Assessment requested more work — the turn loop should force a continuation.
    ForceContinue,
    /// Assessment is `Verify`; the caller runs verification and re-dispatches the
    /// `after_verification` result through the same helper.
    VerifyNotHandled,
}

impl AgentRunner {
    async fn resolve_completion_acceptance(
        &mut self,
        effective_task: &Task,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        orchestration_enabled: bool,
        verification_results: &[VerificationResult],
        revision_rounds_used: &mut usize,
        max_revision_rounds: usize,
        should_write_blocked_handoff: &mut bool,
    ) -> Result<bool> {
        if !orchestration_enabled {
            session_state.is_completed = true;
            session_state.outcome = TaskOutcome::Success;
            return Ok(true);
        }

        match self
            .apply_evaluator_gate(
                effective_task,
                session_state,
                event_recorder,
                verification_results,
                revision_rounds_used,
                max_revision_rounds,
            )
            .await?
        {
            EvaluatorGateOutcome::Accept => {
                session_state.is_completed = true;
                session_state.outcome = TaskOutcome::Success;
                Ok(true)
            }
            EvaluatorGateOutcome::Continue { prompt } => {
                session_state.add_synthetic_user_message(prompt);
                Ok(false)
            }
            EvaluatorGateOutcome::Exhausted { reason } => {
                session_state.outcome = TaskOutcome::failed(reason, vec![], None, None);
                *should_write_blocked_handoff = true;
                Ok(true)
            }
        }
    }

    /// Resolve a [`CompletionAssessment`] produced by either
    /// [`super::continuation::ContinuationController::assess_completion`]
    /// (pre-verification) or
    /// [`super::continuation::ContinuationController::after_verification`]
    /// (post-verification) into a
    /// single [`AssessmentResolution`] the turn loop reacts to.
    ///
    /// This consolidates the previously duplicated match arms for `Accept`,
    /// `SkipAccept`, and `Continue`. `Verify` is returned as
    /// [`AssessmentResolution::VerifyNotHandled`] so the caller can run
    /// verification commands and re-dispatch the `after_verification` result
    /// through this same helper.
    ///
    /// Note: `SkipAccept` intentionally bypasses the evaluator gate (mirroring
    /// the original pre-verification path) regardless of which controller call
    /// produced it.
    #[allow(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    pub(super) async fn resolve_completion_assessment(
        &mut self,
        assessment: CompletionAssessment,
        verification_results: &[VerificationResult],
        effective_task: &Task,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        orchestration_enabled: bool,
        revision_rounds_used: &mut usize,
        max_revision_rounds: usize,
        should_write_blocked_handoff: &mut bool,
    ) -> Result<AssessmentResolution> {
        match assessment {
            CompletionAssessment::Accept => {
                if self
                    .resolve_completion_acceptance(
                        effective_task,
                        &mut runtime.state,
                        event_recorder,
                        orchestration_enabled,
                        verification_results,
                        revision_rounds_used,
                        max_revision_rounds,
                        should_write_blocked_handoff,
                    )
                    .await?
                {
                    return Ok(AssessmentResolution::Break);
                }
                Ok(AssessmentResolution::ForceContinue)
            }
            CompletionAssessment::SkipAccept { reason } => {
                event_recorder.harness_event(
                    HarnessEventKind::ContinuationSkipped,
                    Some(reason),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
                runtime.state.is_completed = true;
                runtime.state.outcome = TaskOutcome::Success;
                Ok(AssessmentResolution::Break)
            }
            CompletionAssessment::Continue { reason, prompt } => {
                self.emit_continuation_started(event_recorder, reason);
                runtime.state.add_synthetic_user_message(prompt);
                Ok(AssessmentResolution::ForceContinue)
            }
            // Handled by the caller, which has access to the continuation
            // controller and runs verification before re-dispatching.
            CompletionAssessment::Verify { .. } => Ok(AssessmentResolution::VerifyNotHandled),
        }
    }

    /// Emit the `ContinuationStarted` harness event for a forced-continuation
    /// assessment. Centralizes the (reason-only) event shape used by both the
    /// pre- and post-verification continuation paths.
    fn emit_continuation_started(&self, event_recorder: &mut ExecEventRecorder, reason: String) {
        event_recorder.harness_event(HarnessEventKind::ContinuationStarted, Some(reason), None, None, None, None, None);
    }

    /// Emit `VerificationFailed` (for the first failing command) or
    /// `VerificationPassed` based on the verification run. Reuses
    /// [`super::continuation::build_verification_failure_payload`] so the failure
    /// headline stays in sync with the continuation prompt builder.
    pub(super) fn emit_verification_outcome(
        &self,
        event_recorder: &mut ExecEventRecorder,
        commands: &[String],
        results: &[VerificationResult],
    ) {
        if let Some(failure) = results.iter().find(|result| !result.success) {
            event_recorder.harness_event(
                HarnessEventKind::VerificationFailed,
                Some(super::continuation::build_verification_failure_payload(failure)),
                Some(failure.command.clone()),
                None,
                failure.exit_code,
                None,
                None,
            );
        } else if !commands.is_empty()
            && results.len() == commands.len()
            && results.iter().all(|result| result.exit_code == Some(0))
        {
            event_recorder.harness_event(
                HarnessEventKind::VerificationPassed,
                Some(format!("Verification passed: {}", commands.join(", "))),
                commands.last().cloned(),
                None,
                Some(0),
                None,
                None,
            );
        }
    }
}
