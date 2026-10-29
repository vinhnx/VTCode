//! Pre-dispatch gates for the `execute_task` turn loop.
//!
//! Runs the confidence-escalation safety gate over pending tool calls and
//! settles coordinator-matrix lifecycle states before tool dispatch.

use std::ops::ControlFlow;

use anyhow::{Context, Result};
use serde_json::json;
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

use super::super::AgentRunner;
use super::super::escalation::{EscalationDecision, EscalationGate};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::task::TaskOutcome;
use crate::exec::events::HarnessEventKind;
use crate::llm::provider::ToolCall;

impl AgentRunner {
    /// Run the confidence-escalation gate over pending tool calls.
    ///
    /// Blocked calls are either re-planned (clearing the pending calls and
    /// forcing continuation), handed to the user for guidance, or aborted.
    #[allow(
        clippy::too_many_arguments,
        reason = "Turn-loop gate threading existing loop state; mirrors `resolve_completion_assessment`."
    )]
    pub(super) fn run_escalation_gate(
        &self,
        runtime: &mut AgentRuntime,
        effective_tool_calls: &mut Option<Vec<ToolCall>>,
        event_recorder: &mut ExecEventRecorder,
        orchestration_enabled: bool,
        provider_name: &str,
        turn_model: &str,
        turn: usize,
        should_write_blocked_handoff: &mut bool,
        forced_continuation: &mut bool,
    ) -> Result<ControlFlow<()>> {
        if self.config().agent.harness.confidence_escalation.enabled
            && !runtime.state.is_completed
            && let Some(tool_calls) = effective_tool_calls.as_ref()
            && !tool_calls.is_empty()
        {
            let escalation_config = &self.config().agent.harness.confidence_escalation;
            let orchestration_mode = if orchestration_enabled {
                "plan_build_evaluate"
            } else {
                "single"
            };

            // Reuse the per-turn cost estimate from the budget check above.
            let cost_estimate = runtime.state.total_cost_usd;

            let result = EscalationGate::decide(
                tool_calls,
                escalation_config,
                &runtime.state.error_recovery.lock(),
                cost_estimate,
                orchestration_mode,
            );

            if result.any_escalated {
                let reasons: Vec<String> = result
                    .decisions
                    .iter()
                    .filter_map(|d| match d {
                        EscalationDecision::Escalate { reason, .. } => Some(reason.clone()),
                        EscalationDecision::Proceed => None,
                    })
                    .collect();
                let summary = reasons.join("; ");

                self.record_diagnostic(
                    event_recorder,
                    &Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        code: "policy.escalation".into(),
                        summary: "A safety check blocked tool calls. Recovery or guidance is required.".into(),
                        agent_details: json!({"provider": provider_name, "model": turn_model, "turn": turn + 1,
                        "reasons": reasons, "attempt": runtime.state.consecutive_escalations + 1,
                        "max_replan_attempts": escalation_config.max_replan_attempts,
                        "max_total_escalations": escalation_config.max_total_escalations})
                        .to_string(),
                        correlation_id: format!("agent:{}:confidence-escalation", self.session_id),
                        actions: vec![],
                    },
                );

                event_recorder.harness_event(
                    HarnessEventKind::EscalationTriggered,
                    Some(summary.clone()),
                    None,
                    None,
                    None,
                    None,
                    None,
                );

                let esc_count = runtime.state.consecutive_escalations;

                // --- Step 3: Re-plan via conversation injection ---
                if esc_count < escalation_config.max_replan_attempts {
                    runtime.state.consecutive_escalations += 1;
                    let replan_msg = format!(
                        "The following tool calls were blocked by the safety \
                         escalation gate:\n\n{summary}\n\n\
                         Please try a different approach. You can use alternative \
                         tools, break the task into smaller steps, or provide a \
                         direct answer based on what you have already learned."
                    );
                    runtime.state.add_synthetic_user_message(replan_msg);
                    // Skip dispatching blocked tool calls — let the LLM re-plan
                    *effective_tool_calls = None;
                    *forced_continuation = true;
                }
                // --- Step 4: Prompt user for guidance ---
                else if esc_count < escalation_config.max_total_escalations
                    && escalation_config.prompt_user_on_exhaust
                {
                    runtime.state.consecutive_escalations += 1;
                    let user_msg = format!(
                        "I've tried multiple approaches but the safety escalation \
                         gate keeps blocking my tool calls:\n\n{summary}\n\n\
                         Could you provide guidance on how to proceed? \
                         What approach should I use instead?"
                    );
                    runtime.state.add_synthetic_user_message(user_msg);
                    *should_write_blocked_handoff = true;
                    runtime.state.outcome = TaskOutcome::escalated(summary, "multi_tool".to_string());
                    return Ok(ControlFlow::Break(()));
                }
                // --- Step 5: Abort with partial results ---
                else {
                    runtime.state.consecutive_escalations += 1;
                    *should_write_blocked_handoff = true;
                    runtime.state.outcome = TaskOutcome::failed(
                        format!("Escalation chain exhausted: {summary}"),
                        Vec::new(),
                        Some(
                            "The safety escalation gate repeatedly blocked tool calls. \
                             Consider disabling the gate or adjusting the confidence \
                             threshold if this action should be permitted."
                                .into(),
                        ),
                        None,
                    );
                    return Ok(ControlFlow::Break(()));
                }
            } else {
                // Tool calls passed escalation gate — reset chain counter
                runtime.state.consecutive_escalations = 0;

                event_recorder.harness_event(
                    HarnessEventKind::EscalationBypassed,
                    Some("All tool calls passed escalation gate".into()),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            }
        }
        Ok(ControlFlow::Continue(()))
    }

    /// Settle coordinator-matrix lifecycle states before dispatch.
    pub(super) async fn settle_coordinator_matrix(
        &self,
        runtime: &mut AgentRuntime,
        effective_tool_calls: &Option<Vec<ToolCall>>,
        forced_continuation: &mut bool,
        should_write_blocked_handoff: &mut bool,
    ) -> Result<ControlFlow<()>> {
        if self
            .active_primary_agent
            .as_ref()
            .is_some_and(|agent| agent.identity.name == "coordinator")
            && effective_tool_calls.as_ref().is_none_or(|calls| calls.is_empty())
            && let Some(controller) = self.tool_registry.subagent_controller()
            && let Some(snapshot) = controller.matrix_snapshot().await
        {
            use crate::exec::events::matrix::MatrixLifecycle;
            match snapshot.lifecycle {
                MatrixLifecycle::Running | MatrixLifecycle::Verifying => {
                    let settled = controller
                        .wait_matrix_idle()
                        .await
                        .context("matrix disappeared before completion")?;
                    anyhow::ensure!(
                        !matches!(settled.lifecycle, MatrixLifecycle::Running | MatrixLifecycle::Verifying),
                        "matrix stopped without a durable result; reconcile owned cleanup before continuation"
                    );
                    runtime.state.is_completed = false;
                    runtime.state.add_synthetic_user_message(format!(
                        "Durable matrix results: {}. Report outcomes from this snapshot; matrix success requires Succeeded. Other states require a coordinator decision.",
                        serde_json::to_string(&settled)?
                    ));
                    *forced_continuation = true;
                }
                MatrixLifecycle::Succeeded => {
                    runtime.state.is_completed = true;
                    runtime.state.outcome = TaskOutcome::Success;
                    return Ok(ControlFlow::Break(()));
                }
                MatrixLifecycle::Cancelled => {
                    runtime.state.outcome = TaskOutcome::Cancelled;
                    return Ok(ControlFlow::Break(()));
                }
                MatrixLifecycle::Blocked => {
                    runtime.state.outcome = TaskOutcome::Failed {
                        reason: "matrix requires a coordinator decision or confirmed owned cleanup".into(),
                        accomplished: vec![],
                        recovery_suggestion: Some(format!(
                            "Resume session and inspect matrix {} status",
                            snapshot.spec.id
                        )),
                        checkpoint_path: None,
                    };
                    *should_write_blocked_handoff = true;
                    return Ok(ControlFlow::Break(()));
                }
                MatrixLifecycle::Created | MatrixLifecycle::Paused => {}
            }
        }
        Ok(ControlFlow::Continue(()))
    }
}
