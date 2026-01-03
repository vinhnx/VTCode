//! Optimized tool registry with reduced lock contention and improved caching

use anyhow::Result;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::core::memory_pool::global_pool;
use crate::tools::registry::ToolExecutionRecord;

/// Lock-free tool metadata cache
#[derive(Clone)]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub is_cached: bool,
    pub avg_execution_time_ms: u64,
}

/// Optimized tool registry with minimal lock contention
pub struct OptimizedToolRegistry {
    /// Read-heavy tool metadata cache (rarely updated)
    tool_metadata: Arc<RwLock<HashMap<String, Arc<ToolMetadata>>>>,

    /// Execution semaphore to limit concurrent tool executions
    execution_semaphore: Arc<Semaphore>,

    /// Hot path cache for frequently accessed tools
    hot_cache: Arc<RwLock<HashMap<String, Arc<ToolMetadata>>>>,

    /// Execution statistics (append-only for performance)
    execution_stats: Arc<RwLock<Vec<ToolExecutionRecord>>>,
}

impl OptimizedToolRegistry {
    pub fn new(max_concurrent_tools: usize) -> Self {
        Self {
            tool_metadata: Arc::new(RwLock::new(HashMap::with_capacity(64))),
            execution_semaphore: Arc::new(Semaphore::new(max_concurrent_tools)),
            hot_cache: Arc::new(RwLock::new(HashMap::with_capacity(16))),
            execution_stats: Arc::new(RwLock::new(Vec::with_capacity(1024))),
        }
    }

    /// Fast tool lookup with hot cache optimization
    pub fn get_tool_metadata(&self, tool_name: &str) -> Option<Arc<ToolMetadata>> {
        // Try hot cache first (most frequently used tools)
        if let Some(metadata) = self.hot_cache.read().get(tool_name) {
            return Some(Arc::clone(metadata));
        }

        // Fallback to main cache
        let metadata = self.tool_metadata.read().get(tool_name).cloned()?;

        // Promote to hot cache if accessed frequently
        self.promote_to_hot_cache(tool_name, &metadata);

        Some(metadata)
    }

    /// Register tool metadata with minimal locking
    pub fn register_tool(&self, metadata: ToolMetadata) {
        let tool_name = metadata.name.clone();
        let metadata_arc = Arc::new(metadata);

        self.tool_metadata.write().insert(tool_name, metadata_arc);
    }

    /// Execute tool with concurrency control and performance tracking
    pub async fn execute_tool_optimized(&self, tool_name: &str, _args: Value) -> Result<Value> {
        // Acquire execution permit
        let _permit = self.execution_semaphore.acquire().await?;

        let start_time = std::time::Instant::now();

        // Get reusable memory from pool
        let pool = global_pool();
        let result_string = pool.get_string();

        // Simulate tool execution (replace with actual implementation)
        let result = self.execute_tool_impl(tool_name, _args).await;

        let execution_time = start_time.elapsed();

        // Update execution statistics asynchronously
        self.record_execution_stats(tool_name, execution_time, result.is_ok())
            .await;

        // Return memory to pool
        pool.return_string(result_string);

        result
    }

    /// Promote frequently accessed tools to hot cache
    fn promote_to_hot_cache(&self, tool_name: &str, metadata: &Arc<ToolMetadata>) {
        let mut hot_cache = self.hot_cache.write();
        if hot_cache.len() < 16 {
            hot_cache.insert(tool_name.to_string(), Arc::clone(metadata));
        }
    }

    /// Record execution statistics with minimal blocking
    async fn record_execution_stats(
        &self,
        tool_name: &str,
        execution_time: std::time::Duration,
        success: bool,
    ) {
        // Use a separate task to avoid blocking the main execution path
        let stats_arc = Arc::clone(&self.execution_stats);
        let tool_name = tool_name.to_string();

        tokio::spawn(async move {
            let record = ToolExecutionRecord {
                tool_name,
                requested_name: String::new(), // Optimize: reuse from pool
                is_mcp: false,
                mcp_provider: None,
                args: Value::Null,
                result: if success {
                    Ok(Value::Null)
                } else {
                    Err("Error".to_string())
                },
                timestamp: std::time::SystemTime::now(),
                success,
                context: crate::tools::registry::HarnessContextSnapshot::new(
                    "optimized".to_string(),
                    None,
                ),
                timeout_category: None,
                base_timeout_ms: None,
                adaptive_timeout_ms: None,
                effective_timeout_ms: Some(execution_time.as_millis() as u64),
                circuit_breaker: false,
            };

            stats_arc.write().push(record);
        });
    }

    /// Actual tool execution implementation
    async fn execute_tool_impl(&self, _tool_name: &str, _args: Value) -> Result<Value> {
        // Placeholder - replace with actual tool execution logic
        Ok(Value::String("success".to_string()))
    }

    /// Get execution statistics without blocking
    pub fn get_stats_snapshot(&self) -> Vec<ToolExecutionRecord> {
        self.execution_stats.read().clone()
    }

    /// Clear hot cache periodically to prevent memory bloat
    pub fn clear_hot_cache(&self) {
        self.hot_cache.write().clear();
    }
}
