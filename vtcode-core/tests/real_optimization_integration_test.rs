//! Test that verifies the REAL optimizations are integrated into the actual ToolRegistry

use std::sync::Arc;
use anyhow::Result;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_config::OptimizationConfig;

#[tokio::test]
async fn test_real_tool_registry_optimizations() -> Result<()> {
    // Create a real ToolRegistry (the one actually used by VT Code)
    let workspace = std::env::temp_dir();
    let mut registry = ToolRegistry::new(workspace).await;
    
    // Verify default state - memory pool is enabled by default
    let initial_optimizations = registry.has_optimizations_enabled();
    let (cache_size, cache_cap) = registry.hot_cache_stats();
    assert_eq!(cache_size, 0);
    assert_eq!(cache_cap, 16); // Default cache size
    
    // The registry should have memory pool enabled by default
    assert!(initial_optimizations, "Memory pool should be enabled by default");
    
    // Configure optimizations
    let mut opt_config = OptimizationConfig::default();
    opt_config.tool_registry.use_optimized_registry = true;
    opt_config.tool_registry.hot_cache_size = 32;
    opt_config.memory_pool.enabled = true;
    
    registry.configure_optimizations(opt_config);
    
    // Verify optimizations are enabled
    assert!(registry.has_optimizations_enabled());
    let (cache_size, cache_cap) = registry.hot_cache_stats();
    assert_eq!(cache_size, 0); // Still empty
    assert_eq!(cache_cap, 32); // Resized cache
    
    // Verify memory pool is available
    let memory_pool = registry.memory_pool();
    let test_string = memory_pool.get_string();
    memory_pool.return_string(test_string);
    
    println!("✅ Real ToolRegistry optimizations are working!");
    
    Ok(())
}

#[tokio::test]
async fn test_tool_hot_cache_functionality() -> Result<()> {
    let workspace = std::env::temp_dir();
    let mut registry = ToolRegistry::new(workspace).await;
    
    // Enable optimizations
    let mut opt_config = OptimizationConfig::default();
    opt_config.tool_registry.use_optimized_registry = true;
    registry.configure_optimizations(opt_config);
    
    // Test tool lookup (this will use the hot cache path)
    let tool1 = registry.get_tool("nonexistent_tool");
    assert!(tool1.is_none());
    
    // The cache should still be empty since the tool doesn't exist
    let (cache_size, _) = registry.hot_cache_stats();
    assert_eq!(cache_size, 0);
    
    // Clear cache (should work without errors)
    registry.clear_hot_cache();
    
    println!("✅ Hot cache functionality is working!");
    
    Ok(())
}

#[tokio::test]
async fn test_optimization_config_integration() -> Result<()> {
    let workspace = std::env::temp_dir();
    let registry = ToolRegistry::new(workspace).await;
    
    // Check default optimization config
    let config = registry.optimization_config();
    assert!(!config.tool_registry.use_optimized_registry);
    assert!(config.memory_pool.enabled); // Default is enabled
    
    println!("✅ Optimization config integration is working!");
    
    Ok(())
}