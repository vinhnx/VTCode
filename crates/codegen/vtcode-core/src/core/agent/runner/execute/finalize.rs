//! Post-loop finalization for `execute_task`.
//!
//! Adjusts the terminal outcome (headless-planning input handoff,
//! completion-verification debt), builds the task summary, persists memory
//! and blocked-handoff artifacts, and emits the terminal thread events.

use std::time::Instant;

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::warn;

use super::super::AgentRunner;
use super::super::execute_helpers::{emit_blocked_handoff_events, record_terminal_turn_event};
use super::super::headless_planning;
use crate::core::agent::blocked_handoff::{BlockedHandoffResume, write_blocked_handoff_with_resume};
use crate::core::agent::events::{ExecEventRecorder, SessionStoreSinkHandle};
use crate::core::agent::harness_artifacts::existing_harness_artifact_paths;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::steering::SteeringMessage;
use crate::core::agent::task::{Task, TaskOutcome, TaskResults};
use crate::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot};
use crate::llm::usage_cost::SessionCostAccumulator;
use crate::utils::colors::style;

/// Mutable task-finalization state threaded through post-loop finalization.
pub(super) struct TaskFinalization<'a> {
    pub runtime: &'a mut AgentRuntime,
    pub event_recorder: &'a mut ExecEventRecorder,
    pub planning_recovery: &'a mut PlanningRecoverySnapshot,
    pub planning_failure_detail: &'a mut Option<String>,
    pub planning_handoff_text: Option<String>,
    pub should_write_blocked_handoff: &'a mut bool,
    pub session_store_handle: &'a Option<SessionStoreSinkHandle>,
    pub task: &'a Task,
    pub run_started_at: Instant,
    pub agent_prefix: &'a str,
    pub effective_task: &'a Task,
    pub max_tool_loops: usize,
    pub max_context_tokens: usize,
    pub session_costs: &'a SessionCostAccumulator,
}

