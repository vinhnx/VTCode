//! Example: Cache + Middleware integration for real tool workflows.
//!
//! Demonstrates:
//! - LRU cache with TTL
//! - Middleware chain for observability
//! - Metrics tracking
//! - Real tool execution pattern

use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::Arc;
use std::time::Duration;
use vtcode_tools::{ToolEvent, cache::LruCache, middleware::*, patterns::PatternDetector};

/// Simulates a tool executor that uses cache and middleware.
struct CachedToolExecutor {
    cache: Arc<LruCache<serde_json::Value>>,
    middleware: MiddlewareChain,
}

impl CachedToolExecutor {
    fn new() -> Self {
        let cache = Arc::new(LruCache::new(100, Duration::from_secs(60)));
        let metrics = MetricsMiddleware::new();
        let logging = LoggingMiddleware::new("tool-executor");

        let middleware = MiddlewareChain::new().add(logging).add(metrics);

        Self { cache, middleware }
    }

    /// Execute a tool with caching and middleware.
    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        // Use a stable, hashed key that avoids creating a large string for the args
        let cache_key = {
            let mut hasher = DefaultHasher::new();
            if let Ok(bytes) = serde_json::to_vec(&args) {
                hasher.write(&bytes);
            }
            format!("{}:{}", tool_name, hasher.finish())
        };
        let start = std::time::Instant::now();

        let args = Arc::new(args);
        let req = ToolRequest {
            tool_name: tool_name.to_string(),
            args: Arc::clone(&args),
            metadata: Default::default(),
        };

        // Before hooks
        self.middleware.before_execute(&req).await?;

        // Try cache
        if let Some(cached_result) = self.cache.get(&cache_key).await {
            eprintln!("✓ Cache hit for {}", tool_name);

            let res = ToolResponse {
                result: Arc::clone(&cached_result),
                duration_ms: start.elapsed().as_millis() as u64,
                cache_hit: true,
            };
            self.middleware.after_execute(&req, &res).await?;
            return Ok((*cached_result).clone());
        }

        // Simulate tool execution
        let result = simulate_tool_execution(tool_name, &req.args).await?;

        // Cache result using an Arc to avoid extra clone costs
        self.cache
            .insert_arc(cache_key, Arc::new(result.clone()))
            .await;

        let res = ToolResponse {
            result: Arc::new(result.clone()),
            duration_ms: start.elapsed().as_millis() as u64,
            cache_hit: false,
        };

        // After hooks
        self.middleware.after_execute(&req, &res).await?;

        Ok(result)
    }
}

/// Simulate tool execution with realistic delays.
async fn simulate_tool_execution(
    tool_name: &str,
    _args: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    // Simulate different execution times
    let delay = match tool_name {
        "fast_tool" => Duration::from_millis(10),
        "slow_tool" => Duration::from_millis(200),
        _ => Duration::from_millis(50),
    };

    tokio::time::sleep(delay).await;

    Ok(json!({
        "tool": tool_name,
        "status": "success",
        "data": format!("Result from {}", tool_name),
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Cache + Middleware Integration Example ===\n");

    let executor = CachedToolExecutor::new();

    println!("1. Testing cache effectiveness:\n");

    // First call - cache miss
    println!("  First call to 'list_files':");
    let result1 = executor
        .execute("list_files", json!({"path": "/tmp"}))
        .await?;
    println!("  Result: {}\n", result1);

    // Second call - cache hit
    println!("  Second call to 'list_files' (same args):");
    let result2 = executor
        .execute("list_files", json!({"path": "/tmp"}))
        .await?;
    println!("  Result: {}\n", result2);

    // Different args - cache miss
    println!("  Third call to 'list_files' (different args):");
    let result3 = executor
        .execute("list_files", json!({"path": "/var"}))
        .await?;
    println!("  Result: {}\n", result3);

    println!("\n2. Testing multiple tool calls:\n");

    // Simulate a workflow
    executor
        .execute("find_files", json!({"pattern": "*.rs"}))
        .await?;
    executor
        .execute("find_files", json!({"pattern": "*.rs"}))
        .await?; // Cache hit
    executor
        .execute("grep_file", json!({"pattern": "fn main"}))
        .await?;
    executor
        .execute("grep_file", json!({"pattern": "fn main"}))
        .await?; // Cache hit

    println!("\n3. Pattern detection:\n");

    let mut detector = PatternDetector::new(2);
    let now = std::time::Instant::now();

    // Record a realistic workflow: find -> grep -> find -> grep
    let workflows = vec![
        ("find_files", true, 50),
        ("grep_file", true, 100),
        ("find_files", true, 50),
        ("grep_file", true, 100),
        ("find_files", true, 55),
        ("grep_file", true, 105),
    ];

    for (tool, success, duration) in workflows {
        detector.record_event(ToolEvent {
            tool_name: tool.to_string(),
            success,
            duration_ms: duration,
            timestamp: now,
        });
    }

    let patterns = detector.patterns();
    println!("  Detected {} patterns:", patterns.len());
    for pattern in patterns.iter().take(3) {
        println!("    - Sequence: {:?}", pattern.sequence);
        println!(
            "      Frequency: {}, Success Rate: {:.1}%",
            pattern.frequency,
            pattern.success_rate * 100.0
        );
    }

    println!("\n4. Feature vector (ML-ready):\n");
    let features = detector.feature_vector();
    println!("  Features: {:?}", features);
    println!("  Feature meanings:");
    println!("    [0] Event count: {}", features[0] as u64);
    println!("    [1] Success rate: {:.1}%", features[1] * 100.0);
    println!("    [2] Avg duration: {:.1}ms", features[2]);
    println!("    [3] Tool diversity: {:.1}", features[3]);
    println!("    [4] Pattern density: {:.1}%", features[4] * 100.0);

    println!("\n5. Cache statistics:\n");
    let stats = executor.cache.stats().await;
    println!("  Hits: {}", stats.hits);
    println!("  Misses: {}", stats.misses);
    println!("  Hit rate: {:.1}%", stats.hit_rate());
    println!("  Evictions: {}", stats.evictions);

    Ok(())
}
