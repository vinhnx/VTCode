//! Task execution setup extracted from `execute_task`.
//!
//! Encapsulates the initialization phase that runs before the main
//! turn loop: harness alignment, conversation building, session state
//! creation, and orchestration planning.

use anyhow::{Error, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::task::spawn_blocking;

use crate::config::VTCodeConfig;
use crate::core::agent::events::{ExecEventRecorder, SessionStoreSinkHandle};
use crate::core::agent::progress_monitor::ProgressMonitor;
use crate::core::agent::runner::continuation::ContinuationController;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::{ContextItem, Task};

use super::AgentRunner;

/// Result of the task execution setup phase.
///
/// Contains everything the main turn loop needs, pre-computed and validated.
pub struct TaskSetup {
    pub agent_prefix: String,
    pub event_recorder: ExecEventRecorder,
    pub session_store_handle: Option<SessionStoreSinkHandle>,
    pub run_started_at: Instant,
    pub is_simple_task: bool,
    pub prompt_bundle: super::execute::RuntimePromptBundle,
    pub preserve_recent_turns: usize,
    pub max_tool_loops: usize,
    pub max_context_tokens: usize,
    pub runtime: AgentRuntime,
    pub continuation_controller: ContinuationController,
    pub effective_task: Task,
    pub orchestration_enabled: bool,
    pub max_budget_usd: Option<f64>,
    pub max_revision_rounds: usize,
}

async fn finish_failed_setup(
    event_recorder: &mut ExecEventRecorder,
    session_id: &str,
    error: &Error,
    session_store_handle: SessionStoreSinkHandle,
) {
    event_recorder.thread_failed(session_id, &error.to_string(), 1);
    if let Err(close_error) = session_store_handle.close().await {
        tracing::error!(
            session_id,
            error = %close_error,
            "failed to close canonical session store after setup failure"
        );
    }
}

impl AgentRunner {
    /// Prepare everything needed before entering the turn loop.
    ///
    /// This extracts the setup phase from `execute_task` into a testable unit.
    pub(super) async fn prepare_task_execution(&mut self, task: &Task, contexts: &[ContextItem]) -> Result<TaskSetup> {
        // Align harness context with runner session/task for structured telemetry
        self.tool_registry.set_harness_session(self.session_id.clone());
        self.tool_registry.set_harness_task(Some(task.id.clone()));

        let steering_receiver = self.steering_receiver.lock().take();

        let retention_workspace = self.workspace().to_path_buf();
        let retention_session_id = self.session_id.clone();
        match spawn_blocking(move || {
            vtcode_memory::apply_retention_preserving(
                &retention_workspace,
                vtcode_memory::RetentionPolicy::default(),
                Some(retention_session_id.as_str()),
            )
        })
        .await
        {
            Ok(Ok(removed)) if removed > 0 => {
                tracing::debug!(removed, "pruned closed canonical sessions before AgentRunner execution");
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "canonical session retention failed before AgentRunner execution");
            }
            Err(error) => {
                tracing::warn!(error = %error, "canonical session retention task failed before AgentRunner execution");
            }
        }

        let agent_prefix = format!("[{}]", self.agent_type);
        // Persist every recorded event to the unified per-session store so it
        // becomes the single source of truth for session state/history.
        let (session_sink, session_store_handle) =
            crate::core::agent::events::session_store_sink_with_handle(self.workspace(), &self.session_id).await?;
        self.tool_registry
            .set_decision_evidence_validator(session_store_handle.decision_validator());
        if let Some(controller) = self.tool_registry.subagent_controller() {
            controller
                .set_matrix_persistence(session_store_handle.matrix_persistence())
                .await?;
        }
        let event_sink = crate::core::agent::events::combine_event_sinks(self.event_sink.clone(), Some(session_sink));
        let mut event_recorder =
            ExecEventRecorder::new(self.session_id.clone(), event_sink, Some(self.thread_handle.clone()));
        let execution_task_id = event_recorder.task_started(&task.description);
        self.tool_registry.set_harness_task(Some(execution_task_id));
        self.runner_println(format_args!("{agent_prefix} Analyzing request and planning approach..."));

        self.runner_println(format_args!(
            "{} Executing {} task: {}",
            crate::utils::colors::style("[AGENT]").magenta().bold().on_black(),
            self.agent_type,
            task.title
        ));

        let run_started_at = Instant::now();
        let is_simple_task = Self::is_simple_task(task, contexts);
        let prompt_bundle = match self.build_validated_runtime_prompt_bundle(is_simple_task).await {
            Ok(bundle) => bundle,
            Err(error) => {
                finish_failed_setup(&mut event_recorder, &self.session_id, &error, session_store_handle).await;
                return Err(error);
            }
        };

        let review_like = super::continuation::is_review_like_task(task);
        let full_auto_active = self.tool_registry.current_full_auto_allowlist().await.is_some();

        let mut conversation = crate::core::agent::conversation::conversation_from_messages(&self.bootstrap_messages);
        conversation.extend(crate::core::agent::conversation::build_conversation(task, contexts));

        let conversation_messages = crate::core::agent::conversation::build_messages_from_conversation(&conversation);

        let max_tool_loops = full_auto_tool_loop_budget(self.config(), full_auto_active);
        let preserve_recent_turns = self.config().context.preserve_recent_turns;
        let max_context_tokens = crate::compaction::effective_context_budget(
            Some(self.config()),
            self.provider_client.as_ref(),
            &self.model,
        );

        let mut session_state =
            AgentSessionState::new(self.session_id.clone(), self.max_turns, max_tool_loops, max_context_tokens);
        session_state.conversation = conversation;
        session_state.messages = Arc::new(conversation_messages);
        session_state.reconcile_token_count();
        session_state.last_processed_message_idx = session_state.conversation.len();

        // Context reset: if a reset manifest exists from a previous session
        // (written by `maybe_write_reset_after_compaction` or
        // `maybe_write_reset_on_stall`), clear the conversation history so
        // this session starts fresh from external artifacts only. The orient
        // context in the system prompt already includes the reset banner.
        self.apply_context_reset_if_pending(&mut session_state).await?;

        let mut runtime = AgentRuntime::new(session_state, None, steering_receiver);

        if prompt_bundle.system_prompt_report.over_budget && self.config().agent.system_prompt_budget_warning {
            runtime.state.push_warning(format!(
                "Base system prompt is ~{} tokens (budget {}); later appendices (session context, runtime line, subagents roster) add more. Consider a leaner system prompt mode or enable agent.trim_system_prompt.",
                prompt_bundle.system_prompt_report.token_estimate,
                self.config().agent.max_system_prompt_tokens
            ));
        }

        if let Err(err) = self.tool_registry.initialize_async().await {
            tracing::warn!(
                error = %err,
                "Tool registry initialization failed at task start"
            );
            runtime.state.push_warning(format!("Tool registry init failed: {err}"));
        }

        let orchestration_enabled = self.harness_plan_build_evaluate_enabled(full_auto_active, review_like);

        let planner_artifacts = if orchestration_enabled {
            Some(match self.run_planner_phase(task, &mut event_recorder).await {
                Ok(artifacts) => artifacts,
                Err(error) => {
                    finish_failed_setup(&mut event_recorder, &self.session_id, &error, session_store_handle).await;
                    return Err(error);
                }
            })
        } else {
            None
        };

        let effective_task = planner_artifacts
            .as_ref()
            .map(|artifacts| self.augment_generator_task(task, artifacts))
            .unwrap_or_else(|| task.clone());

        let mut continuation_controller = ContinuationController::new(
            self._workspace.clone(),
            self.tool_registry.planning_workflow_state(),
            self.config().agent.harness.continuation_policy.clone(),
            full_auto_active,
            self.tool_registry.is_planning_active(),
            review_like,
            self.config().agent.harness.context_reset_mode.clone(),
            self.config().agent.harness.context_reset_stall_threshold,
        )
        .with_transition_identity(self.session_id.clone(), task.id.clone())
        .with_progress_monitor(
            ProgressMonitor::with_persistence_async(
                self.workspace().to_path_buf(),
                &self.session_id,
                &effective_task.id,
            )
            .await,
        );
        if let Err(error) = continuation_controller.prepare(&effective_task).await {
            finish_failed_setup(&mut event_recorder, &self.session_id, &error, session_store_handle).await;
            return Err(error);
        }

        let max_budget_usd = self.config().agent.harness.max_budget_usd;
        let max_revision_rounds = self.config().agent.harness.max_revision_rounds;

        Ok(TaskSetup {
            agent_prefix,
            event_recorder,
            session_store_handle: Some(session_store_handle),
            run_started_at,
            is_simple_task,
            prompt_bundle,
            preserve_recent_turns,
            max_tool_loops,
            max_context_tokens,
            runtime,
            continuation_controller,
            effective_task,
            orchestration_enabled,
            max_budget_usd,
            max_revision_rounds,
        })
    }

    /// Check for a pending context reset manifest and clear conversation
    /// history if one exists.
    ///
    /// This completes the context reset mechanism (TD-017): the manifest was
    /// written by `maybe_write_reset_after_compaction` or
    /// `maybe_write_reset_on_stall` during a previous session, and this method
    /// acts on it by clearing the conversation history so the agent starts
    /// fresh from external artifacts only. The manifest is consumed (deleted)
    /// so it only triggers once.
    async fn apply_context_reset_if_pending(&self, session_state: &mut AgentSessionState) -> Result<()> {
        if crate::core::agent::context_reset::read_transition_manifest_async(&self._workspace)
            .await?
            .is_none()
        {
            return Ok(());
        }

        tracing::info!("Context reset manifest detected — clearing conversation history for fresh start");
        session_state.clear_conversation_history();

        // Consume the manifest so it only triggers once.
        crate::core::agent::context_reset::consume_manifest_async(&self._workspace).await?;
        Ok(())
    }
}

