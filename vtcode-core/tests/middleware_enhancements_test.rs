#![allow(deprecated)]

use std::sync::Arc;
use vtcode_core::exec::agent_optimization::AgentBehaviorAnalyzer;
use vtcode_core::tools::middleware::{
    CircuitBreakerMiddleware, ExecutionMetadata, MetricsMiddleware, Middleware, MiddlewareChain,
    MiddlewareResult, RequestMetadata, ToolRequest,
};

#[test]
fn test_metrics_middleware_records_success() {
    let analyzer = Arc::new(std::sync::RwLock::new(AgentBehaviorAnalyzer::new()));
    let middleware = MetricsMiddleware::new(analyzer.clone());

    let request = ToolRequest {
        tool_name: "test_tool".into(),
        arguments: "arg".into(),
        context: "ctx".into(),
        metadata: RequestMetadata::default(),
    };

    let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
        success: true,
        result: Some("result".into()),
        error: None,
        metadata: ExecutionMetadata::default(),
    });

    let result = middleware.execute(request, executor);
    assert!(result.success);
    assert!(result.metadata.layers_executed.contains(&"metrics".into()));

    // Verify metrics were recorded
    let analyzer_lock = analyzer.read().unwrap();
    assert_eq!(
        *analyzer_lock
            .tool_stats()
            .usage_frequency
            .get("test_tool")
            .unwrap(),
        1
    );
}

#[test]
fn test_metrics_middleware_records_failure() {
    let analyzer = Arc::new(std::sync::RwLock::new(AgentBehaviorAnalyzer::new()));
    let middleware = MetricsMiddleware::new(analyzer.clone());

    let request = ToolRequest {
        tool_name: "failing_tool".into(),
        arguments: "arg".into(),
        context: "ctx".into(),
        metadata: RequestMetadata::default(),
    };

    let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
        success: false,
        result: None,
        error: Some(vtcode_core::tools::middleware::MiddlewareError::ExecutionFailed("test error")),
        metadata: ExecutionMetadata::default(),
    });

    let result = middleware.execute(request, executor);
    assert!(!result.success);

    // Verify failure was recorded
    let analyzer_lock = analyzer.read().unwrap();
    assert!(
        !analyzer_lock
            .failure_patterns()
            .high_failure_tools
            .is_empty()
    );
}

#[test]
fn test_circuit_breaker_opens_after_failures() {
    let middleware = CircuitBreakerMiddleware::new(0.5);

    let request = ToolRequest {
        tool_name: "failing_tool".into(),
        arguments: "arg".into(),
        context: "ctx".into(),
        metadata: RequestMetadata::default(),
    };

    // Simulate 5 failures to open circuit
    for _ in 0..5 {
        let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
            success: false,
            result: None,
            error: Some(
                vtcode_core::tools::middleware::MiddlewareError::ExecutionFailed("test error"),
            ),
            metadata: ExecutionMetadata::default(),
        });

        let _ = middleware.execute(request.clone(), executor);
    }

    // Next call should be blocked
    let executor = Box::new(|_req: ToolRequest| MiddlewareResult {
        success: true,
        result: Some("should not execute".into()),
        error: None,
        metadata: ExecutionMetadata::default(),
    });

    let result = middleware.execute(request, executor);
    assert!(!result.success);
    assert!(
        result
            .metadata
            .layers_executed
            .contains(&"circuit_breaker".into())
    );
}

#[test]
fn test_middleware_chain_with_metrics_and_circuit_breaker() {
    let analyzer = Arc::new(std::sync::RwLock::new(AgentBehaviorAnalyzer::new()));

    let chain = MiddlewareChain::new()
        .with_metrics(analyzer.clone())
        .with_circuit_breaker(0.8);

    let request = ToolRequest {
        tool_name: "test_tool".into(),
        arguments: "arg".into(),
        context: "ctx".into(),
        metadata: RequestMetadata::default(),
    };

    let executor = |_req: ToolRequest| MiddlewareResult {
        success: true,
        result: Some("result".into()),
        error: None,
        metadata: ExecutionMetadata::default(),
    };

    let result = chain.execute_sync(request, executor);
    assert!(result.success);
    assert!(result.metadata.layers_executed.contains(&"metrics".into()));
    assert!(
        result
            .metadata
            .layers_executed
            .contains(&"circuit_breaker".into())
    );
}
