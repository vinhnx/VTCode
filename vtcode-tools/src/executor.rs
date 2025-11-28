//! Production ToolExecutor: Cache + Middleware + Patterns integrated.
//!
//! Drop-in replacement for tool execution with full observability.

use crate::cache::LruCache;
use crate::middleware::{MiddlewareChain, ToolRequest, ToolResponse};
use crate::patterns::{PatternDetector, ToolEvent};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Thread-safe snapshot of executor state.
#[derive(Clone, Debug)]
pub struct ExecutorStats {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub avg_duration_ms: u64,
    pub patterns_detected: usize,
}

/// Production tool executor with cache, middleware, and pattern detection.
pub struct CachedToolExecutor {
    /// Response cache (key = "tool_name:args_json")
    cache: Arc<LruCache<Value>>,
    /// Composable middleware chain
    middleware: MiddlewareChain,
    /// Pattern detector for workflow analysis
    patterns: Arc<RwLock<PatternDetector>>,
    /// Internal stats tracking
    stats: Arc<RwLock<ExecutorStats>>,
}

impl CachedToolExecutor {
    /// Create a new executor with default settings.
    ///
    /// - Cache capacity: 1000 entries
    /// - Cache TTL: 1 hour
    /// - Pattern window: 3-tool sequences
    pub fn new() -> Self {
        Self::with_config(1000, Duration::from_secs(3600), 3)
    }

    /// Create executor with custom cache and pattern settings.
    pub fn with_config(cache_capacity: usize, cache_ttl: Duration, pattern_window: usize) -> Self {
        let cache = Arc::new(LruCache::<Value>::new(cache_capacity, cache_ttl));
        let middleware = MiddlewareChain::new();
        let patterns = Arc::new(RwLock::new(PatternDetector::new(pattern_window)));
        let stats = Arc::new(RwLock::new(ExecutorStats {
            total_calls: 0,
            successful_calls: 0,
            failed_calls: 0,
            cache_hits: 0,
            cache_misses: 0,
            avg_duration_ms: 0,
            patterns_detected: 0,
        }));

        Self {
            cache,
            middleware,
            patterns,
            stats,
        }
    }

    /// Add middleware to the chain.
    pub fn with_middleware(mut self, mw: Arc<dyn crate::middleware::Middleware>) -> Self {
        self.middleware = self.middleware.add(mw);
        self
    }

    /// Execute a tool with full caching and observability.
    pub async fn execute(&self, tool_name: &str, args: Value) -> anyhow::Result<Value> {
        // Delegate to the shared-version and convert to an owned Value
        let r = self.execute_shared_owned(tool_name, args).await?;
        Ok((*r).clone())
    }

    /// Execute a tool but return a shared (Arc) response to avoid clones.
    /// Accepts a shared Arc<Value> to avoid cloning arg contents when caller
    /// already holds a shared reference.
    pub async fn execute_shared(
        &self,
        tool_name: &str,
        args: Arc<Value>,
    ) -> anyhow::Result<Arc<Value>> {
        let start = std::time::Instant::now();
        let cache_key = make_cache_key(tool_name, &*args);

        // Update stats (single write)
        {
            let mut stats = self.stats.write().await;
            stats.total_calls += 1;
        }

        // Create request — reuse the shared `args` (already an Arc)
        let owned_args = Arc::clone(&args);
        let req = ToolRequest {
            tool_name: tool_name.to_string(),
            args: Arc::clone(&owned_args),
            metadata: Default::default(),
        };

        // Before hooks
        self.middleware.before_execute(&req).await?;

        // Check cache
        if let Some(result) = self.cache.get(&cache_key).await {
            let duration_ms = start.elapsed().as_millis() as u64;
            let mut stats = self.stats.write().await;
            stats.successful_calls += 1;
            stats.cache_hits += 1;

            let res = ToolResponse {
                result: Arc::clone(&result),
                duration_ms,
                cache_hit: true,
            };
            self.middleware.after_execute(&req, &res).await?;

            // Record pattern
            self.record_pattern(tool_name, true, duration_ms).await;

            return Ok(Arc::clone(&result));
        }

        // Update stats: cache miss
        {
            let mut stats = self.stats.write().await;
            stats.cache_misses += 1;
        }

        // Execute tool (caller provides actual execution)
        // This is where your tool registry would call the actual tool
        let result = self.execute_tool_internal(tool_name, &*owned_args).await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Wrap result in Arc once, then clone Arc for cache and response
        let arc_res = Arc::new(result);

        // Cache result (Arc clone is cheap - pass Arc directly into cache)
        self.cache.insert_arc(cache_key, Arc::clone(&arc_res)).await;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.successful_calls += 1;
            stats.avg_duration_ms = (stats.avg_duration_ms + duration_ms) / 2;
        }

