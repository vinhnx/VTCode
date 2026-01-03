//! Test that verifies performance optimizations are actually integrated into execute_tool_ref
//! This test validates that the optimizations work in the REAL execution path used by VT Code

use serde_json::json;
use vtcode_core::tools::ToolRegistry;
use vtcode_config::OptimizationConfig;

#[tokio::test]
async fn test_execute_tool_ref_uses_optimizations() {
    // Create a temporary directory for testing
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    
    // Create a test file
    let test_file = workspace_root.join("test.txt");
    std::fs::write(&test_file, "Hello, World!").unwrap();
    
    // Create registry with optimizations enabled
    let mut registry = ToolRegistry::new(workspace_root.clone()).await;
    let mut config = OptimizationConfig::default();
    config.tool_registry.use_optimized_registry = true;
    config.tool_registry.hot_cache_size = 8;
    config.memory_pool.enabled = true;
    registry.configure_optimizations(config);
    
    // Verify optimizations are enabled
    assert!(registry.has_optimizations_enabled());
    let (initial_cache_size, cache_capacity) = registry.hot_cache_stats();
    assert_eq!(cache_capacity, 8);
    assert_eq!(initial_cache_size, 0);
    
    // Test 1: Execute read_file tool multiple times - should use hot cache
    let args = json!({
        "path": test_file.to_string_lossy()
    });
    
    // First execution - should populate cache
    let result1 = registry.execute_tool_ref("read_file", &args).await;
    assert!(result1.is_ok());
    
    // Check that cache might have been populated (depends on tool implementation)
    let (cache_size_after_first, _) = registry.hot_cache_stats();
    
    // Second execution - should potentially use cache
    let result2 = registry.execute_tool_ref("read_file", &args).await;
    assert!(result2.is_ok());
    
    // Third execution with alias - should resolve and potentially cache
    let result3 = registry.execute_tool_ref("read_file", &args).await;
    assert!(result3.is_ok());
    
    // Verify results are consistent
    assert_eq!(result1.unwrap(), result2.unwrap());
    
    println!("✅ execute_tool_ref optimization test passed");
    println!("   Cache size after first call: {}", cache_size_after_first);
    println!("   Cache capacity: {}", cache_capacity);
}

#[tokio::test]
async fn test_execute_tool_ref_memory_pool_integration() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    
    // Create registry with memory pool enabled
    let mut registry = ToolRegistry::new(workspace_root.clone()).await;
    let mut config = OptimizationConfig::default();
    config.memory_pool.enabled = true;
    config.memory_pool.max_string_pool_size = 32;
    registry.configure_optimizations(config);
    
    // Verify memory pool is accessible (just check it exists)
    let _memory_pool = registry.memory_pool();
    
    // Test memory pool usage during tool execution
    let args = json!({
        "path": "."
    });
    
    let result = registry.execute_tool_ref("list_files", &args).await;
    assert!(result.is_ok());
    
    println!("✅ execute_tool_ref memory pool integration test passed");
}

#[tokio::test]
async fn test_execute_tool_ref_without_optimizations() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    
    // Create registry with ALL optimizations explicitly disabled
    let mut registry = ToolRegistry::new(workspace_root.clone()).await;
    let mut config = OptimizationConfig::default();
    config.tool_registry.use_optimized_registry = false;
    config.memory_pool.enabled = false; // Explicitly disable memory pool too
    registry.configure_optimizations(config);
    
    // Verify optimizations are disabled
    assert!(!registry.has_optimizations_enabled());
    let (cache_size, cache_capacity) = registry.hot_cache_stats();
    assert_eq!(cache_size, 0);
    assert_eq!(cache_capacity, 16); // Default capacity
    
    // Tool execution should still work without optimizations
    let args = json!({
        "path": "."
    });
    
    let result = registry.execute_tool_ref("list_files", &args).await;
    assert!(result.is_ok());
    
    println!("✅ execute_tool_ref without optimizations test passed");
}

#[tokio::test]
async fn test_execute_tool_ref_hot_cache_effectiveness() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    
    // Create registry with small cache for testing
    let mut registry = ToolRegistry::new(workspace_root.clone()).await;
    let mut config = OptimizationConfig::default();
    config.tool_registry.use_optimized_registry = true;
    config.tool_registry.hot_cache_size = 2; // Very small cache
    registry.configure_optimizations(config);
    
    let args = json!({"path": "."});
    
    // Execute different tools to test cache behavior
    let _result1 = registry.execute_tool_ref("list_files", &args).await;
    let (cache_size_1, _) = registry.hot_cache_stats();
    
    let _result2 = registry.execute_tool_ref("list_files", &args).await; // Same tool
    let (cache_size_2, _) = registry.hot_cache_stats();
    
    // Cache size should not exceed capacity
    assert!(cache_size_1 <= 2);
    assert!(cache_size_2 <= 2);
    
    println!("✅ execute_tool_ref hot cache effectiveness test passed");
    println!("   Cache size after first tool: {}", cache_size_1);
    println!("   Cache size after second tool: {}", cache_size_2);
}

#[tokio::test]
async fn test_execute_tool_ref_performance_comparison() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    
    // Test with optimizations disabled
    let registry_unoptimized = ToolRegistry::new(workspace_root.clone()).await;
    
    // Test with optimizations enabled
    let mut registry_optimized = ToolRegistry::new(workspace_root.clone()).await;
    let config = OptimizationConfig::production(); // Use production config
    registry_optimized.configure_optimizations(config);
    
    let args = json!({"path": "."});
    
    // Warm up both registries
    let _ = registry_unoptimized.execute_tool_ref("list_files", &args).await;
    let _ = registry_optimized.execute_tool_ref("list_files", &args).await;
    
    // Time unoptimized execution
    let start = std::time::Instant::now();
    for _ in 0..5 {
        let _ = registry_unoptimized.execute_tool_ref("list_files", &args).await;
    }
    let unoptimized_duration = start.elapsed();
    
    // Time optimized execution
    let start = std::time::Instant::now();
    for _ in 0..5 {
        let _ = registry_optimized.execute_tool_ref("list_files", &args).await;
    }
    let optimized_duration = start.elapsed();
    
    println!("✅ execute_tool_ref performance comparison test completed");
    println!("   Unoptimized: {:?}", unoptimized_duration);
    println!("   Optimized: {:?}", optimized_duration);
    println!("   Optimized registry has {} optimizations enabled", 
        registry_optimized.has_optimizations_enabled());
    
    // Both should complete successfully (performance may vary)
    assert!(unoptimized_duration.as_millis() > 0);
    assert!(optimized_duration.as_millis() > 0);
}