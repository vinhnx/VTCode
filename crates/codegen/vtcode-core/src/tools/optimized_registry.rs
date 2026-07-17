//! Optimized tool registry with reduced lock contention and improved caching

use anyhow::Result;
use hashbrown::HashMap;
use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::tools::registry::ToolExecutionRecord;

/// Lock-free tool metadata cache
#[derive(Clone)]
pub struct CachedToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub is_cached: bool,
    pub avg_execution_time_ms: u64,
}

/// Optimized tool registry with minimal lock contention
pub struct OptimizedToolRegistry {
    /// Read-heavy tool metadata cache (rarely updated)
    tool_metadata: Arc<RwLock<HashMap<String, Arc<CachedToolMetadata>>>>,

    /// Execution semaphore to limit concurrent tool executions
    execution_semaphore: Arc<Semaphore>,

    /// Hot path LRU cache for frequently accessed tools.
    /// Uses an LRU cache so that stale entries are evicted when the
    /// capacity is reached, instead of silently refusing new entries.
    hot_cache: Arc<RwLock<lru::LruCache<String, Arc<CachedToolMetadata>>>>,

    /// Execution statistics (append-only for performance)
    execution_stats: Arc<RwLock<Vec<ToolExecutionRecord>>>,
}

/// Maximum number of entries in the hot-path tool metadata cache.
const HOT_CACHE_CAPACITY: usize = 16;

impl OptimizedToolRegistry {
    pub fn new(max_concurrent_tools: usize) -> Self {
        Self {
            tool_metadata: Arc::new(RwLock::new(HashMap::with_capacity(64))),
            execution_semaphore: Arc::new(Semaphore::new(max_concurrent_tools)),
            hot_cache: Arc::new(RwLock::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(HOT_CACHE_CAPACITY)
                    .unwrap_or(std::num::NonZeroUsize::MIN),
            ))),
            execution_stats: Arc::new(RwLock::new(Vec::with_capacity(1024))),
        }
    }

    /// Fast tool lookup with hot cache optimization
    pub fn get_tool_metadata(&self, tool_name: &str) -> Option<Arc<CachedToolMetadata>> {
        // Try hot cache first (most frequently used tools).
        // `get` on LRU promotes the entry to most-recently-used automatically.
        if let Some(metadata) = self.hot_cache.write().get(tool_name) {
            return Some(Arc::clone(metadata));
        }

        // Fallback to main cache
        let metadata = self.tool_metadata.read().get(tool_name).cloned()?;

        // Promote to hot cache
        self.promote_to_hot_cache(tool_name, &metadata);

        Some(metadata)
    }

    /// Register tool metadata with minimal locking
    pub fn register_tool(&self, metadata: CachedToolMetadata) {
        let tool_name = metadata.name.clone();
        let metadata_arc = Arc::new(metadata);

        self.tool_metadata.write().insert(tool_name, metadata_arc);
    }

    /// Execute tool with concurrency control and performance tracking
    pub async fn execute_tool_optimized(&self, tool_name: &str, _args: Value) -> Result<Value> {
        // Acquire execution permit
        let _permit = self.execution_semaphore.acquire().await?;

        let start_time = std::time::Instant::now();

        // Simulate tool execution (replace with actual implementation)
        let result = self.execute_tool_impl(tool_name, _args).await;

        let execution_time = start_time.elapsed();

        // Record execution statistics inline to keep behavior deterministic.
        self.record_execution_stats(tool_name, execution_time, result.is_ok());

        result
    }

    /// Promote frequently accessed tools to hot cache.
    /// The LRU cache automatically evicts the least-recently-used entry
    /// when capacity is reached.
    fn promote_to_hot_cache(&self, tool_name: &str, metadata: &Arc<CachedToolMetadata>) {
        self.hot_cache
            .write()
            .push(tool_name.to_string(), Arc::clone(metadata));
    }

    /// Record execution statistics with minimal blocking
    fn record_execution_stats(
        &self,
        tool_name: &str,
        execution_time: std::time::Duration,
        success: bool,
    ) {
        let record = ToolExecutionRecord {
            tool_name: tool_name.to_string(),
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
            attempt: 1,
            retry_after_ms: None,
            circuit_breaker_state: None,
        };

        let mut stats = self.execution_stats.write();
        // Prune stats to prevent memory leaks (KISS/DRY/Perf)
        if stats.len() >= 1024 {
            stats.drain(..128); // Remove oldest 128 entries
        }
        stats.push(record);
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
