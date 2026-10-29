//! Completion-assessment gate for the `execute_task` turn loop.
//!
//! Detects response loops, resolves completion candidates through tracker
//! and verification state, and force-continues text-only turns while the
//! task tracker still has incomplete steps.

use anyhow::Result;
use serde_json::json;
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

use super::super::AgentRunner;
use super::super::completion::AssessmentResolution;
use super::super::continuation::{
    CompletionAssessment, ContinuationController, build_verification_failure_payload,
    tracker_incomplete_continue_prompt, tracker_status_force_continue_eligible,
};
use crate::config::constants::tools;
use crate::core::agent::completion::{check_completion_candidate, check_for_response_loop};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::task::{Task, TaskOutcome};
use crate::exec::events::HarnessEventKind;
use crate::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot};
use crate::llm::provider::{LLMResponse, ToolCall};
use crate::utils::colors::style;

/// Mutable completion-gate state threaded through turn assessment.
pub(super) struct CompletionGate<'a> {
    pub runtime: &'a mut AgentRuntime,
    pub event_recorder: &'a mut ExecEventRecorder,
    pub response: &'a LLMResponse,
    pub effective_tool_calls: &'a Option<Vec<ToolCall>>,
    pub planning_recovery: &'a PlanningRecoverySnapshot,
    pub provider_name: &'a str,
    pub turn_model: &'a str,
    pub turn: usize,
    pub effective_task: &'a Task,
    pub continuation_controller: &'a mut ContinuationController,
    pub verification_recovery_used: &'a mut bool,
    pub orchestration_enabled: bool,
    pub revision_rounds_used: &'a mut usize,
    pub max_revision_rounds: usize,
    pub should_write_blocked_handoff: &'a mut bool,
    pub forced_continuation: &'a mut bool,
}

/// How the turn loop proceeds after the completion gate.
pub(super) enum CompletionGateFlow {
    FallThrough,
    Break,
    ReturnErr(anyhow::Error),
}

