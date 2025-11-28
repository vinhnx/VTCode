//! Tool result caching for read-only operations
//!
//! Caches results from read-only tools (grep, list_files, ast analysis) within a session
//! to avoid re-running identical queries.

use crate::utils::current_timestamp;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Identifies a cached tool result
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CacheKey {
    /// Tool name (e.g., "grep_file", "list_files")
    pub tool: String,
    /// Normalized parameters (serialized, hashed for speed)
    pub params_hash: u64,
    /// File/path being analyzed
    pub target_path: String,
}

impl CacheKey {
    /// Create a new cache key from tool name, parameters, and target path
    #[inline]
    pub fn new(tool: &str, params: &str, target_path: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        params.hash(&mut hasher);
        let params_hash = hasher.finish();

        CacheKey {
            tool: tool.to_string(),
            params_hash,
            target_path: target_path.to_string(),
        }
    }

    /// Create a cache key directly from a JSON `serde_json::Value` to avoid
    /// serializing into an owned `String` when building a key for caches.
    #[inline]
    pub fn from_json(tool: &str, params: &serde_json::Value, target_path: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        // Try serializing to a stable byte representation, falling back to
        // `to_string()` when serialization fails (unlikely).
        if let Ok(bytes) = serde_json::to_vec(params) {
            bytes.hash(&mut hasher);
        } else {
            params.to_string().hash(&mut hasher);
        }
        let params_hash = hasher.finish();
        CacheKey {
            tool: tool.to_string(),
            params_hash,
            target_path: target_path.to_string(),
        }
    }
}

/// Cached tool result with timestamp
#[derive(Debug, Clone)]
pub struct CachedResult {
    /// The actual result
    pub output: Arc<String>,
    /// When it was cached (Unix timestamp)
    pub cached_at: u64,
    /// How many times this result was used
    pub access_count: usize,
}

impl CachedResult {
    /// Create a new cached result
    #[inline]
    pub fn new(output: String) -> Self {
        CachedResult {
            output: Arc::new(output),
            cached_at: current_timestamp(),
            access_count: 0,
        }
    }

    /// Check if result is fresh (not older than max_age_secs)
    #[inline]
    pub fn is_fresh(&self, max_age_secs: u64) -> bool {
        current_timestamp().saturating_sub(self.cached_at) <= max_age_secs
    }
}

/// Tool result cache with LRU eviction
pub struct ToolResultCache {
    /// Main cache storage
    results: HashMap<CacheKey, CachedResult>,
    /// LRU order (most recent at front)
    lru_order: VecDeque<CacheKey>,
    /// Max cache size
    capacity: usize,
    /// How long results are valid (in seconds)
    ttl_secs: u64,
}

impl ToolResultCache {
    /// Create a new cache with specified capacity
    pub fn new(capacity: usize) -> Self {
        ToolResultCache {
            results: HashMap::new(),
            lru_order: VecDeque::new(),
            capacity: capacity.max(1),
            ttl_secs: 300, // 5 minutes default
        }
    }

    /// Create with custom TTL
    pub fn with_ttl(capacity: usize, ttl_secs: u64) -> Self {
        let mut cache = Self::new(capacity);
        cache.ttl_secs = ttl_secs;
        cache
    }

    /// Remove key from LRU order - internal helper to avoid duplication
    #[inline]
    fn remove_from_lru(&mut self, key: &CacheKey) {
        if let Some(pos) = self.lru_order.iter().position(|k| k == key) {
            self.lru_order.remove(pos);
        }
    }

    /// Move key to front of LRU - internal helper to avoid duplication
    #[inline]
    fn touch_lru(&mut self, key: CacheKey) {
        self.remove_from_lru(&key);
        self.lru_order.push_front(key);
    }