/// Tool-loop budget for an exec run. Full-auto runs start at the hard cap
/// because no operator is present to answer a mid-run grant prompt; the cap
/// itself remains the runaway bound. Ordinary runs keep the configured base
/// limit, and `0` stays unlimited in every mode.
fn full_auto_tool_loop_budget(config: &VTCodeConfig, full_auto_active: bool) -> usize {
    let configured = config.tools.max_tool_loops;
    let grants_enabled = full_auto_active && config.automation.full_auto.enabled;
    if !grants_enabled || configured == 0 || !config.automation.full_auto.auto_grant_tool_limits {
        return configured;
    }
    let capped = crate::config::constants::tool_limits::tool_loop_hard_cap(configured, false);
    if capped != configured {
        let increment = capped.saturating_sub(configured);
        tracing::info!(
            configured,
            capped,
            increment,
            "Full-auto auto-granted +{} tool loops upfront instead of prompting mid-run",
            increment,
        );
    }
    capped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_auto_config(max_tool_loops: usize, auto_grant_tool_limits: bool) -> VTCodeConfig {
        let mut config = VTCodeConfig::default();
        config.tools.max_tool_loops = max_tool_loops;
        config.automation.full_auto.enabled = true;
        config.automation.full_auto.auto_grant_tool_limits = auto_grant_tool_limits;
        config
    }

    #[test]
    fn full_auto_exec_starts_at_the_hard_cap() {
        assert_eq!(full_auto_tool_loop_budget(&full_auto_config(20, true), true), 60);
        assert_eq!(full_auto_tool_loop_budget(&full_auto_config(40, true), true), 120);
    }

    #[test]
    fn exec_budget_keeps_base_limit_without_grants() {
        assert_eq!(full_auto_tool_loop_budget(&full_auto_config(40, true), false), 40);
        assert_eq!(full_auto_tool_loop_budget(&full_auto_config(40, false), true), 40);
        let mut disabled = VTCodeConfig::default();
        disabled.tools.max_tool_loops = 40;
        assert_eq!(full_auto_tool_loop_budget(&disabled, true), 40);
    }

    #[test]
    fn exec_budget_never_shrinks_generous_or_unlimited_limits() {
        assert_eq!(full_auto_tool_loop_budget(&full_auto_config(0, true), true), 0);
        assert_eq!(full_auto_tool_loop_budget(&full_auto_config(200, true), true), 200);
    }
}
