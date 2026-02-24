//! Parallel Tool Batch Execution
//!
//! Batches multiple tool calls and executes them in parallel when safe (read-only),
//! or sequentially when mutating operations are involved.
//!
//! Uses the `UnifiedToolExecutor` trait for actual execution and integrates with
//! the existing async pipeline infrastructure.

use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::tools::unified_error::{UnifiedErrorKind, UnifiedToolError};
use crate::tools::unified_executor::{
    ToolExecutionContext, UnifiedExecutionResult, UnifiedToolExecutor,
};

/// A queued tool call with its execution context
#[derive(Debug, Clone)]
pub struct QueuedToolCall {
    pub name: String,
    pub args: Value,
    pub ctx: ToolExecutionContext,
}

/// Batch of tool calls for parallel or sequential execution
#[derive(Debug, Default)]
pub struct ParallelToolBatch {
    calls: Vec<QueuedToolCall>,
    max_concurrency: usize,
}

impl ParallelToolBatch {
    /// Create a new empty batch with default concurrency (8)
    pub fn new() -> Self {
        Self {
            calls: Vec::new(),
            max_concurrency: 8,
        }
    }

    /// Create a batch with custom max concurrency
    pub fn with_concurrency(max_concurrency: usize) -> Self {
        Self {
            calls: Vec::new(),
            max_concurrency: max_concurrency.max(1),
        }
    }

    /// Add a tool call to the batch
    pub fn add_call(&mut self, name: &str, args: Value, ctx: ToolExecutionContext) {
        self.calls.push(QueuedToolCall {
            name: name.to_string(),
            args,
            ctx,
        });
    }

    /// Check if a tool is safe for parallel execution based on name patterns
    ///
    /// Read-only tools are identified by prefixes:
    /// - read_, list_, get_, grep_, search_, find_
    ///
    /// And specific known read-only tools:
    /// - agent_info, code_intelligence (with read-only operations)
    #[inline]
    pub fn is_parallel_safe(name: &str) -> bool {
        const READ_ONLY_PREFIXES: &[&str] =
            &["read_", "list_", "get_", "grep_", "search_", "find_"];

        const READ_ONLY_TOOLS: &[&str] = &["agent_info", "glob", "fetch_url", "web_search"];

        READ_ONLY_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
            || READ_ONLY_TOOLS.contains(&name)
    }

    /// Check if all calls in the batch are safe for parallel execution
    pub fn all_parallel_safe(&self) -> bool {
        self.calls
            .iter()
            .all(|call| Self::is_parallel_safe(&call.name))
    }

