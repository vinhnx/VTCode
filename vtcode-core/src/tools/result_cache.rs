//! Tool result caching for read-only operations
//!
//! Caches results from read-only tools (grep, list_files, ast analysis) within a session
//! to avoid re-running identical queries.

use crate::cache::{CacheKey as UnifiedCacheKey, UnifiedCache, EvictionPolicy, DEFAULT_CACHE_TTL};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

/// Identifies a cached tool result
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ToolCacheKey {
    /// Tool name (e.g., "grep_file", "list_files")
    pub tool: String,
    /// Normalized parameters (serialized, hashed for speed)
    pub params_hash: u64,
    /// File/path being analyzed
    pub target_path: String,
}

impl UnifiedCacheKey for ToolCacheKey {
    fn to_cache_key(&self) -> String {
        format!("{}:{}:{}", self.tool, self.params_hash, self.target_path)
    }
}

impl ToolCacheKey {
    /// Create a new cache key from tool name, parameters, and target path
    #[inline]
    pub fn new(tool: &str, params: &str, target_path: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        params.hash(&mut hasher);
        let params_hash = hasher.finish();

        ToolCacheKey {
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
        ToolCacheKey {
            tool: tool.to_string(),
            params_hash,
            target_path: target_path.to_string(),
        }
    }
}

/// Cached tool result - using String directly as the cache value
pub type ToolCacheValue = String;



/// Tool result cache with LRU eviction - now using unified cache
pub struct ToolResultCache {
    inner: UnifiedCache<ToolCacheKey, String>,
}

impl ToolResultCache {
    /// Create a new cache with specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: UnifiedCache::new(capacity, DEFAULT_CACHE_TTL, EvictionPolicy::Lru),
        }
    }

    /// Create with custom TTL
    pub fn with_ttl(capacity: usize, ttl_secs: u64) -> Self {
        Self {
            inner: UnifiedCache::new(capacity, Duration::from_secs(ttl_secs), EvictionPolicy::Lru),
        }
    }

    /// Insert a result into the cache
    pub fn insert(&mut self, key: ToolCacheKey, output: String) {
        let size_bytes = std::mem::size_of_val(&output) as u64;
        self.inner.insert(key, output, size_bytes);
    }

    /// Insert an Arc-wrapped result into the cache to avoid cloning when the caller
    /// already has an Arc<String> available.
    pub fn insert_arc(&mut self, key: ToolCacheKey, output: Arc<String>) {
        let size_bytes = std::mem::size_of_val(&**output) as u64;
        self.inner.insert(key, (*output).clone(), size_bytes);
    }

    /// Retrieve a result if cached and fresh - now returns zero-copy Arc by default
    pub fn get(&mut self, key: &ToolCacheKey) -> Option<Arc<String>> {
        self.inner.get(key)
    }

    /// Get owned value (explicitly clones when needed)
    pub fn get_owned(&mut self, key: &ToolCacheKey) -> Option<String> {
        self.inner.get_owned(key)
    }

    /// Clear cache entries for a specific file
    pub fn invalidate_for_path(&mut self, _path: &str) {
        // For now, clear entire cache - in future, implement selective eviction
        // This is complex with the unified cache structure
        self.inner.clear();
    }

    /// Clear entire cache
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> crate::cache::CacheStats {
        self.inner.stats().clone()
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_cache_key() {
        let key = ToolCacheKey::new("grep_file", "pattern=test", "/workspace");
        assert_eq!(key.tool, "grep_file");
        assert_eq!(key.target_path, "/workspace");
    }

    #[test]
    fn from_json_and_new_equivalence() {
        let params = serde_json::json!({"a": 1, "b": [1,2,3]});
        let params_str = serde_json::to_string(&params).unwrap();
        let k1 = ToolCacheKey::new("tool", &params_str, "/workspace");
        let k2 = ToolCacheKey::from_json("tool", &params, "/workspace");
        assert_eq!(k1, k2);
    }

    #[test]
    fn caches_and_retrieves_result() {
        let mut cache = ToolResultCache::new(10);
        let key = ToolCacheKey::new("grep_file", "pattern=test", "/workspace");
        let output = "line 1\nline 2".to_string();

        cache.insert_arc(key.clone(), Arc::new(output.clone()));
        assert_eq!(cache.get(&key).as_ref(), Some(&Arc::new(output)));
    }

    #[test]
    fn returns_none_for_missing_key() {
        let mut cache = ToolResultCache::new(10);
        let key = ToolCacheKey::new("grep_file", "pattern=test", "/workspace");
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut cache = ToolResultCache::new(3);

        let key1 = ToolCacheKey::new("tool", "p1", "/a");
        let key2 = ToolCacheKey::new("tool", "p2", "/b");
        let key3 = ToolCacheKey::new("tool", "p3", "/c");
        let key4 = ToolCacheKey::new("tool", "p4", "/d");

        cache.insert(key1.clone(), "out1".to_string());
        cache.insert(key2.clone(), "out2".to_string());
        cache.insert(key3.clone(), "out3".to_string());

        // Cache is full, adding key4 should evict key1
        cache.insert(key4.clone(), "out4".to_string());

        assert!(cache.get(&key1).is_none());
        assert_eq!(cache.get(&key2).unwrap().as_ref(), "out2");
    }

    #[test]
    fn invalidates_by_path() {
        let mut cache = ToolResultCache::new(10);

        let key1 = ToolCacheKey::new("tool", "p1", "/workspace/file1.rs");
        let key2 = ToolCacheKey::new("tool", "p2", "/workspace/file2.rs");
        let key3 = ToolCacheKey::new("tool", "p3", "/other/file3.rs");

        cache.insert(key1.clone(), "out1".to_string());
        cache.insert(key2.clone(), "out2".to_string());
        cache.insert(key3.clone(), "out3".to_string());

        cache.invalidate_for_path("/workspace/file1.rs");

        assert!(cache.get(&key1).is_none());
        assert_eq!(cache.get(&key2).unwrap().as_ref(), "out2");
        assert_eq!(cache.get(&key3).unwrap().as_ref(), "out3");
    }

    #[test]
    fn tracks_access_count() {
        let mut cache = ToolResultCache::new(10);
        let key = ToolCacheKey::new("tool", "p1", "/a");

        cache.insert(key.clone(), "output".to_string());
        let initial_stats = cache.stats();

        cache.get(&key);
        cache.get(&key);
        
        let final_stats = cache.stats();
        assert!(final_stats.hits > initial_stats.hits);
    }

    #[test]
    fn clears_cache() {
        let mut cache = ToolResultCache::new(10);
        let key = ToolCacheKey::new("tool", "p1", "/a");

        cache.insert(key.clone(), "output".to_string());
        assert_eq!(cache.stats().current_size, 1);

        cache.clear();
        assert_eq!(cache.stats().current_size, 0);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn computes_stats() {
        let mut cache = ToolResultCache::new(10);

        let key1 = ToolCacheKey::new("tool", "p1", "/a");
        let key2 = ToolCacheKey::new("tool", "p2", "/b");

        cache.insert(key1.clone(), "out1".to_string());
        cache.insert(key2.clone(), "out2".to_string());
        cache.get(&key1);
        cache.get(&key2);
        cache.get(&key1);

        let stats = cache.stats();
        assert_eq!(stats.current_size, 2);
        assert_eq!(stats.max_size, 10);
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 2); // 2 from initial gets
    }

    #[test]
    fn insert_arc_and_get_arc() {
        let mut cache = ToolResultCache::new(10);
        let key = ToolCacheKey::new("tool", "p1", "/a");
        let arc = Arc::new("output".to_string());
        cache.insert_arc(key.clone(), Arc::clone(&arc));
        assert_eq!(cache.get(&key).unwrap(), arc);
    }
}
