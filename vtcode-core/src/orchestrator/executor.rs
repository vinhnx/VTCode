use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use super::ScheduledWork;

/// Executor describes a target runtime that can handle scheduled work.
#[async_trait]
pub trait WorkExecutor: Send + Sync {
    async fn execute(&self, work: ScheduledWork) -> Result<Value>;
}

/// Minimal local executor that simulates work for orchestration flows.
#[derive(Debug, Default)]
pub struct LocalExecutor;

#[async_trait]
impl WorkExecutor for LocalExecutor {
    async fn execute(&self, work: ScheduledWork) -> Result<Value> {
        // Simulate asynchronous execution; caller can measure latency.
        sleep(Duration::from_millis(10)).await;
        Ok(serde_json::json!({
            "work_id": work.id,
            "target": work.target.to_string(),
            "metadata": work.metadata,
        }))
    }
}

/// Registry of available executors keyed by logical target.
#[derive(Debug, Default, Clone)]
pub struct ExecutorRegistry {
    executors: std::collections::HashMap<String, Arc<dyn WorkExecutor>>,
}

impl ExecutorRegistry {
    pub fn register(&mut self, target: impl Into<String>, executor: Arc<dyn WorkExecutor>) {
        self.executors.insert(target.into(), executor);
    }

    pub fn get(&self, target: &str) -> Option<Arc<dyn WorkExecutor>> {
        self.executors.get(target).cloned()
    }
}