    /// Number of queued calls
    pub fn len(&self) -> usize {
        self.calls.len()
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// Execute the batch using the provided executor
    ///
    /// - Parallel execution for read-only tools
    /// - Sequential execution if any mutating tools are present
    pub async fn execute_batch<E: UnifiedToolExecutor>(
        &self,
        executor: &E,
    ) -> Vec<Result<UnifiedExecutionResult, UnifiedToolError>> {
        if self.calls.is_empty() {
            return Vec::new();
        }

        if self.all_parallel_safe() {
            self.execute_parallel(executor).await
        } else {
            self.execute_sequential(executor).await
        }
    }

    /// Execute all calls in parallel with concurrency limiting
    async fn execute_parallel<E: UnifiedToolExecutor>(
        &self,
        executor: &E,
    ) -> Vec<Result<UnifiedExecutionResult, UnifiedToolError>> {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let mut handles = Vec::with_capacity(self.calls.len());

        for call in &self.calls {
            let sem = semaphore.clone();
            let name = call.name.clone();
            let args = call.args.clone();
            let ctx = call.ctx.clone();

            let handle = async move {
                let _permit = sem.acquire().await.map_err(|err| {
                    UnifiedToolError::new(
                        UnifiedErrorKind::ExecutionFailed,
                        "Failed to schedule parallel tool execution",
                    )
                    .with_tool_name(&name)
                    .with_source(anyhow::Error::new(err).context(format!(
                        "Failed to acquire semaphore permit for tool '{}'",
                        name
                    )))
                })?;
                executor.execute(ctx, &name, args).await
            };

            handles.push(handle);
        }

        futures::future::join_all(handles).await
    }

    /// Execute all calls sequentially (for batches with mutating tools)
    async fn execute_sequential<E: UnifiedToolExecutor>(
        &self,
        executor: &E,
    ) -> Vec<Result<UnifiedExecutionResult, UnifiedToolError>> {
        let mut results = Vec::with_capacity(self.calls.len());

        for call in &self.calls {
            let result = executor
                .execute(call.ctx.clone(), &call.name, call.args.clone())
                .await;
            results.push(result);
        }

        results
    }

    /// Partition batch into parallel-safe and sequential groups
    ///
    /// Returns (parallel_batch, sequential_batch)
    pub fn partition(self) -> (ParallelToolBatch, ParallelToolBatch) {
        let mut parallel = ParallelToolBatch::with_concurrency(self.max_concurrency);
        let mut sequential = ParallelToolBatch::with_concurrency(1);

        for call in self.calls {
            if Self::is_parallel_safe(&call.name) {
                parallel.calls.push(call);
            } else {
                sequential.calls.push(call);
            }
        }

        (parallel, sequential)
    }

    /// Execute with smart partitioning: run parallel-safe tools first, then sequential
    pub async fn execute_partitioned<E: UnifiedToolExecutor>(
        self,
        executor: &E,
    ) -> Vec<Result<UnifiedExecutionResult, UnifiedToolError>> {
        let (parallel, sequential) = self.partition();
        let mut results = Vec::with_capacity(parallel.len() + sequential.len());

        // Run parallel-safe tools first
        if !parallel.is_empty() {
            results.extend(parallel.execute_parallel(executor).await);
        }

        // Then run sequential tools
        if !sequential.is_empty() {
            results.extend(sequential.execute_sequential(executor).await);
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_parallel_safe() {
        // Read-only prefixes
        assert!(ParallelToolBatch::is_parallel_safe("read_file"));
        assert!(ParallelToolBatch::is_parallel_safe("list_files"));
        assert!(ParallelToolBatch::is_parallel_safe("get_errors"));
        assert!(ParallelToolBatch::is_parallel_safe("grep_file"));
        assert!(ParallelToolBatch::is_parallel_safe("search_tools"));
        assert!(ParallelToolBatch::is_parallel_safe("find_references"));

        // Known read-only tools
        assert!(ParallelToolBatch::is_parallel_safe("agent_info"));
        assert!(ParallelToolBatch::is_parallel_safe("glob"));
        assert!(ParallelToolBatch::is_parallel_safe("web_search"));

        // Mutating tools
        assert!(!ParallelToolBatch::is_parallel_safe("write_file"));
        assert!(!ParallelToolBatch::is_parallel_safe("edit_file"));
        assert!(!ParallelToolBatch::is_parallel_safe("delete_file"));
        assert!(!ParallelToolBatch::is_parallel_safe("shell"));
        assert!(!ParallelToolBatch::is_parallel_safe("apply_patch"));
    }

    #[test]
    fn test_batch_operations() {
        let mut batch = ParallelToolBatch::new();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);

        let ctx = ToolExecutionContext::new("test-session");
        batch.add_call(
            "read_file",
            serde_json::json!({"path": "/tmp/test"}),
            ctx.clone(),
        );
        batch.add_call(
            "list_files",
            serde_json::json!({"path": "/tmp"}),
            ctx.clone(),
        );

        assert!(!batch.is_empty());
        assert_eq!(batch.len(), 2);
        assert!(batch.all_parallel_safe());

        batch.add_call("write_file", serde_json::json!({"path": "/tmp/out"}), ctx);
        assert!(!batch.all_parallel_safe());
    }

    #[test]
    fn test_partition() {
        let mut batch = ParallelToolBatch::new();
        let ctx = ToolExecutionContext::new("test-session");

        batch.add_call("read_file", serde_json::json!({}), ctx.clone());
        batch.add_call("write_file", serde_json::json!({}), ctx.clone());
        batch.add_call("list_files", serde_json::json!({}), ctx.clone());
        batch.add_call("delete_file", serde_json::json!({}), ctx);

        let (parallel, sequential) = batch.partition();

        assert_eq!(parallel.len(), 2); // read_file, list_files
        assert_eq!(sequential.len(), 2); // write_file, delete_file
    }
}
