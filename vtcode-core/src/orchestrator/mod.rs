//! Distributed orchestration primitives for cloud/edge/on-prem scheduling.

mod executor;
mod scheduler;

use anyhow::{Context, Result};
use serde_json::Value;
use std::fmt;
use std::sync::Arc;

pub use executor::{ExecutorRegistry, LocalExecutor, WorkExecutor};
pub use scheduler::Scheduler;

/// Execution target supported by the orchestrator.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecutionTarget {
    Cloud,
    Edge,
    OnPrem,
    Custom(String),
}

impl fmt::Display for ExecutionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionTarget::Cloud => write!(f, "cloud"),
            ExecutionTarget::Edge => write!(f, "edge"),
            ExecutionTarget::OnPrem => write!(f, "on-prem"),
            ExecutionTarget::Custom(name) => write!(f, "{name}"),
        }
    }
}

/// Workload scheduled for execution.
#[derive(Debug, Clone)]
pub struct ScheduledWork {
    pub id: String,
    pub target: ExecutionTarget,
    pub payload: Value,
    pub metadata: Value,
}

impl ScheduledWork {
    pub fn new(
        id: impl Into<String>,
        target: ExecutionTarget,
        payload: Value,
        metadata: Value,
    ) -> Self {
        Self {
            id: id.into(),
            target,
            payload,
            metadata,
        }
    }
}

/// Main orchestrator that coordinates scheduling and execution.
pub struct DistributedOrchestrator {
    scheduler: Scheduler,
    executors: ExecutorRegistry,
}

impl DistributedOrchestrator {
    pub fn new() -> Self {
        let mut executors = ExecutorRegistry::default();
        executors.register("cloud", Arc::new(LocalExecutor));
        executors.register("edge", Arc::new(LocalExecutor));
        executors.register("on-prem", Arc::new(LocalExecutor));

        Self {
            scheduler: Scheduler::new(),
            executors,
        }
    }

    pub fn register_executor(
        &mut self,
        target: impl Into<String>,
        executor: Arc<dyn WorkExecutor>,
    ) {
        self.executors.register(target, executor);
    }

    pub async fn submit(&self, work: ScheduledWork) -> Result<()> {
        self.scheduler.enqueue(work).await;
        Ok(())
    }

    pub async fn tick(&self) -> Result<Option<Value>> {
        if let Some(work) = self.scheduler.next().await {
            let target_key = work.target.to_string();
            let executor = self
                .executors
                .get(&target_key)
                .context("executor not registered for target")?;

            let result = executor.execute(work).await?;
            return Ok(Some(result));
        }

        Ok(None)
    }

    pub async fn queue_depth(&self) -> usize {
        self.scheduler.queue_depth().await
    }
}

impl Default for DistributedOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn schedules_and_executes_work() {
        let orchestrator = DistributedOrchestrator::new();
        orchestrator
            .submit(ScheduledWork::new(
                "job-1",
                ExecutionTarget::Cloud,
                serde_json::json!({"task": "compile"}),
                Value::Null,
            ))
            .await
            .expect("submit should succeed");

        let result = orchestrator.tick().await.expect("tick should run");
        assert!(result.is_some());
    }
}
