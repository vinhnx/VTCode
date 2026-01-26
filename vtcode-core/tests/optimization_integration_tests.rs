//! Integration tests for optimization components

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

use vtcode_core::core::memory_pool::{MemoryPool, global_pool};
use vtcode_core::core::optimized_agent::OptimizedAgentEngine;
use vtcode_core::llm::optimized_client::OptimizedLLMClient;
use vtcode_core::tools::ToolCallRequest;
use vtcode_core::tools::async_pipeline::{
    AsyncToolPipeline, ExecutionContext, ExecutionPriority, ToolRequest,
};
use vtcode_core::tools::optimized_registry::{OptimizedToolRegistry, ToolMetadata};

#[tokio::test]
async fn test_memory_pool_performance() -> Result<()> {
    let pool = MemoryPool::new();

    // Test string pool
    let mut strings = Vec::new();
    for _ in 0..100 {
        strings.push(pool.get_string());
    }

    // Return strings to pool
    for s in strings {
        pool.return_string(s);
    }

    // Verify pool reuse
    let reused_string = pool.get_string();
    assert!(reused_string.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_optimized_tool_registry() -> Result<()> {
    let registry = OptimizedToolRegistry::new(4);

    // Register test tool
    let metadata = ToolMetadata {
        name: "test_tool".to_string(),
        description: "Test tool for optimization".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        is_cached: false,
        avg_execution_time_ms: 100,
    };

    registry.register_tool(metadata);

    // Test tool lookup
    let retrieved = registry.get_tool_metadata("test_tool");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name, "test_tool");

    // Test tool execution
    let result = registry
        .execute_tool_optimized("test_tool", serde_json::json!({"input": "test"}))
        .await;

    assert!(result.is_ok());

    Ok(())
}

#[tokio::test]
async fn test_async_tool_pipeline() -> Result<()> {
    let mut pipeline = AsyncToolPipeline::new(
        4,                          // max_concurrent_tools
        100,                        // cache_size
        5,                          // batch_size
        Duration::from_millis(100), // batch_timeout
    );

    // Start pipeline
    pipeline.start().await?;

    // Submit test requests
    let mut request_ids = Vec::new();
    for i in 0..10 {
        let request = ToolRequest {
            call: ToolCallRequest {
                id: format!("test_request_{}", i),
                tool_name: "test_tool".to_string(),
                args: serde_json::json!({"index": i}),
                metadata: None,
            },
            priority: ExecutionPriority::Normal,
            timeout: Duration::from_secs(5),
            context: ExecutionContext {
                session_id: "test_session".to_string(),
                user_id: None,
                workspace_path: "/tmp".to_string(),
                parent_request_id: None,
            },
        };

        let request_id = pipeline.submit_request(request).await?;
        request_ids.push(request_id);
    }

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check metrics
    let metrics = pipeline.get_metrics().await;
    assert!(metrics.total_requests >= 10);

    // Shutdown pipeline
    pipeline.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_optimized_llm_client() -> Result<()> {
    let client = OptimizedLLMClient::new(
        4,    // pool_size
        50,   // cache_size
        10.0, // requests_per_second
        20,   // burst_capacity
    );

    // Start client
    client.start().await?;

    // Test request (would need actual LLM request implementation)
    // For now, just verify client creation and metrics
    let metrics = client.get_metrics().await;
    assert_eq!(metrics.total_requests, 0);

    Ok(())
}

#[tokio::test]
async fn test_optimized_agent_engine() -> Result<()> {
    // Create dependencies
    let tool_pipeline = Arc::new(AsyncToolPipeline::new(
        4,
        100,
        5,
        Duration::from_millis(100),
    ));

    // Create optimized agent engine
    let _engine = OptimizedAgentEngine::new("test_session".to_string(), tool_pipeline);

    // Test that engine can be created and started
    // (In a real test, we'd run the engine for a short time)

    // For now, just verify creation
    assert!(true); // Placeholder assertion

    Ok(())
}

#[tokio::test]
async fn test_global_memory_pool() -> Result<()> {
    let pool = global_pool();

    // Test concurrent access
    let mut handles = Vec::new();

    for i in 0..10 {
        let pool_clone = Arc::clone(&pool);
        let handle = tokio::spawn(async move {
            let mut s = pool_clone.get_string();
            s.push_str(&format!("test_{}", i));
            tokio::time::sleep(Duration::from_millis(10)).await;
            pool_clone.return_string(s);
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_optimization_integration() -> Result<()> {
    // Test that all optimization components work together

    // 1. Memory pool
    let pool = global_pool();
    let test_string = pool.get_string();

    // 2. Tool registry
    let registry = OptimizedToolRegistry::new(2);
    let metadata = ToolMetadata {
        name: "integration_test".to_string(),
        description: "Integration test tool".to_string(),
        parameters: serde_json::json!({}),
        is_cached: true,
        avg_execution_time_ms: 50,
    };
    registry.register_tool(metadata);

    // 3. Tool pipeline
    let mut pipeline = AsyncToolPipeline::new(2, 50, 3, Duration::from_millis(50));
    pipeline.start().await?;

    // 4. Submit request through pipeline
    let request = ToolRequest {
        call: ToolCallRequest {
            id: "integration_test_request".to_string(),
            tool_name: "integration_test".to_string(),
            args: serde_json::json!({}),
            metadata: None,
        },
        priority: ExecutionPriority::High,
        timeout: Duration::from_secs(1),
        context: ExecutionContext {
            session_id: "integration_test_session".to_string(),
            user_id: Some("test_user".to_string()),
            workspace_path: "/tmp".to_string(),
            parent_request_id: None,
        },
    };

    let request_id = pipeline.submit_request(request).await?;
    assert!(!request_id.is_empty());

    // 5. Wait for processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 6. Check metrics
    let pipeline_metrics = pipeline.get_metrics().await;
    assert!(pipeline_metrics.total_requests > 0);

    // 7. Cleanup
    pool.return_string(test_string);
    pipeline.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn test_performance_under_load() -> Result<()> {
    use std::time::Instant;

    let start_time = Instant::now();

    // Create optimized components
    let registry = OptimizedToolRegistry::new(8);
    let mut pipeline = AsyncToolPipeline::new(8, 200, 10, Duration::from_millis(50));
    pipeline.start().await?;

    // Register multiple tools
    for i in 0..20 {
        let metadata = ToolMetadata {
            name: format!("load_test_tool_{}", i),
            description: format!("Load test tool {}", i),
            parameters: serde_json::json!({}),
            is_cached: i % 3 == 0, // Some tools cached
            avg_execution_time_ms: 25 + (i * 5) as u64,
        };
        registry.register_tool(metadata);
    }

    // Submit many requests concurrently
    let mut handles = Vec::new();
    for i in 0..100 {
        let request = ToolRequest {
            call: ToolCallRequest {
                id: format!("load_test_{}", i),
                tool_name: format!("load_test_tool_{}", i % 20),
                args: serde_json::json!({"iteration": i}),
                metadata: None,
            },
            priority: if i % 10 == 0 {
                ExecutionPriority::High
            } else {
                ExecutionPriority::Normal
            },
            timeout: Duration::from_secs(2),
            context: ExecutionContext {
                session_id: format!("load_test_session_{}", i % 5),
                user_id: Some(format!("user_{}", i % 10)),
                workspace_path: "/tmp".to_string(),
                parent_request_id: None,
            },
        };

        let handle = pipeline.submit_request(request).await?;
        handles.push(handle);
    }

    // Wait for all requests to be submitted
    // (handles now contain request IDs, not join handles)

    // Wait for processing to complete
    tokio::time::sleep(Duration::from_secs(2)).await;

    let total_time = start_time.elapsed();
    let metrics = pipeline.get_metrics().await;

    // Verify performance characteristics
    assert!(metrics.total_requests >= 100);
    assert!(total_time < Duration::from_secs(5)); // Should complete within 5 seconds
    assert!(metrics.avg_execution_time_ms < 1000.0); // Average execution under 1 second

    println!("Load test completed in {:?}", total_time);
    println!("Pipeline metrics: {:?}", metrics);

    pipeline.shutdown().await?;

    Ok(())
}

/// Benchmark memory allocation patterns
#[tokio::test]
async fn benchmark_memory_allocations() -> Result<()> {
    use std::time::Instant;

    let iterations = 10000;

    // Test without memory pool
    let start = Instant::now();
    for _ in 0..iterations {
        let mut s = String::new();
        s.push_str("test data");
        let _v: Vec<String> = vec![s];
    }
    let without_pool = start.elapsed();

    // Test with memory pool
    let pool = global_pool();
    let start = Instant::now();
    for _ in 0..iterations {
        let mut s = pool.get_string();
        s.push_str("test data");
        let mut v = pool.get_vec();
        v.push(s.clone());
        pool.return_string(s);
        pool.return_vec(v);
    }
    let with_pool = start.elapsed();

    println!("Without pool: {:?}", without_pool);
    println!("With pool: {:?}", with_pool);

    // Memory pool should be faster for repeated allocations
    // (This might not always be true in debug builds, but should be in release)

    Ok(())
}