        let res = ToolResponse {
            result: Arc::clone(&arc_res),
            duration_ms,
            cache_hit: false,
        };

        // After hooks
        self.middleware.after_execute(&req, &res).await?;

        // Record pattern
        self.record_pattern(tool_name, true, duration_ms).await;

        Ok(arc_res)
    }

    /// Backwards-compatible wrapper for callers that still pass an owned Value.
    pub async fn execute_shared_owned(
        &self,
        tool_name: &str,
        args: Value,
    ) -> anyhow::Result<Arc<Value>> {
        let arg = Arc::new(args);
        self.execute_shared(tool_name, arg).await
    }

    /// Execute tool (override this for real tool execution)
    async fn execute_tool_internal(
        &self,
        _tool_name: &str,
        _args: &Value,
    ) -> anyhow::Result<Value> {
        // Default: return placeholder result
        // In real usage, this would call ToolRegistry
        Ok(serde_json::json!({"status": "ok"}))
    }

    /// Record event in pattern detector
    async fn record_pattern(&self, tool_name: &str, success: bool, duration_ms: u64) {
        let mut patterns = self.patterns.write().await;
        patterns.record_event(ToolEvent {
            tool_name: tool_name.to_string(),
            success,
            duration_ms,
            timestamp: std::time::Instant::now(),
        });
    }

    /// Get current executor statistics.
    pub async fn stats(&self) -> ExecutorStats {
        let mut stats = self.stats.read().await.clone();
        let patterns = self.patterns.read().await;
        stats.patterns_detected = patterns.patterns().len();
        stats
    }

    /// Get cache statistics.
    pub async fn cache_stats(&self) -> crate::cache::CacheStats {
        self.cache.stats().await
    }

    /// Get detected workflow patterns.
    pub async fn patterns(&self) -> Vec<crate::patterns::DetectedPattern> {
        let patterns = self.patterns.read().await;
        patterns.patterns()
    }

    /// Get ML-ready feature vector from patterns.
    pub async fn feature_vector(&self) -> Vec<f64> {
        let patterns = self.patterns.read().await;
        patterns.feature_vector()
    }

    /// Clear cache
    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    /// Clear patterns
    pub async fn clear_patterns(&self) {
        let mut patterns = self.patterns.write().await;
        patterns.reset();
    }

    /// Print execution report
    pub async fn report(&self) {
        let stats = self.stats().await;
        let cache_stats = self.cache_stats().await;
        let patterns = self.patterns().await;

        println!("\n=== ToolExecutor Report ===\n");

        println!("Execution Statistics:");
        println!("  Total calls:      {}", stats.total_calls);
        println!("  Successful:       {}", stats.successful_calls);
        println!("  Failed:           {}", stats.failed_calls);
        println!("  Avg duration:     {}ms", stats.avg_duration_ms);

        println!("\nCache Performance:");
        println!("  Hits:             {}", cache_stats.hits);
        println!("  Misses:           {}", cache_stats.misses);
        println!("  Hit rate:         {:.1}%", cache_stats.hit_rate());
        println!("  Evictions:        {}", cache_stats.evictions);
        println!("  Expirations:      {}", cache_stats.expirations);

        println!("\nWorkflow Patterns ({} detected):", patterns.len());
        for (i, pattern) in patterns.iter().take(5).enumerate() {
            println!("  {}. {:?}", i + 1, pattern.sequence);
            println!(
                "     Frequency: {}, Confidence: {:.1}%",
                pattern.frequency,
                pattern.confidence * 100.0
            );
        }

        println!("\n");
    }
}

