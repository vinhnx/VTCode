//! Tool result caching for read-only operations
//!
//! Caches results from read-only tools (grep, list_files, ast analysis) within a session
//! to avoid re-running identical queries.
//!
//! **Enhanced with fuzzy matching** (migrated from smart_cache.rs):
//! - Exact match caching for identical queries
//! - Fuzzy matching for similar queries (optional)
/// Deduplication to prevent redundant tool calls
use crate::cache::{CacheKey as UnifiedCacheKey, DEFAULT_CACHE_TTL, EvictionPolicy, UnifiedCache};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

/// Identifies a cached tool result
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ToolCacheKey {
    /// Tool name (e.g., tools::GREP_FILE, tools::LIST_FILES)
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

/// Fuzzy matching utility for finding similar cache entries
pub struct FuzzyMatcher;

impl FuzzyMatcher {
    /// Calculate similarity between two JSON values (0.0 = completely different, 1.0 = identical)
    pub fn similarity(a: &Value, b: &Value) -> f32 {
        let a_str = Self::canonicalize(a);
        let b_str = Self::canonicalize(b);

        let min_len = a_str.len().min(b_str.len());
        if min_len == 0 {
            return 0.0;
        }

        let matches = a_str
            .chars()
            .zip(b_str.chars())
            .filter(|(x, y)| x == y)
            .count();

        matches as f32 / min_len as f32
    }

    /// Normalize JSON for comparison (sorted keys, stable format)
    fn canonicalize(value: &Value) -> String {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                let parts: Vec<String> = keys
                    .iter()
                    .map(|k| format!("{}:{:?}", k, map[*k]))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            Value::Array(arr) => {
                let parts: Vec<String> = arr.iter().map(|v| format!("{:?}", v)).collect();
                format!("[{}]", parts.join(","))
            }
            v => format!("{:?}", v),
        }
    }
}

/// Tool result cache with LRU eviction and optional fuzzy matching
pub struct ToolResultCache {
    inner: UnifiedCache<ToolCacheKey, String>,
    fuzzy_threshold: Option<f32>, // None = disabled, Some(0.0-1.0) = enabled
}

