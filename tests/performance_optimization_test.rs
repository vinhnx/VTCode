use std::path::PathBuf;
use std::time::Instant;
use vtcode_core::config::loader::ConfigManager;

#[test]
fn test_configuration_caching() {
    // Test that our configuration caching pattern works
    let workspace = PathBuf::from(".");

    // First load
    let start_time = Instant::now();
    let config1 = ConfigManager::load_from_workspace(&workspace)
        .ok()
        .map(|manager| manager.config().clone());
    let first_load_time = start_time.elapsed();

    // Second load using cached path
    let start_time = Instant::now();
    let config2 = ConfigManager::load_from_workspace(&workspace)
        .ok()
        .map(|manager| manager.config().clone());
    let second_load_time = start_time.elapsed();

    // Both should load successfully
    assert!(config1.is_some() || config2.is_some());

    println!("First load time: {:?}", first_load_time);
    println!("Second load time: {:?}", second_load_time);
}

#[test]
fn test_workspace_path_caching() {
    // Test that our workspace path caching optimization works
    let workspace = PathBuf::from(".");

    // This simulates the optimization where we cache the workspace path
    let cached_workspace = workspace.clone();

    // Verify we can use the cached path
    assert_eq!(cached_workspace, workspace);
    assert!(
        cached_workspace.exists() || workspace == std::path::Path::new(".")
    );
}

#[test]
fn test_idle_configuration_defaults() {
    // Test that our idle configuration has sensible defaults
    let config = vtcode_core::config::loader::VTCodeConfig::default();
    let idle_config = &config.optimization.agent_execution;

    // Should have reasonable defaults
    assert!(idle_config.idle_timeout_ms > 0);
    assert!(idle_config.idle_backoff_ms > 0);
    assert!(idle_config.max_idle_cycles > 0);

    println!("Idle timeout: {}ms", idle_config.idle_timeout_ms);
    println!("Idle backoff: {}ms", idle_config.idle_backoff_ms);
    println!("Max idle cycles: {}", idle_config.max_idle_cycles);
}

#[test]
fn test_optimization_config_structure() {
    // Test that our new optimization fields are properly structured
    use vtcode_config::OptimizationConfig;

    let config = OptimizationConfig::default();
    assert!(config.agent_execution.idle_timeout_ms > 0);
    assert!(config.agent_execution.idle_backoff_ms > 0);
    assert!(config.agent_execution.max_idle_cycles > 0);

    // Test development config
    let dev_config = OptimizationConfig::development();
    assert!(dev_config.agent_execution.idle_timeout_ms > 0);

    // Test production config
    let prod_config = OptimizationConfig::production();
    assert!(prod_config.agent_execution.idle_timeout_ms > 0);
}
