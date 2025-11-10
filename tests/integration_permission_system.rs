//! Integration tests for the permission system (CommandResolver, Cache, Audit)

#[cfg(test)]
mod integration_tests {
    use std::path::PathBuf;
    use tempfile::TempDir;
    use vtcode_core::{
        audit::{PermissionAuditLog, PermissionDecision},
        tools::{CommandResolver, PermissionCache},
    };

    #[test]
    fn test_command_resolver_basic() {
        let mut resolver = CommandResolver::new();

        // Test resolving a common command
        let resolution = resolver.resolve("ls");
        assert_eq!(resolution.command, "ls");
        assert!(resolution.found, "ls should be found on Unix systems");
        assert!(resolution.resolved_path.is_some());
    }

    #[test]
    fn test_command_resolver_caching() {
        let mut resolver = CommandResolver::new();

        // First resolution (cache miss)
        resolver.resolve("cargo");
        let (hits1, misses1) = resolver.cache_stats();
        assert_eq!(hits1, 0, "First resolve should be a miss");
        assert_eq!(misses1, 1, "Should have 1 miss");

        // Second resolution (cache hit)
        resolver.resolve("cargo");
        let (hits2, misses2) = resolver.cache_stats();
        assert_eq!(hits2, 1, "Second resolve should be a hit");
        assert_eq!(misses2, 1, "Should still have 1 miss");
    }

    #[test]
    fn test_permission_cache_store_and_retrieve() {
        let mut cache = PermissionCache::new();

        // Store a decision
        cache.put("cargo fmt", true, "test reason");

        // Retrieve it
        assert_eq!(cache.get("cargo fmt"), Some(true));

        // Different command should not be cached
        assert_eq!(cache.get("cargo check"), None);
    }

    #[test]
    fn test_permission_cache_expiration() {
        use std::thread;
        use std::time::Duration;

        let mut cache = PermissionCache::with_ttl(Duration::from_millis(50));

        cache.put("test", true, "reason");
        assert_eq!(
            cache.get("test"),
            Some(true),
            "Should be available immediately"
        );

        thread::sleep(Duration::from_millis(100));
        assert_eq!(cache.get("test"), None, "Should expire after TTL");
    }

    #[test]
    fn test_audit_log_creation_and_logging() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let mut log = PermissionAuditLog::new(dir.path().to_path_buf())?;

        assert_eq!(log.event_count(), 0);

        // Log a decision
        log.log_command_decision(
            "cargo fmt",
            PermissionDecision::Allowed,
            "allow_glob match: cargo *",
            Some(PathBuf::from("/usr/local/bin/cargo")),
        )?;

        assert_eq!(log.event_count(), 1);
        assert!(dir.path().exists());
        assert!(log.log_path().exists());

        Ok(())
    }

    #[test]
    fn test_full_permission_flow() -> anyhow::Result<()> {
        use std::path::Path;

        // 1. Initialize audit log
        let audit_dir = TempDir::new()?;
        let mut audit_log = PermissionAuditLog::new(audit_dir.path().to_path_buf())?;

        // 2. Initialize resolver
        let mut resolver = CommandResolver::new();

        // 3. Initialize cache
        let mut cache = PermissionCache::new();

        // 4. Simulate command evaluation
        let command = "cargo fmt";

        // Check cache
        let cached_decision = cache.get(command);
        assert!(cached_decision.is_none(), "Cache should be empty initially");

        // Resolve command
        let resolution = resolver.resolve(command);
        assert_eq!(resolution.command, "cargo");

        // Simulate policy evaluation
        let allowed = true;

        // Cache the decision
        cache.put(command, allowed, "allow_glob match: cargo *");

        // Log to audit
        audit_log.log_command_decision(
            command,
            if allowed {
                PermissionDecision::Allowed
            } else {
                PermissionDecision::Denied
            },
            "allow_glob match: cargo *",
            resolution.resolved_path.clone(),
        )?;

        // Verify results
        assert_eq!(cache.get(command), Some(true));
        assert_eq!(audit_log.event_count(), 1);

        Ok(())
    }

    #[test]
    fn test_resolver_with_args_extracts_base_command() {
        let mut resolver = CommandResolver::new();

        // Should extract "cargo" from "cargo fmt --check"
        let resolution = resolver.resolve("cargo fmt --check");
        assert_eq!(resolution.command, "cargo");
    }

    #[test]
    fn test_nonexistent_command_not_found() {
        let mut resolver = CommandResolver::new();

        let resolution = resolver.resolve("this_command_definitely_does_not_exist_xyz_123");
        assert!(!resolution.found);
        assert!(resolution.resolved_path.is_none());
    }

    #[test]
    fn test_cache_multiple_commands() {
        let mut cache = PermissionCache::new();

        cache.put("cmd1", true, "allowed");
        cache.put("cmd2", false, "denied");
        cache.put("cmd3", true, "allowed");

        assert_eq!(cache.get("cmd1"), Some(true));
        assert_eq!(cache.get("cmd2"), Some(false));
        assert_eq!(cache.get("cmd3"), Some(true));

        let (total, _expired) = cache.stats();
        assert_eq!(total, 3);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = PermissionCache::new();

        cache.put("cmd1", true, "test");
        cache.put("cmd2", false, "test");

        let (total_before, _) = cache.stats();
        assert_eq!(total_before, 2);

        cache.clear();

        let (total_after, _) = cache.stats();
        assert_eq!(total_after, 0);

        assert_eq!(cache.get("cmd1"), None);
    }

    #[test]
    fn test_audit_log_file_location() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let audit_log = PermissionAuditLog::new(dir.path().to_path_buf())?;

        let log_path = audit_log.log_path();
        assert!(log_path.to_string_lossy().contains("permissions-"));
        assert!(log_path.to_string_lossy().contains(".log"));

        Ok(())
    }

    #[tokio::test]
    async fn test_resolver_stats() {
        let mut resolver = CommandResolver::new();

        resolver.resolve("ls");
        resolver.resolve("ls");
        resolver.resolve("pwd");
        resolver.resolve("pwd");
        resolver.resolve("pwd");

        let (hits, misses) = resolver.cache_stats();
        assert_eq!(hits, 3, "Should have 3 cache hits");
        assert_eq!(misses, 2, "Should have 2 cache misses");
    }
}
