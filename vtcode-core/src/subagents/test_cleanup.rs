//! Test for subagent cleanup functionality

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;
    use crate::subagents::{SubagentRegistry, SubagentsConfig};

    fn test_config() -> SubagentsConfig {
        SubagentsConfig {
            enabled: true,
            max_concurrent: 3,
            default_timeout_seconds: 300,
            default_model: None,
            additional_agent_dirs: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_cleanup_on_initialization() {
        // Create a registry
        let config = test_config();
        let max_concurrent = config.max_concurrent;

        let registry = SubagentRegistry::new(
            PathBuf::from("/tmp/test"),
            config
        ).await.unwrap();

        // Initially should have 0 running
        let count = registry.running_count().await;
        eprintln!("Running count: {}", count);
        assert_eq!(count, 0);

        let can_spawn = registry.can_spawn().await;
        eprintln!("Can spawn: {}", can_spawn);
        eprintln!("Max concurrent: {}", max_concurrent);
        assert!(can_spawn);
    }

    #[tokio::test]
    async fn test_stale_cleanup() {
        let mut config = test_config();
        config.default_timeout_seconds = 1; // 1 second timeout

        let registry = SubagentRegistry::new(
            PathBuf::from("/tmp/test"),
            config
        ).await.unwrap();

        // Manually register a "running" subagent
        if let Some(agent_config) = registry.get("explore") {
            registry.register_running("test-123".to_string(), agent_config.clone()).await;
        }

        // Should show 1 running (no cleanup yet)
        assert_eq!(registry.running_count().await, 1);

        // Wait for stale timeout (2x default_timeout_seconds = 2 seconds)
        tokio::time::sleep(Duration::from_secs(3)).await;

        // can_spawn triggers cleanup
        let can_spawn = registry.can_spawn().await;
        assert!(can_spawn);

        // Check count again - should be 0 after cleanup
        assert_eq!(registry.running_count().await, 0);
    }

    #[tokio::test]
    async fn test_can_spawn_respects_max_concurrent() {
        let mut config = test_config();
        config.max_concurrent = 2; // Only allow 2 concurrent

        let registry = SubagentRegistry::new(
            PathBuf::from("/tmp/test"),
            config
        ).await.unwrap();

        let agent_config = registry.get("explore").unwrap().clone();

        // Register 2 subagents
        registry.register_running("test-1".to_string(), agent_config.clone()).await;
        registry.register_running("test-2".to_string(), agent_config.clone()).await;

        // Should not be able to spawn more
        assert!(!registry.can_spawn().await);

        // Unregister one
        registry.unregister_running("test-1").await;

        // Now should be able to spawn
        assert!(registry.can_spawn().await);
    }
}