impl AgentRunner {
    /// Finalize a task after the turn loop: adjust the terminal outcome,
    /// build the summary, persist artifacts, and emit terminal events.
    pub(super) async fn finalize_task_execution(
        &self,
        fin: TaskFinalization<'_>,
    ) -> Result<(TaskResults, Option<UnboundedReceiver<SteeringMessage>>)> {
        let TaskFinalization {
            runtime,
            event_recorder,
            planning_recovery,
            planning_failure_detail,
            mut planning_handoff_text,
            should_write_blocked_handoff,
            session_store_handle,
            task,
            run_started_at,
            agent_prefix,
            effective_task,
            max_tool_loops,
            max_context_tokens,
            session_costs,
        } = fin;
        runtime.state.finalize_outcome(self.max_turns);
        if self.tool_registry.is_planning_active()
            && !matches!(runtime.state.outcome, TaskOutcome::Cancelled | TaskOutcome::Refused { .. })
            && planning_recovery.outcome != PlanningOutcome::AwaitingApproval
        {
            let detail = planning_failure_detail
                .take()
                .unwrap_or_else(|| runtime.state.outcome.description());
            headless_planning::retain_progress(
                planning_recovery,
                &runtime.state.messages,
                &task.description,
                self.provider_client.name(),
                &self.get_selected_model(),
            );
            let already_shown = planning_recovery.input_prompt_shown;
            planning_recovery.outcome = PlanningOutcome::AwaitingInput;
            planning_recovery.input_prompt_shown = true;
            runtime.state.is_completed = false;
            if runtime.state.outcome.is_success() || runtime.state.outcome == TaskOutcome::Unknown {
                runtime.state.outcome =
                    TaskOutcome::failed("Planning needs your input.".to_owned(), vec![], None, None);
            }
            headless_planning::persist_checkpoint(
                session_store_handle.as_ref(),
                planning_recovery,
                &self.session_id,
                runtime.state.stats.turns_executed,
            )
            .await?;
            let diagnostic = headless_planning::awaiting_input_diagnostic(planning_recovery, &self.session_id, &detail);
            planning_handoff_text = Some(diagnostic.text());
            if !already_shown {
                self.record_diagnostic(event_recorder, &diagnostic);
            }
            *should_write_blocked_handoff = true;
        }
        if runtime.state.outcome.is_success() && self.tool_registry.completion_verification_pending() {
            runtime.state.is_completed = false;
            runtime.state.outcome =
                TaskOutcome::failed(self.tool_registry.unverified_completion_reason(), vec![], None, None);
            *should_write_blocked_handoff = true;
        }

        if self.tool_registry.completion_verification_pending() && !runtime.state.outcome.is_success() {
            runtime.state.warnings.push(format!(
                "{} Blocker: {}",
                self.tool_registry.unverified_completion_reason(),
                runtime.state.outcome.description()
            ));
        }
        let total_duration_ms = run_started_at.elapsed().as_millis();

        // Agent execution completed
        self.runner_println(format_args!("{agent_prefix} Done"));

        // Generate meaningful summary based on agent actions
        let average_turn_duration_ms = if runtime.state.turn_count > 0 {
            Some(runtime.state.turn_total_ms as f64 / runtime.state.turn_count as f64)
        } else {
            None
        };

        let max_turn_duration_ms = if runtime.state.turn_count > 0 {
            Some(runtime.state.turn_max_ms)
        } else {
            None
        };

        let outcome = runtime.state.outcome.clone();
        self.thread_handle.replace_messages((*runtime.state.messages).clone());
        let summary = planning_handoff_text.unwrap_or_else(|| {
            self.generate_task_summary(
                effective_task,
                &runtime.state.modified_files,
                &runtime.state.executed_commands,
                &runtime.state.warnings,
                &runtime.state.messages,
                runtime.state.stats.turns_executed,
                runtime.state.max_tool_loop_streak,
                max_tool_loops,
                outcome,
                total_duration_ms,
                average_turn_duration_ms,
                max_turn_duration_ms,
                &runtime.state.stats.total_usage,
            )
        });

        // A completed read-only task must return the actual plan, rather
        // than replacing it with operational task-summary bookkeeping.
        let planning_response =
            if self.tool_registry.is_planning_active() && matches!(runtime.state.outcome, TaskOutcome::Success) {
                runtime
                    .state
                    .messages
                    .iter()
                    .rev()
                    .find(|message| {
                        message.role == crate::llm::provider::MessageRole::Assistant
                            && !message.content.as_text().trim().is_empty()
                    })
                    .map(|message| message.content.as_text().into_owned())
            } else {
                None
            };
        if !summary.trim().is_empty() {
            // Preserve the user-facing result in the event stream.
            event_recorder.agent_message(planning_response.as_deref().unwrap_or(&summary));
            // Also display summary prominently for immediate visibility in TUI transcript
            self.runner_println(format_args!("\n{} Agent Task Summary\n{}", style("[Task]").cyan().bold(), summary));
        }

        let runtime_agent_config = self.core_agent_config();
        if let Err(err) = crate::persistent_memory::finalize_persistent_memory(
            &runtime_agent_config,
            Some(self.config()),
            &runtime.state.messages,
            &self.session_id,
        )
        .await
        {
            warn!(
                error = %err,
                session_id = %self.session_id,
                "Failed to update persistent memory"
            );
        }

        if runtime.state.outcome.is_hard_block() || *should_write_blocked_handoff {
            let relevant_paths = existing_harness_artifact_paths(&self._workspace);
            match write_blocked_handoff_with_resume(
                &self._workspace,
                &self.session_id,
                runtime.state.outcome.code(),
                &runtime.state.outcome.description(),
                &relevant_paths,
                BlockedHandoffResume::Unavailable(
                    "Resume unavailable because the legacy agent runner does not create a session archive.",
                ),
                self.tool_registry.is_planning_active(),
            ) {
                Ok(artifacts) => emit_blocked_handoff_events(
                    event_recorder,
                    &runtime.state.outcome.description(),
                    &artifacts.current_path,
                    &artifacts.archive_path,
                ),
                Err(err) => warn!(
                    error = %err,
                    session_id = %self.session_id,
                    "Failed to persist blocked handoff"
                ),
            }
        }

        let total_usage = runtime.state.stats.total_usage.clone();
        record_terminal_turn_event(event_recorder, &runtime.state.outcome, total_usage.clone());
        event_recorder.thread_completed(
            &self.session_id,
            runtime.state.outcome.thread_completion_subtype(),
            runtime.state.outcome.code(),
            runtime
                .state
                .outcome
                .is_success()
                .then_some(planning_response.as_deref().unwrap_or(&summary)),
            runtime.state.stop_reason.as_deref(),
            total_usage,
            session_costs
                .total()
                .and_then(|cost| serde_json::Number::from_f64(cost.effective_usd)),
            runtime.state.stats.turns_executed,
        );
        let thread_events = event_recorder.take_events();
        let steering_receiver = runtime.take_steering_receiver();
        let state = std::mem::replace(
            &mut runtime.state,
            AgentSessionState::new(self.session_id.clone(), self.max_turns, max_tool_loops, max_context_tokens),
        );

        Ok((state.into_results(summary, thread_events, total_duration_ms), steering_receiver))
    }
}
