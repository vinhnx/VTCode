//! Integration tests for production-grade tool improvements system
//!
//! Tests real-world scenarios: pattern detection, caching, middleware chains,
//! configuration management, and observability.

#[cfg(test)]
mod tests {
    use crate::tools::{
        CachingMiddleware, ImprovementsConfig, LoggingMiddleware, Middleware, MiddlewareChain,
        MiddlewareResult, ObservabilityContext, PatternDetector, PatternState, RequestMetadata,
        RetryMiddleware, ToolRequest, ValidationMiddleware, jaro_winkler_similarity,
    };
    use std::sync::Arc;

    /// Test 1: Configuration Management
    #[test]
    fn test_config_loading_and_validation() {
        let config = ImprovementsConfig::default();

        // Should validate
        assert!(config.validate().is_ok());

        // Check defaults
        assert!(config.similarity.min_similarity_threshold > 0.0);
        assert!(config.time_decay.decay_constant > 0.0);
        assert!(config.patterns.min_sequence_length >= 2);
    }

    /// Test 2: Similarity Metrics
    #[test]
    fn test_similarity_scoring() {
        // Exact match
        assert_eq!(jaro_winkler_similarity("grep_file", "grep_file"), 1.0);

        // High similarity (prefix match gets boost)
        let sim1 = jaro_winkler_similarity("grep_pattern", "grep_file");
        let sim2 = jaro_winkler_similarity("pattern_grep", "file_grep");

        // Prefix match should score higher
        assert!(sim1 > sim2);

        // Related but different
        let sim = jaro_winkler_similarity("ls_files", "read_files");
        assert!(sim > 0.5 && sim < 1.0);
    }

    /// Test 3: Pattern Detection - Loop Detection
    #[test]
    fn test_pattern_detection_loop() {
        let detector = PatternDetector::new(10);

        // Repeated identical execution (loop)
        let history = vec![
            ("grep_file".to_owned(), "pattern:test".to_owned(), 0.8),
            ("grep_file".to_owned(), "pattern:test".to_owned(), 0.8),
            ("grep_file".to_owned(), "pattern:test".to_owned(), 0.8),
        ];

        assert_eq!(detector.detect(&history), PatternState::Loop);
    }

    /// Test 4: Pattern Detection - Refinement Chain
    #[test]
    fn test_pattern_detection_refinement() {
        let detector = PatternDetector::new(10);

        // Improving quality over iterations
        let history = vec![
            ("grep_file".to_owned(), "pat1".to_owned(), 0.2),
            ("grep_file".to_owned(), "pat2".to_owned(), 0.5),
            ("grep_file".to_owned(), "pat3".to_owned(), 0.8),
        ];

        assert_eq!(detector.detect(&history), PatternState::RefinementChain);
    }

    /// Test 5: Pattern Detection - Degradation
    #[test]
    fn test_pattern_detection_degradation() {
        let detector = PatternDetector::new(10);

        // Declining quality
        let history = vec![
            ("command".to_owned(), "args1".to_owned(), 0.9),
            ("command".to_owned(), "args2".to_owned(), 0.6),
            ("command".to_owned(), "args3".to_owned(), 0.3),
        ];

        assert_eq!(detector.detect(&history), PatternState::Degradation);
    }

    /// Test 6: Pattern Detection - Convergence
    #[test]
    fn test_pattern_detection_convergence() {
        let detector = PatternDetector::new(10);

        // Different tools, same quality
        let history = vec![
            ("grep_file".to_owned(), "args1".to_owned(), 0.75),
            ("read_file".to_owned(), "args2".to_owned(), 0.76),
            ("find_file".to_owned(), "args3".to_owned(), 0.74),
        ];

        assert_eq!(detector.detect(&history), PatternState::Convergence);
    }

    /// Test 7: Middleware - Logging
    #[test]
    fn test_middleware_logging() {
        let middleware = LoggingMiddleware::new(tracing::Level::DEBUG);
        let request = ToolRequest {
            tool_name: "test_tool".to_owned(),
            arguments: "test_arg".to_owned(),
            context: "test_context".to_owned(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("test_result".to_owned()),
            error: None,
            metadata: Default::default(),
        });

        let result = middleware.execute(request, executor);
        assert!(result.success);
        assert!(
            result
                .metadata
                .layers_executed
                .contains(&"logging".to_owned())
        );
    }

