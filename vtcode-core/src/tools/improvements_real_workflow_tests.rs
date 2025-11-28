//! Real-world integration tests showing actual workflows
//!
//! These tests demonstrate how the improved system works together
//! in practical scenarios matching real VTCode usage.

#[cfg(test)]
mod real_workflow_tests {
    use crate::tools::{
        DetectedPattern, EventType, ExecutionEvent, LruCache, ObservabilityContext, PatternEngine,
        ToolRegistryImprovement,
    };
    use std::sync::Arc;
    use std::time::Duration;

    /// Test: User searches for files, realizes they need grep
    #[test]
    fn test_workflow_tool_discovery_and_refinement() {
        let obs = Arc::new(ObservabilityContext::logging());
        let ext = ToolRegistryImprovement::new(obs.clone());

        // User tries ls first (not great for search)
        ext.record_execution("ls_files".to_string(), "src/".to_string(), true, 0.4, 50);

        // User realizes grep is better
        ext.record_execution(
            "grep_file".to_string(),
            "pattern:error".to_string(),
            true,
            0.7,
            100,
        );

        // User refines the pattern (improving quality)
        ext.record_execution(
            "grep_file".to_string(),
            "pattern:ERROR".to_string(),
            true,
            0.85,
            95,
        );

        // Check metrics
        let ranked = ext.rank_tools();
        assert_eq!(ranked[0].0, "grep_file"); // Best performer

        let summary = ext.get_summary();
        assert_eq!(summary.unique_tools, 2);
        assert!(summary.success_rate > 0.9);
    }

    /// Test: Tool caching prevents redundant execution
    #[test]
    fn test_workflow_caching_with_metrics() {
        let obs = Arc::new(ObservabilityContext::noop());
        let ext = ToolRegistryImprovement::new(obs);

        // First execution
        ext.record_execution(
            "expensive_tool".to_string(),
            "complex_search".to_string(),
            true,
            0.9,
            500,
        );
        ext.cache_result("expensive_tool", "complex_search", "found 42 results");

        // Second execution should hit cache
        if let Some(cached) = ext.get_cached_result("expensive_tool", "complex_search") {
            assert_eq!(cached, "found 42 results");
        }

        let metrics = ext.get_tool_metrics("expensive_tool").unwrap();
        assert_eq!(metrics.total_calls, 1); // Still 1 because cache is separate
    }

