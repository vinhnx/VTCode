//! Task execution setup extracted from `execute_task`.
//!
//! Encapsulates the initialization phase that runs before the main
//! turn loop: harness alignment, conversation building, session state
//! creation, and orchestration planning.

use anyhow::Result;
use std::time::Instant;

use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::harness_artifacts;
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

impl AgentRunner {
    /// Prepare everything needed before entering the turn loop.
    ///
    /// This extracts the setup phase from `execute_task` into a testable unit.
    pub(super) async fn prepare_task_execution(
        &mut self,
        task: &Task,
        contexts: &[ContextItem],
    ) -> Result<TaskSetup> {
        // Align harness context with runner session/task for structured telemetry
        self.tool_registry
            .set_harness_session(self.session_id.clone());
        self.tool_registry.set_harness_task(Some(task.id.clone()));

        let steering_receiver = self.steering_receiver.lock().take();

        let agent_prefix = format!("[{}]", self.agent_type);
        // Persist every recorded event to the unified per-session store so it
        // becomes the single source of truth for session state/history.
        let session_sink =
            crate::core::agent::events::session_store_sink(self.workspace(), &self.session_id);
        let event_sink =
            crate::core::agent::events::combine_event_sinks(self.event_sink.clone(), session_sink);
        let mut event_recorder = ExecEventRecorder::new(
            self.session_id.clone(),
            event_sink,
            Some(self.thread_handle.clone()),
        );
        event_recorder.turn_started();
        self.runner_println(format_args!(
            "{agent_prefix} Analyzing request and planning approach..."
        ));

        self.runner_println(format_args!(
            "{} Executing {} task: {}",
            crate::utils::colors::style("[AGENT]")
                .magenta()
                .bold()
                .on_black(),
            self.agent_type,
            task.title
        ));

        let run_started_at = Instant::now();
        let is_simple_task = Self::is_simple_task(task, contexts);
        let prompt_bundle = self
            .build_validated_runtime_prompt_bundle(is_simple_task)
            .await?;

        let review_like = super::continuation::is_review_like_task(task);
        let full_auto_active = self
            .tool_registry
            .current_full_auto_allowlist()
            .await
            .is_some();

        let mut conversation =
            crate::core::agent::conversation::conversation_from_messages(&self.bootstrap_messages);
        conversation.extend(crate::core::agent::conversation::build_conversation(
            task, contexts,
        ));

        let conversation_messages =
            crate::core::agent::conversation::build_messages_from_conversation(&conversation);

        let max_tool_loops = self.config().tools.max_tool_loops;
        let preserve_recent_turns = self.config().context.preserve_recent_turns;
        let max_context_tokens = self.config().context.max_context_tokens;

        let mut session_state = AgentSessionState::new(
            self.session_id.clone(),
            self.max_turns,
            max_tool_loops,
            max_context_tokens,
        );
        session_state.conversation = conversation;
        session_state.messages = std::sync::Arc::new(conversation_messages);
        session_state.reconcile_token_count();
        session_state.last_processed_message_idx = session_state.conversation.len();

        // Context reset: if a reset manifest exists from a previous session
        // (written by `maybe_write_reset_after_compaction` or
        // `maybe_write_reset_on_stall`), clear the conversation history so
        // this session starts fresh from external artifacts only. The orient
        // context in the system prompt already includes the reset banner.
        self.apply_context_reset_if_pending(&mut session_state);

        let mut runtime = AgentRuntime::new(session_state, None, steering_receiver);

        if prompt_bundle.system_prompt_report.over_budget
            && self.config().agent.system_prompt_budget_warning
        {
            runtime.state.warnings.push(format!(
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
            runtime
                .state
                .warnings
                .push(format!("Tool registry init failed: {err}"));
        }

        let orchestration_enabled =
            self.harness_plan_build_evaluate_enabled(full_auto_active, review_like);

        let planner_artifacts = if orchestration_enabled {
            Some(self.run_planner_phase(task, &mut event_recorder).await?)
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
        .with_progress_monitor(ProgressMonitor::with_persistence(
            self.workspace().to_path_buf(),
            &self.session_id,
            &effective_task.id,
        ));
        continuation_controller.prepare(&effective_task).await?;

        let max_budget_usd = self.config().agent.harness.max_budget_usd;
        let max_revision_rounds = self.config().agent.harness.max_revision_rounds;

        Ok(TaskSetup {
            agent_prefix,
            event_recorder,
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
    fn apply_context_reset_if_pending(&self, session_state: &mut AgentSessionState) {
        let manifest_path = harness_artifacts::current_context_reset_path(&self._workspace);

        if !manifest_path.exists() {
            return;
        }

        tracing::info!(
            "Context reset manifest detected — clearing conversation history for fresh start"
        );
        session_state.clear_conversation_history();

        // Consume the manifest so it only triggers once.
        if let Err(e) = std::fs::remove_file(&manifest_path) {
            tracing::warn!(
                error = %e,
                path = %manifest_path.display(),
                "Failed to remove context reset manifest after applying reset"
            );
        }
    }
}
