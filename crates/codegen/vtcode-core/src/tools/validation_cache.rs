use std::time::Duration;

use crate::cache::{CacheKey, EvictionPolicy, UnifiedCache};

/// Cache key for validation results.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ValidationKey {
    tool: String,
    args_hash: u64,
}

impl CacheKey for ValidationKey {
    fn to_cache_key(&self) -> String {
        format!("{}:{}", self.tool, self.args_hash)
    }
}

/// Cache for validation results to avoid repetitive checks for the same tool/args combination.
pub struct ValidationCache {
    cache: UnifiedCache<ValidationKey, bool>,
}

impl ValidationCache {
    /// Create a new validation cache with the specified TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: UnifiedCache::new(1000, ttl, EvictionPolicy::TtlOnly),
        }
    }

    /// Check if a validation result is cached and valid.
    pub fn check(&self, tool: &str, args_hash: u64) -> Option<bool> {
        let key = ValidationKey {
            tool: tool.to_string(),
            args_hash,
        };
        self.cache.get_owned(&key)
    }

    /// Insert a validation result into the cache.
    pub fn insert(&self, tool: &str, args_hash: u64, result: bool) {
        let key = ValidationKey {
            tool: tool.to_string(),
            args_hash,
        };
        self.cache.insert(key, result, 1); // bool is tiny
    }
}

impl Default for ValidationCache {
    fn default() -> Self {
        Self::new(Duration::from_secs(300)) // 5 minutes default
    }
}
