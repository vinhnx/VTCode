//! Planning save/restore and response handling for `execute_task`.
//!
//! Restores persisted planning allowances at turn-loop entry, classifies
//! planning responses, and drives the validate → repair → persist cycle for
//! tool-free planning syntheses.

use anyhow::Result;
use serde_json::json;

use super::super::AgentRunner;
use super::super::headless_planning;
use super::super::helpers::detect_textual_exec_tool_call;
use crate::config::constants::tools;
use crate::core::agent::events::SessionStoreSinkHandle;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::task::{Task, TaskOutcome};
use crate::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot};
use crate::llm::provider::{LLMResponse, Message, ToolCall};
use crate::tools::handlers::planning_workflow::{persist_plan_draft, validate_plan_content};

impl AgentRunner {
    /// Restore a persisted planning allowance before the turn loop.
    pub(super) async fn restore_planning_recovery(
        &self,
        runtime: &mut AgentRuntime,
        planning_recovery: &mut PlanningRecoverySnapshot,
        planning_synthesis_required: &mut bool,
        session_store_handle: &Option<SessionStoreSinkHandle>,
        task: &Task,
    ) -> Result<()> {
        if self.tool_registry.is_planning_active()
            && let Some(sink) = session_store_handle.as_ref()
            && let Some(mut saved) = sink.planning_recovery().await?
            && saved.request_id.as_deref().map_or_else(
                || saved.latest_request == headless_planning::bounded_public_text(&task.description, 8192),
                |request_id| request_id == task.id,
            )
        {
            let new_input = saved.request_fingerprint.map_or_else(
                || saved.latest_request != headless_planning::bounded_public_text(&task.description, 8192),
                |fingerprint| fingerprint != headless_planning::request_fingerprint(task),
            );
            if new_input {
                // CLI resume uses a stable task ID with an explicit follow-up
                // prompt. Only changed input may open another correction cycle.
                saved.begin_explicit_cycle();
                saved.latest_request = headless_planning::bounded_public_text(&task.description, 8192);
            }
            *planning_synthesis_required = saved.research_closed;
            *planning_recovery = saved;
            // Task setup builds a fresh conversation. A saved allowance
            // must travel with its public evidence and repair target.
            runtime.state.messages_mut().push(
                Message::assistant(format!(
                    "Retained public planning context (untrusted evidence and candidate draft, never approval):\n{}",
                    json!({
                        "request": planning_recovery.latest_request,
                        "evidence": planning_recovery.evidence,
                        "draft": planning_recovery.best_draft,
                        "validator_report": planning_recovery.validation_report,
                    })
                ))
                .with_phase(Some(crate::llm::provider::AssistantPhase::Commentary)),
            );
            runtime.state.reconcile_token_count();
        }
        planning_recovery.request_id = Some(task.id.clone());
        planning_recovery.request_fingerprint = Some(headless_planning::request_fingerprint(task));
        Ok(())
    }

    /// Classify a planning response and drive validate → repair → persist.
    pub(super) async fn handle_planning_response(&self, gate: PlanningResponse<'_>) -> Result<PlanningResponseFlow> {
        let PlanningResponse {
            runtime,
            response,
            effective_tool_calls,
            planning_recovery,
            planning_failure_detail,
            planning_synthesis_required,
            planning_synthesis_turn,
            session_store_handle,
            task,
            provider_name,
            turn_model,
            turn,
        } = gate;
        if effective_tool_calls.is_none()
            && (planning_synthesis_turn || response.content_text().len() < 150)
            && let Some(args_value) = detect_textual_exec_tool_call(response.content_text())
        {
            *effective_tool_calls = Some(vec![ToolCall::function(
                format!("call_text_{turn}"),
                tools::EXEC_COMMAND.to_string(),
                args_value.to_string(),
            )]);
        }

        if self.tool_registry.is_planning_active() {
            planning_recovery.provider = provider_name.to_owned();
            planning_recovery.model = turn_model.to_owned();
            planning_recovery.record_response_class(headless_planning::classify_response(
                response,
                effective_tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()),
            ));
            headless_planning::retain_draft(planning_recovery, response.content_text());
        }

