use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Cache for validation results to avoid repetitive checks for the same tool/args combination.
pub struct ValidationCache {
    cache: RwLock<HashMap<(String, u64), (bool, Instant)>>,
    ttl: Duration,
}

impl ValidationCache {
    /// Create a new validation cache with the specified TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Check if a validation result is cached and valid.
    pub fn check(&self, tool: &str, args_hash: u64) -> Option<bool> {
        let cache = self.cache.read().ok()?;
        cache
            .get(&(tool.to_string(), args_hash))
            .filter(|(_, ts)| ts.elapsed() < self.ttl)
            .map(|(result, _)| *result)
    }

    /// Insert a validation result into the cache.
    pub fn insert(&self, tool: &str, args_hash: u64, result: bool) {
        if let Ok(mut cache) = self.cache.write() {
            cache.insert((tool.to_string(), args_hash), (result, Instant::now()));
        }
    }
}

impl Default for ValidationCache {
    fn default() -> Self {
        Self::new(Duration::from_secs(300)) // 5 minutes default
    }
}
