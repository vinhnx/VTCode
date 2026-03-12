//! Harness context accessors for ToolRegistry.

use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use super::HarnessContextSnapshot;
use super::ToolRegistry;

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
        self.execute_unified_exec(args).await
    }
}
