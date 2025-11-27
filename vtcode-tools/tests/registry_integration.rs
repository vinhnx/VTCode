//! Integration test: Real ToolRegistry with cache + middleware.
//!
//! Verifies end-to-end workflows using actual ToolRegistry.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;
    use vtcode_tools::{cache::LruCache, middleware::*, patterns::PatternDetector};

    #[tokio::test]
    async fn test_cache_effectiveness_with_same_args() -> anyhow::Result<()> {
        let cache: Arc<LruCache<String>> = Arc::new(LruCache::new(10, Duration::from_secs(60)));

        // Simulate two identical tool calls
        let tool_name = "list_files";
        let args_json = r#"{"path": "/tmp"}"#;

        // First call - miss
        let cache_key = format!("{}:{}", tool_name, args_json);
        assert_eq!(cache.get(&cache_key).await, None);

        // Cache result
        cache
            .insert(cache_key.clone(), "file1.txt\nfile2.txt".into())
            .await;

        // Second call - hit
        assert_eq!(
            cache.get_owned(&cache_key).await,
            Some("file1.txt\nfile2.txt".to_string())
        );

        let stats = cache.stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_middleware_metrics() -> anyhow::Result<()> {
        let metrics = MetricsMiddleware::new();
        let chain = MiddlewareChain::new().add(metrics.clone());

        let req = ToolRequest {
            tool_name: "test_tool".to_string(),
            args: Arc::new(serde_json::json!({})),
            metadata: Default::default(),
        };

        // Simulate 5 tool executions
        for i in 0..5 {
            chain.before_execute(&req).await?;

            let res = ToolResponse {
                result: Arc::new(serde_json::json!({"result": i})),
                duration_ms: 10 + i as u64,
                cache_hit: i % 2 == 0, // Alternate cache hits
            };

            chain.after_execute(&req, &res).await?;
        }

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.total_calls, 5);
        assert_eq!(snapshot.successful_calls, 5);
        assert_eq!(snapshot.cache_hits, 3); // 3 out of 5 were cache hits

        Ok(())
    }

    #[tokio::test]
    async fn test_pattern_detection_workflow() -> anyhow::Result<()> {
        let mut detector = PatternDetector::new(2);
        let now = std::time::Instant::now();

        // Simulate realistic workflow: find -> grep -> find -> grep
        let tools = vec!["find_files", "grep_file", "find_files", "grep_file"];

        for tool in tools {
            detector.record_event(vtcode_tools::ToolEvent {
                tool_name: tool.to_string(),
                success: true,
                duration_ms: 50,
                timestamp: now,
            });
        }

        let patterns = detector.patterns();
        assert!(!patterns.is_empty());

        // Should detect (find, grep) pattern
        let has_find_grep = patterns.iter().any(|p| {
            p.sequence.len() == 2 && p.sequence[0] == "find_files" && p.sequence[1] == "grep_file"
        });

        assert!(has_find_grep, "Should detect find->grep pattern");

        Ok(())
    }

    #[tokio::test]
    async fn test_full_workflow_simulation() -> anyhow::Result<()> {
        let cache = Arc::new(LruCache::new(100, Duration::from_secs(60)));
        let metrics = MetricsMiddleware::new();
        let chain = MiddlewareChain::new().add(metrics.clone());

        let mut detector = PatternDetector::new(2);
        let now = std::time::Instant::now();

        // Simulate a realistic development workflow:
        // 1. list_files
        // 2. grep_file (same as previous)
        // 3. list_files (same as #1)
        // 4. grep_file (same as #2)

        let workflows = vec![
            ("list_files", r#"{"path": "/src"}"#, true),
            ("grep_file", r#"{"pattern": "fn main"}"#, true),
            ("list_files", r#"{"path": "/src"}"#, true), // Cache hit
            ("grep_file", r#"{"pattern": "fn main"}"#, true), // Cache hit
        ];

        for (tool, args, _success) in workflows {
            let cache_key = format!("{}:{}", tool, args);

            // Try cache
            let is_cached = cache.get(&cache_key).await.is_some();

            if !is_cached {
                // Simulate execution and cache
                cache
                    .insert(cache_key, format!("result from {}", tool))
                    .await;
            }

            // Record in pattern detector
            detector.record_event(vtcode_tools::ToolEvent {
                tool_name: tool.to_string(),
                success: true,
                duration_ms: 50,
                timestamp: now,
            });

            // Update metrics
            let req = ToolRequest {
                tool_name: tool.to_string(),
                args: Arc::new(serde_json::json!(args)),
                metadata: Default::default(),
            };

            chain.before_execute(&req).await?;
            let res = ToolResponse {
                result: Arc::new(serde_json::json!({"status": "ok"})),
                duration_ms: 50,
                cache_hit: is_cached,
            };
            chain.after_execute(&req, &res).await?;
        }

        // Verify cache effectiveness
        let cache_stats = cache.stats().await;
        assert!(cache_stats.hits > 0, "Should have cache hits");

        // Verify metrics
        let metrics_snapshot = metrics.snapshot().await;
        assert_eq!(metrics_snapshot.total_calls, 4);
        assert!(metrics_snapshot.cache_hits > 0);

        // Verify patterns detected
        let patterns = detector.patterns();
        assert!(!patterns.is_empty(), "Should detect workflow patterns");

        Ok(())
    }

    #[tokio::test]
    async fn test_cache_ttl_enforcement() -> anyhow::Result<()> {
        let cache: LruCache<String> = LruCache::new(10, Duration::from_millis(100));

        cache.insert("key".into(), "value".into()).await;
        assert_eq!(cache.get_owned("key").await, Some("value".to_string()));

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should return None due to expiration
        assert_eq!(cache.get("key").await, None);

        let stats = cache.stats().await;
        assert!(stats.expirations > 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_cache_lru_eviction() -> anyhow::Result<()> {
        let cache: LruCache<i32> = LruCache::new(3, Duration::from_secs(60));

        // Fill cache
        cache.insert("a".into(), 1).await;
        cache.insert("b".into(), 2).await;
        cache.insert("c".into(), 3).await;

        // Access 'a' to mark as recently used
        let _ = cache.get("a").await;

        // Insert new item - should evict 'b' (least recently used)
        cache.insert("d".into(), 4).await;

        assert_eq!(cache.get("a").await.map(|v| *v), Some(1));
        assert_eq!(cache.get("b").await, None); // Evicted
        assert_eq!(cache.get("c").await.map(|v| *v), Some(3));
        assert_eq!(cache.get("d").await.map(|v| *v), Some(4));

        Ok(())
    }
}