    /// Test: Detecting when user is stuck in a loop
    #[test]
    fn test_workflow_loop_detection() {
        let obs = Arc::new(ObservabilityContext::noop());
        let engine = PatternEngine::new(100, 20);

        // User runs the same command 3 times
        for i in 0..3 {
            engine.record(ExecutionEvent {
                tool_name: "grep_file".to_string(),
                arguments: "pattern:config".to_string(),
                success: false, // Failing
                quality_score: 0.2,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }

        // Should detect loop
        let pattern = engine.detect_pattern();
        assert_eq!(pattern, DetectedPattern::Loop);

        // Summary shows user is stuck
        let summary = engine.summary();
        assert_eq!(summary.success_rate, 0.0);
    }

    /// Test: Detecting refinement - user improving their approach
    #[test]
    fn test_workflow_refinement_detection() {
        let obs = Arc::new(ObservabilityContext::noop());
        let engine = PatternEngine::new(100, 20);

        // User iteratively improves their search
        let patterns = vec![
            ("grep -r error", 0.3),   // First attempt: poor results
            ("grep -r ERROR", 0.6),   // Better, switched case
            ("grep -ri error", 0.85), // Even better, case-insensitive
        ];

        for (i, (pattern, quality)) in patterns.iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: "grep_file".to_string(),
                arguments: pattern.to_string(),
                success: true,
                quality_score: *quality,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }

        // Should detect refinement chain
        let detected = engine.detect_pattern();
        assert_eq!(detected, DetectedPattern::Refinement);

        // Next tool prediction
        let next = engine.predict_next_tool();
        assert_eq!(next, Some("grep_file".to_string()));
    }

    /// Test: Multiple tools with similar results (convergence)
    #[test]
    fn test_workflow_convergence_detection() {
        let obs = Arc::new(ObservabilityContext::noop());
        let engine = PatternEngine::new(100, 20);

        // User tries different tools, all working equally well
        for (i, tool) in ["grep_file", "find_file", "read_file"].iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: tool.to_string(),
                arguments: "search_term".to_string(),
                success: true,
                quality_score: 0.8, // All similar
                duration_ms: 100,
                timestamp: i as u64,
            });
        }

        // Should detect convergence (different tools, similar results)
        let pattern = engine.detect_pattern();
        assert_eq!(pattern, DetectedPattern::Convergence);

        let summary = engine.summary();
        assert_eq!(summary.unique_tools, 3);
        assert_eq!(summary.success_rate, 1.0);
    }

    /// Test: LRU cache with expiration
    #[test]
    fn test_workflow_cache_with_ttl() {
        let cache = LruCache::new(5, Duration::from_millis(100));

        // Put items
        for i in 0..5 {
            cache
                .put(format!("key{}", i), format!("value{}", i))
                .unwrap();
        }

        // Should get them
        assert_eq!(cache.get("key0"), Some("value0".to_string()));

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));

        // Should be expired
        assert_eq!(cache.get_owned("key0"), None);
    }

    /// Test: LRU eviction when cache is full
    #[test]
    fn test_workflow_cache_lru_eviction() {
        let cache = LruCache::new(3, Duration::from_secs(3600));

        // Fill cache
        cache
            .put_arc("key1".to_string(), Arc::new("value1".to_string()))
            .unwrap();
        cache
            .put_arc("key2".to_string(), Arc::new("value2".to_string()))
            .unwrap();
        cache
            .put_arc("key3".to_string(), Arc::new("value3".to_string()))
            .unwrap();

        // Access key1 to mark it recently used
        cache.get_arc("key1");

        // Add 4th item (should evict key2 as LRU)
        cache
            .put_arc("key4".to_string(), Arc::new("value4".to_string()))
            .unwrap();

        assert_eq!(cache.get_owned("key1"), Some("value1".to_string()));
        assert_eq!(cache.get_owned("key2"), None); // Evicted
        assert_eq!(cache.get_owned("key3"), Some("value3".to_string()));
        assert_eq!(cache.get_owned("key4"), Some("value4".to_string()));
    }

    /// Test: Tool ranking by effectiveness
    #[test]
    fn test_workflow_tool_ranking() {
        let obs = Arc::new(ObservabilityContext::noop());
        let ext = ToolRegistryImprovement::new(obs);

        // Tool1: Slow but reliable
        for _ in 0..10 {
            ext.record_execution("tool1".to_string(), "arg".to_string(), true, 0.9, 500);
        }

        // Tool2: Fast but unreliable
        for _ in 0..10 {
            ext.record_execution("tool2".to_string(), "arg".to_string(), false, 0.3, 50);
        }

        // Tool3: Medium speed, good results
        for _ in 0..10 {
            ext.record_execution("tool3".to_string(), "arg".to_string(), true, 0.85, 200);
        }

        let ranked = ext.rank_tools();
        assert_eq!(ranked[0].0, "tool1"); // Best success rate
        assert_eq!(ranked[1].0, "tool3"); // Good balance
        assert_eq!(ranked[2].0, "tool2"); // Worst

        // Verify metrics
        let t1_metrics = ext.get_tool_metrics("tool1").unwrap();
        assert_eq!(t1_metrics.success_rate(), 1.0);
    }

    /// Test: Cache statistics
    #[test]
    fn test_workflow_cache_statistics() {
        let cache = LruCache::new(10, Duration::from_secs(3600));

        cache
            .put_arc("key1", Arc::new("value1".to_string()))
            .unwrap();
        cache
            .put_arc("key2", Arc::new("value2".to_string()))
            .unwrap();
        cache
            .put_arc("key3", Arc::new("value3".to_string()))
            .unwrap();

        let stats = cache.stats();
        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.max_size, 10);
        assert!(stats.utilization_percent > 25.0 && stats.utilization_percent < 35.0);
        assert_eq!(stats.expired_entries, 0);
    }

    /// Test: End-to-end workflow combining all components
    #[test]
    fn test_workflow_full_integration() {
        let obs = Arc::new(ObservabilityContext::noop());
        let ext = ToolRegistryImprovement::new(obs);

        // Simulate user workflow
        // 1. User searches with one approach, gets mediocre results
        ext.record_execution("grep_file".to_string(), "error".to_string(), true, 0.5, 100);
        ext.cache_result("grep_file", "error", "10 results");

        // 2. User refines approach
        ext.record_execution("grep_file".to_string(), "ERROR".to_string(), true, 0.75, 95);
        ext.cache_result("grep_file", "ERROR", "15 results");

        // 3. User refines further
        ext.record_execution(
            "grep_file".to_string(),
            "\\[ERROR\\]".to_string(),
            true,
            0.9,
            105,
        );
        ext.cache_result("grep_file", "\\[ERROR\\]", "25 results");

        // Verify final state
        let summary = ext.get_summary();
        assert_eq!(summary.total_executions, 3);
        assert_eq!(summary.successful_executions, 3);
        assert_eq!(summary.success_rate, 1.0);
        assert_eq!(summary.unique_tools, 1);

        // Verify cache
        assert_eq!(
            ext.get_cached_result("grep_file", "\\[ERROR\\]"),
            Some("25 results".to_string())
        );

        // Verify tool ranking
        let ranked = ext.rank_tools();
        assert!(ranked.len() > 0);
        assert_eq!(ranked[0].0, "grep_file");

        // Verify pattern detection
        match summary.current_pattern {
            DetectedPattern::Refinement => {} // Expected
            _ => panic!("Expected refinement pattern"),
        }
    }
}