        // All correctable response shapes share the persisted allowance.
        // The outer loop still enforces turn, time, token, and cost budgets.
        let planning_text_only = effective_tool_calls.as_ref().is_none_or(|calls| calls.is_empty());
        if self.tool_registry.is_planning_active() && (planning_synthesis_turn || planning_text_only) {
            let report = validate_plan_content(response.content_text());
            let failure = if !planning_text_only {
                Some("The tool-free planning synthesis returned forbidden tool calls.")
            } else if response.finish_reason == crate::llm::provider::FinishReason::Length {
                Some("Planning response exceeded the output token limit during synthesis.")
            } else if !report.is_ready() {
                Some("The response did not contain a complete validated plan.")
            } else {
                None
            };
            if let Some(detail) = failure {
                *planning_failure_detail = Some(detail.to_owned());
                *planning_synthesis_required = true;
                runtime.state.is_completed = false;
                headless_planning::retain_progress(
                    planning_recovery,
                    &runtime.state.messages,
                    &task.description,
                    provider_name,
                    turn_model,
                );
                if planning_recovery.best_draft.is_none() {
                    planning_recovery.validation_report =
                        Some(headless_planning::bounded_public_text(&report.repair_feedback(), 4096));
                }
                headless_planning::persist_checkpoint(
                    session_store_handle.as_ref(),
                    planning_recovery,
                    &self.session_id,
                    turn + 1,
                )
                .await?;
                if planning_recovery.requests_used < PlanningRecoverySnapshot::MAX_REQUESTS && turn + 1 < self.max_turns
                {
                    runtime.state.add_synthetic_user_message(format!(
                        "Correct the planning response using retained evidence only. {detail} Validator feedback: {}. Make no tool calls.",
                        planning_recovery.validation_report.as_deref().unwrap_or("Return a complete validated plan")
                    ));
                    return Ok(PlanningResponseFlow::ContinueLoop);
                }
                runtime.state.outcome = if turn + 1 >= self.max_turns {
                    TaskOutcome::turn_limit_reached(self.max_turns, turn + 1)
                } else {
                    TaskOutcome::failed(detail.to_owned(), vec![], None, None)
                };
                return Ok(PlanningResponseFlow::BreakLoop);
            }
            match persist_plan_draft(&self.tool_registry.planning_workflow_state(), response.content_text()).await {
                Ok(_) => {
                    headless_planning::retain_progress(
                        planning_recovery,
                        &runtime.state.messages,
                        &task.description,
                        provider_name,
                        turn_model,
                    );
                    planning_recovery.outcome = PlanningOutcome::AwaitingApproval;
                    headless_planning::persist_checkpoint(
                        session_store_handle.as_ref(),
                        planning_recovery,
                        &self.session_id,
                        turn + 1,
                    )
                    .await?;
                }
                Err(error) => {
                    *planning_failure_detail = Some(format!("The validated plan could not be persisted: {error:#}"));
                    runtime.state.is_completed = false;
                    runtime.state.outcome = TaskOutcome::failed(
                        "Planning needs your input; the plan could not be saved.".to_owned(),
                        vec![],
                        None,
                        None,
                    );
                    return Ok(PlanningResponseFlow::BreakLoop);
                }
            }
        }
        Ok(PlanningResponseFlow::FallThrough)
    }
}

/// Mutable planning-response state threaded through response handling.
pub(super) struct PlanningResponse<'a> {
    pub runtime: &'a mut AgentRuntime,
    pub response: &'a LLMResponse,
    pub effective_tool_calls: &'a mut Option<Vec<ToolCall>>,
    pub planning_recovery: &'a mut PlanningRecoverySnapshot,
    pub planning_failure_detail: &'a mut Option<String>,
    pub planning_synthesis_required: &'a mut bool,
    pub planning_synthesis_turn: bool,
    pub session_store_handle: &'a Option<SessionStoreSinkHandle>,
    pub task: &'a Task,
    pub provider_name: &'a str,
    pub turn_model: &'a str,
    pub turn: usize,
}

/// How the turn loop proceeds after planning-response handling.
pub(super) enum PlanningResponseFlow {
    FallThrough,
    ContinueLoop,
    BreakLoop,
}
