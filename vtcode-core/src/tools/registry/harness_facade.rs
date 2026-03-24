//! Harness context accessors for ToolRegistry.

use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use super::HarnessContextSnapshot;
use super::ToolRegistry;
use crate::config::constants::tools;

impl ToolRegistry {
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
    pub fn set_shared_circuit_breaker(
        &self,
        circuit_breaker: Arc<crate::tools::circuit_breaker::CircuitBreaker>,
    ) {
        if let Ok(mut slot) = self.shared_circuit_breaker.write() {
            *slot = Some(circuit_breaker);
        }
    }

    /// Return the shared per-tool circuit breaker when configured.
    pub fn shared_circuit_breaker(
        &self,
    ) -> Option<Arc<crate::tools::circuit_breaker::CircuitBreaker>> {
        self.shared_circuit_breaker
            .read()
            .ok()
            .and_then(|g| g.clone())
    }

    /// Execute a harness-owned verification command through the same exec/sandbox
    /// runtime used by the public `unified_exec` tool while bypassing the
    /// model-facing full-auto allow-list gate.
    pub async fn execute_harness_unified_exec(&self, args: Value) -> Result<Value> {
        let value = self.execute_unified_exec(args).await?;
        let processed = self
            .process_tool_output(tools::UNIFIED_EXEC, value, false)
            .await;
        Ok(super::normalize_tool_output(processed))
    }

    /// Start a harness-owned PTY command session while retaining the session metadata even when
    /// the command exits immediately. ACP terminal sessions use explicit release semantics.
    pub async fn execute_harness_unified_exec_terminal_run(&self, args: Value) -> Result<Value> {
        let value = self
            .execute_harness_unified_exec_terminal_run_raw(args)
            .await?;
        let processed = self
            .process_tool_output(tools::UNIFIED_EXEC, value, false)
            .await;
        Ok(super::normalize_tool_output(processed))
    }

    pub async fn read_harness_exec_session_output(
        &self,
        session_id: &str,
        drain: bool,
    ) -> Result<Option<String>> {
        self.exec_sessions
            .read_session_output(session_id, drain)
            .await
    }

    pub async fn harness_exec_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        self.exec_sessions.is_session_completed(session_id).await
    }

    pub async fn terminate_harness_exec_session(&self, session_id: &str) -> Result<()> {
        self.exec_sessions.terminate_session(session_id).await
    }

    pub async fn close_harness_exec_session(&self, session_id: &str) -> Result<()> {
        let metadata = self.exec_sessions.close_session(session_id).await?;
        if metadata.backend == "pty" {
            self.decrement_active_pty_sessions();
        }
        Ok(())
    }
}