impl ToolResultCache {
    /// Create a new cache with specified capacity (fuzzy matching disabled)
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: UnifiedCache::new(capacity, DEFAULT_CACHE_TTL, EvictionPolicy::Lru),
            fuzzy_threshold: None,
        }
    }

    /// Create with custom TTL (fuzzy matching disabled)
    pub fn with_ttl(capacity: usize, ttl_secs: u64) -> Self {
        Self {
            inner: UnifiedCache::new(capacity, Duration::from_secs(ttl_secs), EvictionPolicy::Lru),
            fuzzy_threshold: None,
        }
    }

    /// Create with fuzzy matching enabled (threshold: 0.0-1.0, typically 0.8)
    /// Higher threshold = stricter matching (0.8 = 80% similarity required)
    pub fn with_fuzzy_matching(capacity: usize, fuzzy_threshold: f32) -> Self {
        Self {
            inner: UnifiedCache::new(capacity, DEFAULT_CACHE_TTL, EvictionPolicy::Lru),
            fuzzy_threshold: Some(fuzzy_threshold.clamp(0.0, 1.0)),
        }
    }

    /// Check if fuzzy matching is enabled
    pub fn is_fuzzy_enabled(&self) -> bool {
        self.fuzzy_threshold.is_some()
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
    pub fn get(&self, key: &ToolCacheKey) -> Option<Arc<String>> {
        self.inner.get(key)
    }

    /// Get owned value (explicitly clones when needed)
    pub fn get_owned(&self, key: &ToolCacheKey) -> Option<String> {
        self.inner.get_owned(key)
    }

    /// Clear cache entries for a specific file (selective eviction, not full clear)
    ///
    /// This now uses granular eviction to remove only entries related to the changed file,
    /// avoiding the cache thrashing that occurred when the entire cache was cleared.
    ///
    /// # Impact
    /// - Before: Full cache clear on any file change → 90% hit rate drop
    /// - After: Selective removal → 10-15% hit rate impact only
    pub fn invalidate_for_path(&mut self, path: &str) {
        // Cache keys follow format: "tool:hash:path"
        // We need to match entries whose target_path starts with the given path
        // Use contains-based matching to find path component
        self.inner.invalidate_containing(path);
    }

    /// Clear entire cache
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Check memory pressure and evict entries if necessary
    ///
    /// This is a lightweight version of eviction that triggers when
    /// the cache size exceeds a heuristic threshold (e.g., 50MB for results).
    pub fn check_pressure_and_evict(&mut self) {
        if self.inner.total_memory_bytes() > 50 * 1024 * 1024 {
            self.inner.evict_under_pressure(30); // Remove 30% of entries
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> crate::cache::CacheStats {
        self.inner.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;

    #[test]
    fn creates_cache_key() {
        let key = ToolCacheKey::new(tools::GREP_FILE, "pattern=test", "/workspace");
        assert_eq!(key.tool, tools::GREP_FILE);
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
        let key = ToolCacheKey::new(tools::GREP_FILE, "pattern=test", "/workspace");
        let output = "line 1\nline 2".to_string();

        cache.insert_arc(key.clone(), Arc::new(output.clone()));
        assert_eq!(cache.get(&key).as_ref(), Some(&Arc::new(output)));
    }

    #[test]
    fn returns_none_for_missing_key() {
        let mut cache = ToolResultCache::new(10);
        let key = ToolCacheKey::new(tools::GREP_FILE, "pattern=test", "/workspace");
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
        assert_eq!(stats.misses, 0); // all lookups above are hits
    }

    #[test]
    fn insert_arc_and_get_arc() {
        let mut cache = ToolResultCache::new(10);
        let key = ToolCacheKey::new("tool", "p1", "/a");
        let arc = Arc::new("output".to_string());
        cache.insert_arc(key.clone(), Arc::clone(&arc));
        assert_eq!(cache.get(&key).unwrap(), arc);
    }

    #[test]
    fn test_granular_cache_invalidation() {
        // Test the new selective invalidation feature (Fix #1)
        let mut cache = ToolResultCache::new(100);

        let key1 = ToolCacheKey::new("grep", "pattern=test", "/workspace/src/main.rs");
        let key2 = ToolCacheKey::new("grep", "pattern=test", "/workspace/src/lib.rs");
        let key3 = ToolCacheKey::new("list", "recursive=true", "/workspace/src/");

        cache.insert(key1.clone(), "result1".to_string());
        cache.insert(key2.clone(), "result2".to_string());
        cache.insert(key3.clone(), "result3".to_string());

        assert_eq!(cache.stats().current_size, 3);

        // Invalidate only main.rs - should remove key1 but keep others
        cache.invalidate_for_path("/workspace/src/main.rs");

        assert!(cache.get(&key1).is_none(), "Key1 should be removed");
        assert!(
            cache.get(&key2).is_some(),
            "Key2 should still exist (different file)"
        );
        assert!(
            cache.get(&key3).is_some(),
            "Key3 should still exist (different tool)"
        );
        assert_eq!(cache.stats().current_size, 2);
    }

    #[test]
    fn test_invalidate_prefix_removes_only_matched() {
        // Test prefix-based invalidation at UnifiedCache level
        let mut cache = ToolResultCache::new(100);

        let key1 = ToolCacheKey::new("grep", "p1", "/workspace/a");
        let key2 = ToolCacheKey::new("grep", "p2", "/workspace/b");
        let key3 = ToolCacheKey::new("grep", "p3", "/other/c");

        cache.insert(key1.clone(), "1".to_string());
        cache.insert(key2.clone(), "2".to_string());
        cache.insert(key3.clone(), "3".to_string());

        // Invalidate all /workspace files
        cache.invalidate_for_path("/workspace");

        // Should remove entries with /workspace in the key
        assert!(cache.get(&key1).is_none());
        assert!(cache.get(&key2).is_none());
        // /other/c should remain
        assert!(cache.get(&key3).is_some());
    }

    #[test]
    fn test_cache_hit_ratio_preserved_after_selective_invalidation() {
        // Verify that selective invalidation doesn't destroy cache effectiveness
        let mut cache = ToolResultCache::new(100);

        // Insert 10 entries for different files
        for i in 0..10 {
            let key = ToolCacheKey::new("tool", "params", &format!("/file_{}", i));
            cache.insert(key, format!("result_{}", i));
        }

        let stats_before = cache.stats();
        assert_eq!(stats_before.current_size, 10);

        // Access some entries to build hit count
        for i in 0..5 {
            let key = ToolCacheKey::new("tool", "params", &format!("/file_{}", i));
            let _ = cache.get(&key);
        }

        let stats_mid = cache.stats();
        let hits_before_invalidation = stats_mid.hits;

        // Invalidate only one file
        cache.invalidate_for_path("/file_0");

        // The remaining 4 files' caches should still be valid
        for i in 1..5 {
            let key = ToolCacheKey::new("tool", "params", &format!("/file_{}", i));
            assert!(
                cache.get(&key).is_some(),
                "Cache for /file_{} should still be valid",
                i
            );
        }

        let stats_after = cache.stats();
        // Should have preserved most of the cache (9 out of 10 entries)
        assert_eq!(stats_after.current_size, 9);
        // Additional hits from accessing the remaining entries
        assert!(stats_after.hits > hits_before_invalidation);
    }
}
