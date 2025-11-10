//! Command permission cache
//! Caches policy evaluation results with TTL to improve performance

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::debug;

/// A cached permission decision
#[derive(Debug, Clone)]
struct CacheEntry {
    allowed: bool,
    timestamp: Instant,
    reason: String,
}

/// Cache for command permission decisions
pub struct PermissionCache {
    entries: HashMap<String, CacheEntry>,
    ttl: Duration,
}

impl PermissionCache {
    /// Create cache with 5-minute default TTL
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(300),
        }
    }

    /// Create cache with custom TTL
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    /// Check if a command is cached and not expired
    pub fn get(&self, command: &str) -> Option<bool> {
        self.entries.get(command).and_then(|entry| {
            if entry.timestamp.elapsed() < self.ttl {
                debug!(
                    command = command,
                    reason = &entry.reason,
                    "Permission cache hit ({}s old)",
                    entry.timestamp.elapsed().as_secs()
                );
                Some(entry.allowed)
            } else {
                None
            }
        })
    }

    /// Store a permission decision in cache
    pub fn put(&mut self, command: &str, allowed: bool, reason: &str) {
        self.entries.insert(
            command.to_string(),
            CacheEntry {
                allowed,
                timestamp: Instant::now(),
                reason: reason.to_string(),
            },
        );
        debug!(
            command = command,
            allowed = allowed,
            reason = reason,
            "Cached permission decision"
        );
    }

    /// Clear expired entries
    pub fn cleanup_expired(&mut self) {
        let cutoff = Instant::now() - self.ttl;
        self.entries.retain(|_, entry| entry.timestamp > cutoff);
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        let total = self.entries.len();
        let expired = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.timestamp.elapsed() >= self.ttl)
            .count();
        (total, expired)
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
        debug!("Permission cache cleared");
    }
}

impl Default for PermissionCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_cache_stores_decision() {
        let mut cache = PermissionCache::new();
        cache.put("cargo fmt", true, "allow_glob match");
        assert_eq!(cache.get("cargo fmt"), Some(true));
    }

    #[test]
    fn test_cache_expires() {
        let mut cache = PermissionCache::with_ttl(Duration::from_millis(100));
        cache.put("cargo fmt", true, "test");

        // Immediately available
        assert_eq!(cache.get("cargo fmt"), Some(true));

        // Wait for expiration
        thread::sleep(Duration::from_millis(150));
        assert_eq!(cache.get("cargo fmt"), None);
    }

    #[test]
    fn test_cache_cleanup() {
        let mut cache = PermissionCache::with_ttl(Duration::from_millis(100));
        cache.put("cmd1", true, "test");
        cache.put("cmd2", false, "test");

        thread::sleep(Duration::from_millis(150));
        let (total, _) = cache.stats();
        assert_eq!(total, 2);

        cache.cleanup_expired();
        let (total, _) = cache.stats();
        assert_eq!(total, 0);
    }
}
