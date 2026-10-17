//! Harness context accessors for ToolRegistry.

use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use super::HarnessContextSnapshot;
use super::ToolRegistry;
use crate::config::constants::tools;

impl ToolRegistry {
    /// Begin a genuine request, optionally restoring adoption from its persisted
    /// history or explicit continuation. Internal turns must not reset this latch.
    pub fn begin_tracker_request(&self, adopted: bool) {
        self.execution_history.begin_tracker_request();
        if !adopted {
            self.set_required_completion_verifiers(Vec::new());
        }
        self.harness_context
            .tracker_adopted
            .store(adopted, std::sync::atomic::Ordering::Relaxed);
    }

    /// Successful current-request adoption is independent of compactable history.
    pub fn tracker_adopted_for_request(&self) -> bool {
        self.harness_context.tracker_adopted.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Attach the canonical queue barrier used by public decision recording.
    pub fn set_decision_evidence_validator(&self, validator: crate::core::agent::events::DecisionEvidenceValidator) {
        *self.harness_context.decision_validator.write() = Some(validator);
    }
    async fn process_harness_command_session_output(&self, value: Value) -> Result<Value> {
        let processed = self
            .process_tool_output(
                tools::UNIFIED_EXEC,
                value,
                false,
                vtcode_utility_tool_specs::DEFAULT_MAX_OUTPUT_TOKENS,
            )
            .await;
        Ok(super::normalize_tool_output(processed))
    }

    /// Update harness session identifier used for structured tool telemetry
    pub fn set_harness_session(&self, session_id: impl Into<String>) {
        self.harness_context.set_session_id(session_id);
    }

    /// Update current task identifier used for structured tool telemetry
    pub fn set_harness_task(&self, task_id: Option<String>) {
        self.harness_context.set_task_id(task_id);
    }

    /// Snapshot harness context metadata.
    pub fn harness_context_snapshot(&self) -> HarnessContextSnapshot {
        self.harness_context.snapshot()
    }

    /// Attach the runloop's shared per-tool circuit breaker.
    pub fn set_shared_circuit_breaker(&self, circuit_breaker: Arc<crate::tools::circuit_breaker::CircuitBreaker>) {
        if let Ok(mut slot) = self.shared_circuit_breaker.write() {
            *slot = Some(circuit_breaker);
        }
    }

    /// Return the shared per-tool circuit breaker when configured.
    pub fn shared_circuit_breaker(&self) -> Option<Arc<crate::tools::circuit_breaker::CircuitBreaker>> {
        self.shared_circuit_breaker.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Grant an exact, single-use inspection exemption for a command supplied
    /// directly by a trusted user. Model tool approval must never call this.
    pub fn trusted_user_command_exemption(
        &self,
        args: &Value,
    ) -> Result<crate::tools::source_inspection::TrustedCommandExemption> {
        let cwd = self
            .pty_manager()
            .working_dir_candidate(crate::tools::command_args::working_dir_text(args))?;
        Ok(self
            .edited_file_monitor_ref()
            .source_inspection()
            .scoped_user_command(args, &cwd))
    }

    /// Execute a harness-owned verification command through the same exec/sandbox
    /// runtime used by the public `command_session` tool while bypassing the
    /// model-facing full-auto allow-list gate.
    pub async fn execute_harness_command_session(&self, args: Value) -> Result<Value> {
        let value = self.execute_command_session(args).await?;
        self.process_harness_command_session_output(value).await
    }

    /// Start a harness-owned PTY command session while retaining the session metadata even when
    /// the command exits immediately. ACP terminal sessions use explicit release semantics.
    pub async fn execute_harness_command_session_terminal_run(&self, args: Value) -> Result<Value> {
        let launch = self.begin_completion_verifier(&args);
        let result = self.execute_harness_command_session_terminal_run_raw(args.clone()).await;
        self.observe_completion_verifier(launch, &args, &result);
        let value = result?;
        self.observe_completion_mutation(tools::EXEC_COMMAND, &args, &value);
        self.process_harness_command_session_output(value).await
    }

    pub async fn read_harness_exec_session_output(&self, session_id: &str, drain: bool) -> Result<Option<String>> {
        self.exec_sessions.read_session_output(session_id, drain).await
    }

    /// Observe the owned native terminal session's actual exit for runtime verification.
    pub async fn harness_exec_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let result = self.exec_sessions.is_session_completed(session_id).await;
        if let Ok(Some(exit_code)) = result {
            self.observe_completion_verifier(
                None,
                &serde_json::json!({"action":"wait", "session_id":session_id}),
                &Ok(serde_json::json!({"exit_code":exit_code})),
            );
        }
        result
    }

    /// Terminate an owned session and retire any active mutation boundary.
    pub async fn terminate_harness_exec_session(&self, session_id: &str) -> Result<()> {
        let result = self.exec_sessions.terminate_session(session_id).await;
        if result.is_ok() {
            self.observe_completion_verifier(
                None,
                &serde_json::json!({"action":"close", "session_id":session_id}),
                &Ok(serde_json::json!({"success":true})),
            );
        }
        result
    }

    pub async fn close_harness_exec_session(&self, session_id: &str) -> Result<()> {
        self.close_exec_session(session_id).await?;
        self.observe_completion_verifier(
            None,
            &serde_json::json!({"action":"close", "session_id":session_id}),
            &Ok(serde_json::json!({"success":true})),
        );
        Ok(())
    }

    /// Bounded snapshot of all exec sessions still running for turn-end
    /// diagnostics and telemetry. Newest first; capped by the caller.
    pub async fn in_progress_exec_sessions(&self, cap: usize) -> Vec<crate::tools::types::VTCodeExecSession> {
        self.exec_sessions.in_progress_exec_sessions(cap).await
    }

    pub async fn in_progress_foreground_exec_sessions(
        &self,
        cap: usize,
    ) -> Vec<crate::tools::types::VTCodeExecSession> {
        self.exec_sessions.in_progress_foreground_exec_sessions(cap).await
    }
}
