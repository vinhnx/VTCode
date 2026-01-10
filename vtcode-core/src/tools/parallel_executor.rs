use crate::tools::traits::ToolExecutor;
use anyhow::{Context, Result};
use serde_json::Value; // Removed futures::stream::FuturesUnordered and futures::StreamExt
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Executor that runs tools in parallel with concurrency limits.
pub struct ParallelToolExecutor {
    inner: Arc<dyn ToolExecutor>,
    semaphore: Arc<Semaphore>,
}

impl ParallelToolExecutor {
    /// Create a new parallel executor wrapping an inner executor.
    pub fn new(inner: Arc<dyn ToolExecutor>, max_concurrency: usize) -> Self {
        Self {
            inner,
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
        }
    }

    /// Execute a set of tool calls in parallel.
    pub async fn execute_parallel(
        &self,
        calls: Vec<(String, Value)>,
    ) -> Vec<Result<Value>> {
        let mut results = Vec::with_capacity(calls.len());
        
        let handles = calls.into_iter().map(|(name, args)| {
            let inner = self.inner.clone();
            let semaphore = self.semaphore.clone();
            
            tokio::spawn(async move {
                // Acquire permit
                let _permit = semaphore.acquire().await.context("Semaphore closed");
                if let Err(e) = _permit {
                    return Err(e);
                }
                inner.execute_tool(&name, args).await
            })
        }).collect::<Vec<_>>();

        let joined_results = futures::future::join_all(handles).await;

        for res in joined_results {
            match res {
                Ok(Ok(val)) => results.push(Ok(val)),
                Ok(Err(e)) => results.push(Err(e)),
                Err(e) => results.push(Err(e.into())), // JoinError
            }
        }

        results
    }
}

/// A group of tool calls scheduled for execution
pub struct ExecutionGroup {
    pub tool_calls: Vec<(String, Arc<Value>, String)>,
}

impl ExecutionGroup {
    pub fn len(&self) -> usize {
        self.tool_calls.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.tool_calls.is_empty()
    }
}

/// Planner that groups tool calls into execution stages
pub struct ParallelExecutionPlanner;

impl Default for ParallelExecutionPlanner {
    fn default() -> Self {
        Self::new()
    }
}

impl ParallelExecutionPlanner {
    pub fn new() -> Self {
        Self
    }

    /// Plan execution groups.
    /// Since we don't have access to the registry here to check individual tool safety/policy,
    /// we aggressively group all calls into a single batch. 
    /// The caller (e.g., tool_outcomes.rs) is responsible for verifying the safety 
    /// of the group and falling back to sequential execution if any member is unsafe.
    pub fn plan(&self, calls: &[(String, Arc<Value>, String)]) -> Vec<ExecutionGroup> {
        if calls.is_empty() {
            return Vec::new();
        }

        // Optimistic strategy: Try to run everything in one parallel batch.
        // The consumer will demote to sequential if any tool in the batch is unsafe.
        // A more sophisticated planner would need the ToolRegistry to split based on dependencies.
        vec![ExecutionGroup {
            tool_calls: calls.to_vec(),
        }]
    }
}