impl AgentRunner {
    /// Run the completion-assessment gate for a text-only turn.
    pub(super) async fn assess_completion_gate(&mut self, gate: CompletionGate<'_>) -> Result<CompletionGateFlow> {
        let CompletionGate {
            runtime,
            event_recorder,
            response,
            effective_tool_calls,
            planning_recovery,
            provider_name,
            turn_model,
            turn,
            effective_task,
            continuation_controller,
            verification_recovery_used,
            orchestration_enabled,
            revision_rounds_used,
            max_revision_rounds,
            should_write_blocked_handoff,
            forced_continuation,
        } = gate;
        if !*forced_continuation
            && !runtime.state.is_completed
            && effective_tool_calls.as_ref().is_none_or(|tool_calls| tool_calls.is_empty())
            && !response.content_text().trim().is_empty()
        {
            // A validated persisted plan may repeat a previously truncated draft.
            // Its successful repair is progress, even when the public text matches.
            if planning_recovery.outcome != PlanningOutcome::AwaitingApproval
                && check_for_response_loop(response.content_text(), &mut runtime.state)
            {
                self.record_diagnostic(
                    event_recorder,
                    &Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        code: "agent.response_loop".into(),
                        summary: "Repeated assistant responses stopped this task.".into(),
                        agent_details: json!({"provider": provider_name, "model": turn_model, "turn": turn + 1,
                        "outcome": "loop_detected"})
                        .to_string(),
                        correlation_id: format!("agent:{}:response-loop", self.session_id),
                        actions: vec![],
                    },
                );
                runtime.state.outcome = TaskOutcome::LoopDetected;
                return Ok(CompletionGateFlow::Break);
            }

            if (self.tool_registry.is_planning_active()
                && matches!(response.finish_reason, crate::llm::provider::FinishReason::Stop))
                || check_completion_candidate(response.content_text())
                || self.tool_registry.completion_verification_pending()
            {
                self.runner_println(format_args!(
                    "[{}] {}",
                    self.agent_type,
                    style("Completion candidate detected; checking tracker and verification state.")
                        .green()
                        .bold()
                ));
                let mut assessment = continuation_controller
                    .assess_completion(effective_task, &runtime.state, self.tool_registry.tracker_adopted_for_request())
                    .await?;

                if self.tool_registry.completion_verification_pending()
                    && !matches!(assessment, CompletionAssessment::Continue { .. })
                {
                    let mut commands = match &assessment {
                        CompletionAssessment::Verify { commands } => commands.clone(),
                        _ => Vec::new(),
                    };
                    if commands.is_empty() {
                        commands = self.tool_registry.missing_required_completion_verifiers();
                    }
                    if commands.is_empty() {
                        let override_command =
                            self.config().agent.harness.verification.default_verifier_override.as_deref();
                        commands = override_command
                            .filter(|command| {
                                crate::tools::tool_intent::classify_shell_activity(
                                    tools::EXEC_COMMAND,
                                    &json!({"cmd":command}),
                                ) == crate::tools::tool_intent::ShellActivity::Verification
                            })
                            .map(str::to_owned)
                            .or_else(|| crate::tools::tool_intent::default_verifier_for_workspace(&self._workspace))
                            .into_iter()
                            .collect();
                    }
                    if commands.is_empty() {
                        runtime.state.outcome =
                            TaskOutcome::failed(self.tool_registry.unverified_completion_reason(), vec![], None, None);
                        *should_write_blocked_handoff = true;
                        return Ok(CompletionGateFlow::Break);
                    }
                    assessment = CompletionAssessment::Verify { commands };
                }

                // Verify requires running verification commands before
                // re-dispatching the after_verification result through the
                // same helper. All other variants are resolved directly.
                if let CompletionAssessment::Verify { commands } = &assessment {
                    event_recorder.harness_event(
                        HarnessEventKind::VerificationStarted,
                        Some(format!("Running verification: {}", commands.join(", "))),
                        commands.first().cloned(),
                        None,
                        None,
                        None,
                        None,
                    );
                    let verification_results =
                        self.run_verification_commands(commands, event_recorder, runtime).await?;
                    self.emit_verification_outcome(event_recorder, commands, &verification_results);
                    if matches!(runtime.state.outcome, TaskOutcome::Cancelled) {
                        return Ok(CompletionGateFlow::Break);
                    }

                    let post_verification = continuation_controller.after_verification(&verification_results).await?;
                    if let CompletionAssessment::Continue { reason, .. } = &post_verification {
                        let policy = &self.config().agent.harness.verification;
                        if *verification_recovery_used
                            || !policy.auto_execute
                            || policy.in_turn_attempts == 0
                            || policy.cross_turn_turns == 0
                            || policy.max_consecutive_failures <= 1
                        {
                            let evidence = verification_results
                                .iter()
                                .find(|result| !result.success || result.exit_code != Some(0))
                                .map(build_verification_failure_payload)
                                .unwrap_or_else(|| reason.clone());
                            let evidence = vtcode_commons::formatting::truncate_byte_budget(
                                &evidence,
                                2000,
                                "\n[verification evidence truncated]",
                            );
                            runtime.state.outcome = TaskOutcome::failed(
                                format!("Unverified: completion verification did not pass. {evidence}"),
                                vec![],
                                None,
                                None,
                            );
                            *should_write_blocked_handoff = true;
                            return Ok(CompletionGateFlow::Break);
                        }
                        *verification_recovery_used = true;
                    }
                    match self
                        .resolve_completion_assessment(
                            post_verification,
                            &verification_results,
                            effective_task,
                            runtime,
                            event_recorder,
                            orchestration_enabled,
                            revision_rounds_used,
                            max_revision_rounds,
                            should_write_blocked_handoff,
                        )
                        .await?
                    {
                        AssessmentResolution::Break => return Ok(CompletionGateFlow::Break),
                        AssessmentResolution::ForceContinue => {
                            *forced_continuation = true;
                        }
                        // `after_verification` never yields `Verify`.
                        AssessmentResolution::VerifyNotHandled => {}
                    }
                } else {
                    match self
                        .resolve_completion_assessment(
                            assessment,
                            &[],
                            effective_task,
                            runtime,
                            event_recorder,
                            orchestration_enabled,
                            revision_rounds_used,
                            max_revision_rounds,
                            should_write_blocked_handoff,
                        )
                        .await?
                    {
                        AssessmentResolution::Break => return Ok(CompletionGateFlow::Break),
                        AssessmentResolution::ForceContinue => {
                            *forced_continuation = true;
                        }
                        AssessmentResolution::VerifyNotHandled => {
                            // Verify is handled in the if-branch above;
                            // the helper only returns this for Verify.
                            return Ok(CompletionGateFlow::ReturnErr(anyhow::anyhow!(
                                "unexpected VerifyNotHandled from assess_completion"
                            )));
                        }
                    }
                }
            } else {
                // Tracker-aware status continuation: text-only responses
                // that are not completion candidates still must not end
                // the run while the task tracker has incomplete steps.
                //
                // Use a read-only tracker probe — never `assess_completion`
                // — so status text cannot create/complete the internal
                // scaffold or invent a pending verify step under
                // ContinuationPolicy::All.
                let tracker_auto_continue = self.config().agent.harness.continuation.auto_continue_tracker;
                let idle_limit = self.config().agent.idle_turn_limit;
                let idle_limit_hit = runtime.state.consecutive_idle_turns >= idle_limit;
                let asks_user = response.content_text().contains('?');
                if tracker_auto_continue && !idle_limit_hit && !asks_user {
                    let status_text = response.content_text();
                    let safety_handoff =
                        crate::core::agent::completion::tracker_final_text_is_safety_handoff(status_text);
                    if !safety_handoff {
                        let incomplete = continuation_controller
                            .incomplete_tracker_labels(self.tool_registry.tracker_adopted_for_request())
                            .await?;
                        if !incomplete.is_empty() {
                            let joined = incomplete.join(", ");
                            let reason = format!("Task tracker is incomplete: {joined}.");
                            if tracker_status_force_continue_eligible(
                                tracker_auto_continue,
                                idle_limit_hit,
                                asks_user,
                                &reason,
                            ) {
                                let prompt = tracker_incomplete_continue_prompt(&joined);
                                self.runner_println(format_args!(
                                    "[{}] {}: {}",
                                    self.agent_type,
                                    style("[TRACKER CONTINUE]").yellow().bold(),
                                    reason
                                ));
                                runtime.state.add_synthetic_user_message(prompt);
                                *forced_continuation = true;
                            }
                        }
                    }
                }
            }
        }
        Ok(CompletionGateFlow::FallThrough)
    }
}
