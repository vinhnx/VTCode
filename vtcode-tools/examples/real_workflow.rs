//! Real-world example: Code search + edit workflow with caching and metrics.
//!
//! Simulates a developer workflow:
//! 1. Find files matching pattern
//! 2. Search for specific code in those files
//! 3. Cache results for reuse
//! 4. Track metrics and detect workflow patterns

use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::Arc;
use std::time::Duration;
use vtcode_tools::{
    cache::LruCache,
    middleware::{
        LoggingMiddleware, MetricsMiddleware, MiddlewareChain, ToolRequest, ToolResponse,
    },
    patterns::{PatternDetector, ToolEvent},
};

/// Production-like tool executor.
struct ToolExecutor {
    cache: Arc<LruCache<Vec<String>>>,
    metrics: Arc<MetricsMiddleware>,
    pattern_detector: Arc<tokio::sync::RwLock<PatternDetector>>,
}

impl ToolExecutor {
    fn new() -> Self {
        let cache = Arc::new(LruCache::new(1000, Duration::from_secs(3600)));
        let metrics = MetricsMiddleware::new();
        let pattern_detector = Arc::new(tokio::sync::RwLock::new(PatternDetector::new(3)));

        Self {
            cache,
            metrics,
            pattern_detector,
        }
    }

    /// Execute a tool call with full observability.
    async fn execute(&self, tool_name: &str, args: &str) -> Vec<String> {
        let start = std::time::Instant::now();
        // Small deterministic hashed key to avoid large, repeated string allocations
        let cache_key = {
            let mut hasher = DefaultHasher::new();
            hasher.write(args.as_bytes());
            format!("{}:{}", tool_name, hasher.finish())
        };

        let middleware = MiddlewareChain::new()
            .add(LoggingMiddleware::new("workflow"))
            .add(self.metrics.clone());

        let request_id = format!("workflow-{}", tool_name);
        let args_json = serde_json::json!({"query": args});
        let req = ToolRequest {
            id: request_id.clone(),
            tool_name: tool_name.to_string(),
            args: args_json,
            metadata: Some(Default::default()),
        };

        let _ = middleware.before_execute(&req).await;

        // Check cache first (get returns Arc<Vec<String>>)
        let result = if let Some(cached) = self.cache.get(&cache_key).await {
            (*cached).clone()
        } else {
            // Simulate tool execution
            let result = match tool_name {
                "find_files" => simulate_find(args),
                "grep_code" => simulate_grep(args),
                "list_files" => simulate_list(args),
                _ => vec!["unknown_tool".into()],
            };

            // Cache the result using insert_arc to avoid cloning the vector for cache
            self.cache
                .insert_arc(cache_key.clone(), Arc::new(result.clone()))
                .await;
            result
        };

        let duration = start.elapsed().as_millis() as u64;
        let was_cached = self.cache.get(&cache_key).await.is_some();
        let res = ToolResponse {
            id: request_id,
            success: true,
            result: Some(json!({"files": result})),
            error: None,
            duration_ms: Some(duration),
            cache_hit: Some(was_cached),
        };

        let _ = middleware.after_execute(&req, &res).await;

        // Record in pattern detector
        let success = !result.is_empty();
        let mut detector = self.pattern_detector.write().await;
        detector.record_event(ToolEvent {
            tool_name: tool_name.to_string(),
            success,
            duration_ms: duration,
            timestamp: start,
        });

        result
    }
}

fn simulate_find(pattern: &str) -> Vec<String> {
    match pattern {
        "*.rs" => vec!["main.rs".into(), "lib.rs".into(), "utils.rs".into()],
        "*.toml" => vec!["Cargo.toml".into()],
        _ => vec!["file.txt".into()],
    }
}

fn simulate_grep(pattern: &str) -> Vec<String> {
    match pattern {
        "fn main" => vec!["main.rs:42".into(), "main.rs:105".into()],
        "pub struct" => vec!["lib.rs:10".into(), "lib.rs:67".into(), "utils.rs:3".into()],
        _ => vec!["match.txt".into()],
    }
}

fn simulate_list(path: &str) -> Vec<String> {
    match path {
        "/src" => vec!["main.rs".into(), "lib.rs".into(), "utils.rs".into()],
        "/tests" => vec!["integration.rs".into(), "unit.rs".into()],
        _ => vec!["file.txt".into()],
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Real-World Tool Workflow Example ===\n");

    let executor = ToolExecutor::new();

    println!("Scenario: Code search and analysis workflow\n");
    println!("1. Finding Rust files...");
    let rust_files = executor.execute("find_files", "*.rs").await;
    println!("   Found: {:?}\n", rust_files);

    println!("2. Searching for main function...");
    let main_matches = executor.execute("grep_code", "fn main").await;
    println!("   Matches: {:?}\n", main_matches);

    println!("3. Searching again (should hit cache)...");
    let main_matches_again = executor.execute("grep_code", "fn main").await;
    println!("   Matches: {:?}\n", main_matches_again);

    println!("4. Searching for struct definitions...");
    let struct_matches = executor.execute("grep_code", "pub struct").await;
    println!("   Matches: {:?}\n", struct_matches);

    println!("5. Listing src directory...");
    let src_files = executor.execute("list_files", "/src").await;
    println!("   Files: {:?}\n", src_files);

    println!("6. Finding again (cache hit)...");
    let rust_files_again = executor.execute("find_files", "*.rs").await;
    println!("   Found: {:?}\n", rust_files_again);

    // Print metrics
    println!("\n=== Metrics ===\n");
    let metrics_snapshot = executor.metrics.snapshot().await;
    println!("Total tool calls:     {}", metrics_snapshot.total_calls);
    println!(
        "Successful calls:     {}",
        metrics_snapshot.successful_calls
    );
    println!("Cache hits:           {}", metrics_snapshot.cache_hits);
    println!(
        "Hit rate:             {:.1}%",
        (metrics_snapshot.cache_hits as f64 / metrics_snapshot.total_calls.max(1) as f64) * 100.0
    );
    println!(
        "Total duration:       {}ms",
        metrics_snapshot.total_duration_ms
    );

    // Print cache stats
    println!("\n=== Cache Statistics ===\n");
    let cache_stats = executor.cache.stats().await;
    println!("Cache hits:           {}", cache_stats.hits);
    println!("Cache misses:         {}", cache_stats.misses);
    println!("Hit rate:             {:.1}%", cache_stats.hit_rate());
    println!("Current size:         {}", executor.cache.len().await);

    // Print patterns
    println!("\n=== Detected Patterns ===\n");
    let detector = executor.pattern_detector.read().await;
    let patterns = detector.patterns();
    println!("Detected {} patterns:", patterns.len());
    for pattern in patterns.iter().take(5) {
        println!("  - Sequence: {:?}", pattern.sequence);
        println!("    Confidence: {:.1}%", pattern.confidence * 100.0);
        println!("    Frequency: {}", pattern.frequency);
    }

    println!("\n=== Feature Vector (ML Input) ===\n");
    let features = detector.feature_vector();
    println!("Event count:          {}", features[0] as u64);
    println!("Success rate:         {:.1}%", features[1] * 100.0);
    println!("Avg duration:         {:.1}ms", features[2]);
    println!("Tool diversity:       {:.1}", features[3]);
    println!("Pattern density:      {:.1}%", features[4] * 100.0);

    println!("\nWorkflow complete! This example demonstrates:");
    println!("  ✓ Cache effectiveness in real workflows");
    println!("  ✓ Middleware composition for observability");
    println!("  ✓ Pattern detection in tool sequences");
    println!("  ✓ ML-ready feature engineering");

    Ok(())
}