    /// Insert a result into the cache
    pub fn insert(&mut self, key: CacheKey, output: String) {
        // Remove old entry if exists
        if self.results.remove(&key).is_some() {
            self.remove_from_lru(&key);
        }

        // Add to cache
        self.results.insert(key.clone(), CachedResult::new(output));
        self.lru_order.push_front(key);

        // Evict if over capacity
        while self.results.len() > self.capacity {
            if let Some(evicted) = self.lru_order.pop_back() {
                self.results.remove(&evicted);
            }
        }
    }

    /// Insert an Arc-wrapped result into the cache to avoid cloning when the caller
    /// already has an Arc<String> available.
    pub fn insert_arc(&mut self, key: CacheKey, output: Arc<String>) {
        // Remove old entry if exists
        if self.results.remove(&key).is_some() {
            self.remove_from_lru(&key);
        }

        // Add to cache
        let cached = CachedResult {
            output,
            cached_at: current_timestamp(),
            access_count: 0,
        };

        self.results.insert(key.clone(), cached);
        self.lru_order.push_front(key);

        // Evict if over capacity
        while self.results.len() > self.capacity {
            if let Some(evicted) = self.lru_order.pop_back() {
                self.results.remove(&evicted);
            }
        }
    }

    /// Retrieve a result if cached and fresh
    pub fn get(&mut self, key: &CacheKey) -> Option<String> {
        // First check freshness and get output, then update LRU order
        let (is_fresh, output) = match self.results.get_mut(key) {
            Some(result) if result.is_fresh(self.ttl_secs) => {
                result.access_count += 1;
                (true, Some((*result.output).clone()))
            }
            Some(_) => (false, None), // Expired
            None => return None,
        };

        if is_fresh {
            self.touch_lru(key.clone());
            output
        } else {
            // Expired, remove it
            self.results.remove(key);
            self.remove_from_lru(key);
            None
        }
    }

    /// Return an Arc reference to the cached output to avoid cloning.
    pub fn get_arc(&mut self, key: &CacheKey) -> Option<Arc<String>> {
        // First check freshness and get output, then update LRU order
        let (is_fresh, output) = match self.results.get_mut(key) {
            Some(result) if result.is_fresh(self.ttl_secs) => {
                result.access_count += 1;
                (true, Some(Arc::clone(&result.output)))
            }
            Some(_) => (false, None), // Expired
            None => return None,
        };

        if is_fresh {
            self.touch_lru(key.clone());
            output
        } else {
            // Expired, remove it
            self.results.remove(key);
            self.remove_from_lru(key);
            None
        }
    }

    /// Clear cache entries for a specific file
    pub fn invalidate_for_path(&mut self, path: &str) {
        self.results
            .retain(|key, _| !key.target_path.ends_with(path) && key.target_path != path);
        self.lru_order
            .retain(|key| !key.target_path.ends_with(path) && key.target_path != path);
    }