// Helper for stable cache key generation — uses a small 64-bit hash of the
// serialized JSON arguments to avoid storing large argument strings as cache
// keys while still differentiating distinct argument payloads.
#[inline]
fn make_cache_key(tool_name: &str, args: &Value) -> String {
    use std::hash::Hash;
    let mut hasher = DefaultHasher::new();
    // Hash the tool name first for better distribution.
    tool_name.hash(&mut hasher);
    // Use compact JSON serialization for hashing.
    if let Ok(bytes) = serde_json::to_vec(args) {
        hasher.write(&bytes);
    }
    let h = hasher.finish();
    format!("{}:{:x}", tool_name, h)
}

// test above added to the test module further below

impl Default for CachedToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::LoggingMiddleware;

    #[tokio::test]
    async fn test_executor_basic() -> anyhow::Result<()> {
        let executor = CachedToolExecutor::new();

        // First call - miss
        let result = executor
            .execute("test_tool", serde_json::json!({"arg": 1}))
            .await?;
        assert_eq!(result, serde_json::json!({"status": "ok"}));

        let stats = executor.stats().await;
        assert_eq!(stats.total_calls, 1);
        assert_eq!(stats.cache_misses, 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_executor_cache_hit() -> anyhow::Result<()> {
        let executor = CachedToolExecutor::new();

        // Two identical calls
        executor
            .execute("test_tool", serde_json::json!({"arg": 1}))
            .await?;
        executor
            .execute("test_tool", serde_json::json!({"arg": 1}))
            .await?;

        let stats = executor.stats().await;
        assert_eq!(stats.total_calls, 2);
        assert_eq!(stats.successful_calls, 2);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);

        Ok(())
    }

    #[tokio::test]
    async fn cache_hit_after_repeat_call() {
        let exec = CachedToolExecutor::with_config(10, Duration::from_secs(60), 3);
        let args = serde_json::json!({"x": 1});

        // First call -> cache miss
        let _first = exec.execute("test_tool", args.clone()).await.unwrap();

        // Second call with same args -> should use cache
        let _second = exec.execute("test_tool", args.clone()).await.unwrap();

        // Check stats for cache hit/miss
        let stats = exec.stats().await;
        assert!(stats.cache_hits >= 1);
        assert!(stats.cache_misses >= 1);
    }

    #[tokio::test]
    async fn test_executor_with_middleware() -> anyhow::Result<()> {
        let executor = CachedToolExecutor::new().with_middleware(LoggingMiddleware::new("test"));

        executor.execute("test_tool", serde_json::json!({})).await?;

        let stats = executor.stats().await;
        assert_eq!(stats.total_calls, 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_executor_patterns() -> anyhow::Result<()> {
        let executor = CachedToolExecutor::new();

        // Record pattern: A -> B -> A -> B
        executor.execute("tool_a", serde_json::json!({})).await?;
        executor.execute("tool_b", serde_json::json!({})).await?;
        executor.execute("tool_a", serde_json::json!({})).await?;
        executor.execute("tool_b", serde_json::json!({})).await?;

        let patterns = executor.patterns().await;
        assert!(!patterns.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_executor_clear() -> anyhow::Result<()> {
        let executor = CachedToolExecutor::new();

        executor.execute("test", serde_json::json!({})).await?;

        let stats_before = executor.stats().await;
        assert_eq!(stats_before.total_calls, 1);

        executor.clear_cache().await;
        executor.clear_patterns().await;

        let cache_stats = executor.cache_stats().await;
        assert_eq!(cache_stats.hits + cache_stats.misses, 0);

        Ok(())
    }
}