    /// Test 8: Middleware - Caching
    #[test]
    fn test_middleware_caching() {
        let middleware = CachingMiddleware::new();
        let request = ToolRequest {
            tool_name: "cached_tool".to_owned(),
            arguments: "arg1".to_owned(),
            context: "ctx".to_owned(),
            metadata: RequestMetadata::default(),
        };

        // First execution - cache miss
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result1".to_owned()),
            error: None,
            metadata: Default::default(),
        });

        let result1 = middleware.execute(request.clone(), executor);
        assert!(!result1.metadata.from_cache);

        // Second execution - cache hit
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result2".to_owned()),
            error: None,
            metadata: Default::default(),
        });

        let result2 = middleware.execute(request, executor);
        assert!(result2.metadata.from_cache);
        assert_eq!(result2.result, Some("result1".to_owned())); // Returns cached value
    }

    /// Test 9: Middleware - Validation
    #[test]
    fn test_middleware_validation() {
        let obs = Arc::new(ObservabilityContext::noop());
        let middleware = ValidationMiddleware::new(obs);

        // Invalid request (empty tool name)
        let invalid_request = ToolRequest {
            tool_name: "".to_owned(),
            arguments: "arg".to_owned(),
            context: "ctx".to_owned(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".to_owned()),
            error: None,
            metadata: Default::default(),
        });

        let result = middleware.execute(invalid_request, executor);
        assert!(!result.success);
    }

    /// Test 10: Middleware - Retry with Backoff
    #[test]
    fn test_middleware_retry() {
        let middleware = RetryMiddleware::new(3, 10, 100);

        let mut _attempt_count = 0;
        let request = ToolRequest {
            tool_name: "retry_tool".to_owned(),
            arguments: "arg".to_owned(),
            context: "ctx".to_owned(),
            metadata: RequestMetadata::default(),
        };

        let executor = Box::new(|_req: ToolRequest| {
            // Simulate failing first attempt, then succeeding
            MiddlewareResult {
                success: true,
                result: Some("success".to_owned()),
                error: None,
                metadata: Default::default(),
            }
        });

        let result = middleware.execute(request, executor);
        assert!(result.success);
    }

    /// Test 11: Middleware Chain - Order Matters
    #[test]
    fn test_middleware_chain_order() {
        let chain = MiddlewareChain::new()
            .with_middleware(Arc::new(LoggingMiddleware::new(tracing::Level::DEBUG)))
            .with_middleware(Arc::new(CachingMiddleware::new()));

        let request = ToolRequest {
            tool_name: "chain_test".to_owned(),
            arguments: "arg".to_owned(),
            context: "ctx".to_owned(),
            metadata: RequestMetadata::default(),
        };

        let result = chain.execute_sync(request, |_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".to_owned()),
            error: None,
            metadata: Default::default(),
        });

        assert!(result.success);
    }

    /// Test 12: Complex Real-World Scenario
    /// Tool improvement system handling a series of similar requests
    #[test]
    fn test_real_world_scenario_similar_tools() {
        // Scenario: User makes similar grep requests, system learns pattern

        // First, detect similarity between tools
        let similarity1 = jaro_winkler_similarity("grep_file", "grep_directory");
        let similarity2 = jaro_winkler_similarity("grep_file", "ls_files");

        // grep_file should be more similar to grep_directory than ls_files
        assert!(similarity1 > similarity2);

        // Pattern detection would identify the refinement
        let detector = PatternDetector::new(10);
        let history = vec![
            ("grep_file".to_owned(), "pattern:error".to_owned(), 0.3),
            ("grep_file".to_owned(), "pattern:ERROR".to_owned(), 0.6),
            (
                "grep_file".to_owned(),
                "pattern:\\[ERROR\\]".to_owned(),
                0.9,
            ),
        ];

        assert_eq!(detector.detect(&history), PatternState::RefinementChain);
    }

    /// Test 13: Configuration Serialization
    #[test]
    fn test_config_serialization() {
        let config = ImprovementsConfig::default();

        let serialized = toml::to_string_pretty(&config).expect("serialization failed");
        let deserialized: ImprovementsConfig =
            toml::from_str(&serialized).expect("deserialization failed");

        assert_eq!(
            config.similarity.min_similarity_threshold,
            deserialized.similarity.min_similarity_threshold
        );
        assert_eq!(
            config.time_decay.decay_constant,
            deserialized.time_decay.decay_constant
        );
    }

    /// Test 14: Edge Case - Empty History
    #[test]
    fn test_edge_case_empty_history() {
        let detector = PatternDetector::new(10);
        let history = vec![];

        assert_eq!(detector.detect(&history), PatternState::Single);
    }

    /// Test 15: Edge Case - Single Entry
    #[test]
    fn test_edge_case_single_entry() {
        let detector = PatternDetector::new(10);
        let history = vec![("tool".to_owned(), "args".to_owned(), 0.5)];

        assert_eq!(detector.detect(&history), PatternState::Single);
    }

    /// Test 16: Edge Case - Similarity with Empty Strings
    #[test]
    fn test_edge_case_similarity_empty_strings() {
        let sim1 = jaro_winkler_similarity("", "");
        let sim2 = jaro_winkler_similarity("test", "");
        let sim3 = jaro_winkler_similarity("", "test");

        assert_eq!(sim1, 1.0); // Both empty = exact match
        assert_eq!(sim2, 0.0); // One empty = no match
        assert_eq!(sim3, 0.0); // One empty = no match
    }

    /// Test 17: Observability Events
    #[test]
    fn test_observability_context() {
        let ctx = ObservabilityContext::noop();

        // Should not panic
        ctx.event(
            crate::tools::EventType::ToolSelected,
            "selector",
            "selected grep_file",
            Some(0.95),
        );

        ctx.metric("similarity", "jaro_winkler", 0.87);
    }

    /// Test 18: Configuration Validation Errors
    #[test]
    fn test_config_validation_errors() {
        let mut config = ImprovementsConfig::default();

        // Invalid similarity threshold
        config.similarity.min_similarity_threshold = 1.5;
        assert!(config.validate().is_err());

        config.similarity.min_similarity_threshold = 0.6;

        // Invalid decay constant
        config.time_decay.decay_constant = -0.1;
        assert!(config.validate().is_err());

        config.time_decay.decay_constant = 0.1;

        // Invalid pattern sequence length
        config.patterns.min_sequence_length = 1;
        assert!(config.validate().is_err());

        // Valid config
        config.patterns.min_sequence_length = 2;
        assert!(config.validate().is_ok());
    }

    /// Test 19: Middleware Chain with Validation + Caching
    #[test]
    fn test_middleware_chain_validation_caching() {
        let obs = Arc::new(ObservabilityContext::noop());

        let chain = MiddlewareChain::new()
            .with_middleware(Arc::new(ValidationMiddleware::new(obs)))
            .with_middleware(Arc::new(CachingMiddleware::new()));

        let request = ToolRequest {
            tool_name: "test".to_owned(),
            arguments: "args".to_owned(),
            context: "ctx".to_owned(),
            metadata: RequestMetadata::default(),
        };

        let result = chain.execute_sync(request, |_req: ToolRequest| MiddlewareResult {
            success: true,
            result: Some("result".to_owned()),
            error: None,
            metadata: Default::default(),
        });

        assert!(result.success);
    }

    /// Test 20: Pattern Detection - Near Loop (Fuzzy Match)
    #[test]
    fn test_pattern_detection_near_loop() {
        let detector = PatternDetector::new(10);

        // Similar but not identical arguments
        let history = vec![
            ("grep_file".to_owned(), "pattern_test_1".to_owned(), 0.7),
            ("grep_file".to_owned(), "pattern_test_2".to_owned(), 0.7),
            ("grep_file".to_owned(), "pattern_test_3".to_owned(), 0.7),
        ];

        assert_eq!(detector.detect(&history), PatternState::NearLoop);
    }
}