    /// Clear entire cache
    pub fn clear(&mut self) {
        self.results.clear();
        self.lru_order.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let total_accesses = self.results.values().map(|r| r.access_count).sum::<usize>();

        let total_results = self.results.len();
        let capacity = self.capacity;

        CacheStats {
            size: total_results,
            capacity,
            utilization: if capacity > 0 {
                (total_results * 100) / capacity
            } else {
                0
            },
            total_accesses,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub capacity: usize,
    pub utilization: usize,
    pub total_accesses: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_cache_key() {
        let key = CacheKey::new("grep_file", "pattern=test", "/workspace");
        assert_eq!(key.tool, "grep_file");
        assert_eq!(key.target_path, "/workspace");
    }

    #[test]
    fn from_json_and_new_equivalence() {
        let params = serde_json::json!({"a": 1, "b": [1,2,3]});
        let params_str = serde_json::to_string(&params).unwrap();
        let k1 = CacheKey::new("tool", &params_str, "/workspace");
        let k2 = CacheKey::from_json("tool", &params, "/workspace");
        assert_eq!(k1, k2);
    }

    #[test]
    fn caches_and_retrieves_result() {
        let mut cache = ToolResultCache::new(10);
        let key = CacheKey::new("grep_file", "pattern=test", "/workspace");
        let output = "line 1\nline 2".to_string();

        cache.insert_arc(key.clone(), Arc::new(output.clone()));
        assert_eq!(cache.get(&key).as_ref(), Some(&output));
    }

    #[test]
    fn returns_none_for_missing_key() {
        let mut cache = ToolResultCache::new(10);
        let key = CacheKey::new("grep_file", "pattern=test", "/workspace");
        assert_eq!(cache.get(&key), None);
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut cache = ToolResultCache::new(3);

        let key1 = CacheKey::new("tool", "p1", "/a");
        let key2 = CacheKey::new("tool", "p2", "/b");
        let key3 = CacheKey::new("tool", "p3", "/c");
        let key4 = CacheKey::new("tool", "p4", "/d");

        cache.insert(key1.clone(), "out1".to_string());
        cache.insert(key2.clone(), "out2".to_string());
        cache.insert(key3.clone(), "out3".to_string());

        // Cache is full, adding key4 should evict key1
        cache.insert(key4.clone(), "out4".to_string());

        assert_eq!(cache.get(&key1), None);
        assert_eq!(cache.get(&key2), Some("out2".to_string()));
    }

    #[test]
    fn invalidates_by_path() {
        let mut cache = ToolResultCache::new(10);

        let key1 = CacheKey::new("tool", "p1", "/workspace/file1.rs");
        let key2 = CacheKey::new("tool", "p2", "/workspace/file2.rs");
        let key3 = CacheKey::new("tool", "p3", "/other/file3.rs");

        cache.insert(key1.clone(), "out1".to_string());
        cache.insert(key2.clone(), "out2".to_string());
        cache.insert(key3.clone(), "out3".to_string());

        cache.invalidate_for_path("/workspace/file1.rs");

        assert_eq!(cache.get(&key1), None);
        assert_eq!(cache.get(&key2), Some("out2".to_string()));
        assert_eq!(cache.get(&key3), Some("out3".to_string()));
    }

    #[test]
    fn tracks_access_count() {
        let mut cache = ToolResultCache::new(10);
        let key = CacheKey::new("tool", "p1", "/a");

        cache.insert(key.clone(), "output".to_string());
        assert_eq!(cache.results[&key].access_count, 0);

        cache.get(&key);
        assert_eq!(cache.results[&key].access_count, 1);

        cache.get(&key);
        assert_eq!(cache.results[&key].access_count, 2);
    }

    #[test]
    fn clears_cache() {
        let mut cache = ToolResultCache::new(10);
        let key = CacheKey::new("tool", "p1", "/a");

        cache.insert(key.clone(), "output".to_string());
        assert_eq!(cache.results.len(), 1);

        cache.clear();
        assert_eq!(cache.results.len(), 0);
        assert_eq!(cache.get(&key), None);
    }

    #[test]
    fn computes_stats() {
        let mut cache = ToolResultCache::new(10);

        let key1 = CacheKey::new("tool", "p1", "/a");
        let key2 = CacheKey::new("tool", "p2", "/b");

        cache.insert(key1.clone(), "out1".to_string());
        cache.insert(key2.clone(), "out2".to_string());
        cache.get(&key1);
        cache.get(&key2);
        cache.get(&key1);

        let stats = cache.stats();
        assert_eq!(stats.size, 2);
        assert_eq!(stats.capacity, 10);
        assert_eq!(stats.utilization, 20);
        assert_eq!(stats.total_accesses, 3);
    }

    #[test]
    fn insert_arc_and_get_arc() {
        let mut cache = ToolResultCache::new(10);
        let key = CacheKey::new("tool", "p1", "/a");
        let arc = Arc::new("output".to_string());
        cache.insert_arc(key.clone(), Arc::clone(&arc));
        assert_eq!(cache.get_arc(&key).unwrap(), arc);
    }
}
