//! Unified caching system for VT Code
//!
//! This module provides a consolidated caching framework that replaces
//! the multiple duplicate cache implementations throughout the codebase.
//!
//! Uses interior mutability with `RwLock` to allow `&self` methods,
//! following the pattern from matklad's "Caches in Rust" article.

use anyhow::Result;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Default TTL for cache entries (2 minutes for memory-constrained environments)
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(120);

/// Default maximum cache capacity (reduced from 10,000 to 1,000 for memory efficiency)
pub const DEFAULT_MAX_CACHE_CAPACITY: usize = 1_000;

/// Maximum number of items to return in context-limited operations
pub const MAX_CONTEXT_ITEMS: usize = 5;

/// Unified cache key trait for all cache types
pub trait CacheKey: Send + Sync + std::hash::Hash + Eq + Clone + 'static {
    fn to_cache_key(&self) -> String;
}

/// Unified cache value trait
pub trait CacheValue: Send + Sync + Clone + 'static {}

impl<T> CacheValue for T where T: Send + Sync + Clone + 'static {}

/// Cache statistics with consistent structure across all cache types
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub current_size: usize,
    pub max_size: usize,
    pub total_memory_bytes: u64,
}

/// Cache entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry<V> {
    pub value: Arc<V>,
    pub created_at: SystemTime,
    pub last_accessed: SystemTime,
    pub access_count: u64,
    pub size_bytes: u64,
}

impl<V> CacheEntry<V> {
    pub fn new(value: V, size_bytes: u64) -> Self {
        let now = SystemTime::now();
        Self {
            value: Arc::new(value),
            created_at: now,
            last_accessed: now,
            access_count: 1,
            size_bytes,
        }
    }

    pub fn mark_accessed(&mut self) {
        self.last_accessed = SystemTime::now();
        self.access_count += 1;
    }

    pub fn is_expired(&self, ttl: Duration) -> bool {
        SystemTime::now()
            .duration_since(self.created_at)
            .map(|age| age > ttl)
            .unwrap_or(true)
    }
}

/// Unified cache backend with configurable eviction policies
///
/// Uses interior mutability via `RwLock` to allow `&self` methods,
/// enabling easier use in concurrent contexts without borrow checker conflicts.
pub struct UnifiedCache<K, V> {
    inner: RwLock<UnifiedCacheInner<K, V>>,
}

/// Internal state for `UnifiedCache`, protected by `RwLock`
struct UnifiedCacheInner<K, V> {
    entries: FxHashMap<K, CacheEntry<V>>,
    max_size: usize,
    ttl: Duration,
    stats: CacheStats,
    eviction_policy: EvictionPolicy,
}

#[derive(Debug, Clone, Copy)]
pub enum EvictionPolicy {
    /// Least Recently Used
    Lru,
    /// Least Frequently Used
    Lfu,
    /// First In, First Out
    Fifo,
    /// Time-based expiration only
    TtlOnly,
}

impl<K, V> UnifiedCache<K, V>
where
    K: CacheKey,
    V: CacheValue,
{
    pub fn new(max_size: usize, ttl: Duration, eviction_policy: EvictionPolicy) -> Self {
        Self {
            inner: RwLock::new(UnifiedCacheInner {
                entries: FxHashMap::with_capacity_and_hasher(max_size, Default::default()),
                max_size,
                ttl,
                stats: CacheStats {
                    max_size,
                    ..Default::default()
                },
                eviction_policy,
            }),
        }
    }

    /// Get value from cache with zero-copy access by default.
    ///
    /// Uses a read-first fast path and only attempts a non-blocking write lock
    /// for best-effort metadata/stat updates.
    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        enum LookupState<T> {
            Hit(Arc<T>),
            Expired,
            Miss,
        }

        let state = {
            let inner = self.inner.read().ok()?;
            let ttl = inner.ttl;

            match inner.entries.get(key) {
                Some(entry) if !entry.is_expired(ttl) => LookupState::Hit(Arc::clone(&entry.value)),
                Some(_) => LookupState::Expired,
                None => LookupState::Miss,
            }
        };

        match state {
            LookupState::Hit(value) => {
                if let Ok(mut inner) = self.inner.try_write() {
                    if let Some(entry) = inner.entries.get_mut(key) {
                        entry.mark_accessed();
                    }
                    inner.stats.hits += 1;
                }
                Some(value)
            }
            LookupState::Expired => {
                if let Ok(mut inner) = self.inner.try_write() {
                    let ttl = inner.ttl;
                    let should_remove = inner
                        .entries
                        .get(key)
                        .map(|entry| entry.is_expired(ttl))
                        .unwrap_or(false);
                    if should_remove {
                        Self::remove_inner(&mut inner, key);
                    }
                    inner.stats.misses += 1;
                }
                None
            }
            LookupState::Miss => {
                if let Ok(mut inner) = self.inner.try_write() {
                    inner.stats.misses += 1;
                }
                None
            }
        }
    }

    /// Get owned value (explicitly clones when needed)
    pub fn get_owned(&self, key: &K) -> Option<V> {
        self.get(key).map(|arc| (*arc).clone())
    }

    /// Insert value into cache with automatic eviction
    pub fn insert(&self, key: K, value: V, size_bytes: u64) {
        let Ok(mut inner) = self.inner.write() else {
            return;
        };

        // Remove expired entries first
        Self::remove_expired_entries_inner(&mut inner);

        // Batch evict if over capacity: remove 10% of entries at once
        // to avoid repeated O(n) scans on consecutive inserts
        if inner.entries.len() >= inner.max_size {
            let to_remove = (inner.max_size / 10).max(1);
            Self::evict_batch_inner(&mut inner, to_remove);
        }

        let entry = CacheEntry::new(value, size_bytes);
        inner.entries.insert(key, entry);
        inner.stats.current_size = inner.entries.len();
        inner.stats.total_memory_bytes += size_bytes;
    }

    /// Remove expired entries based on TTL
    fn remove_expired_entries_inner(inner: &mut UnifiedCacheInner<K, V>) {
        let expired_keys: Vec<K> = inner
            .entries
            .iter()
            .filter_map(|(k, v)| {
                if v.is_expired(inner.ttl) {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();

        for key in expired_keys {
            Self::remove_inner(inner, &key);
        }
    }

    /// Evict one entry based on the eviction policy
    fn evict_one_inner(inner: &mut UnifiedCacheInner<K, V>) {
        if inner.entries.is_empty() {
            return;
        }

        let key_to_remove = match inner.eviction_policy {
            EvictionPolicy::Lru => Self::find_lru_entry_inner(inner),
            EvictionPolicy::Lfu => Self::find_lfu_entry_inner(inner),
            EvictionPolicy::Fifo => Self::find_fifo_entry_inner(inner),
            EvictionPolicy::TtlOnly => Self::find_oldest_entry_inner(inner),
        };

        if let Some(key) = key_to_remove {
            Self::remove_inner(inner, &key);
            inner.stats.evictions += 1;
        }
    }

    /// Batch-evict `count` entries based on the eviction policy.
    /// Performs a single O(n log n) sort instead of `count` × O(n) linear scans.
    fn evict_batch_inner(inner: &mut UnifiedCacheInner<K, V>, count: usize) {
        if inner.entries.is_empty() {
            return;
        }

        let keys_to_remove: Vec<K> = match inner.eviction_policy {
            EvictionPolicy::Lru => {
                let mut entries: Vec<_> = inner
                    .entries
                    .iter()
                    .map(|(k, e)| (k.clone(), e.last_accessed))
                    .collect();
                entries.sort_by_key(|(_, ts)| *ts);
                entries.into_iter().take(count).map(|(k, _)| k).collect()
            }
            EvictionPolicy::Lfu => {
                let mut entries: Vec<_> = inner
                    .entries
                    .iter()
                    .map(|(k, e)| (k.clone(), e.access_count))
                    .collect();
                entries.sort_by_key(|(_, c)| *c);
                entries.into_iter().take(count).map(|(k, _)| k).collect()
            }
            EvictionPolicy::Fifo | EvictionPolicy::TtlOnly => {
                let mut entries: Vec<_> = inner
                    .entries
                    .iter()
                    .map(|(k, e)| (k.clone(), e.created_at))
                    .collect();
                entries.sort_by_key(|(_, ts)| *ts);
                entries.into_iter().take(count).map(|(k, _)| k).collect()
            }
        };

        for key in &keys_to_remove {
            Self::remove_inner(inner, key);
        }
        inner.stats.evictions += keys_to_remove.len() as u64;
    }

    fn find_lru_entry_inner(inner: &UnifiedCacheInner<K, V>) -> Option<K> {
        inner
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(k, _)| k.clone())
    }

    fn find_lfu_entry_inner(inner: &UnifiedCacheInner<K, V>) -> Option<K> {
        inner
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.access_count)
            .map(|(k, _)| k.clone())
    }

    fn find_fifo_entry_inner(inner: &UnifiedCacheInner<K, V>) -> Option<K> {
        inner
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(k, _)| k.clone())
    }

    fn find_oldest_entry_inner(inner: &UnifiedCacheInner<K, V>) -> Option<K> {
        Self::find_fifo_entry_inner(inner)
    }

    fn remove_inner(inner: &mut UnifiedCacheInner<K, V>, key: &K) {
        if let Some(entry) = inner.entries.remove(key) {
            inner.stats.total_memory_bytes -= entry.size_bytes;
            inner.stats.current_size = inner.entries.len();
        }
    }

    /// Get cache statistics (returns owned clone)
    pub fn stats(&self) -> CacheStats {
        self.inner
            .read()
            .map(|inner| inner.stats.clone())
            .unwrap_or_default()
    }

    /// Clear all entries
    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.entries.clear();
            inner.stats.current_size = 0;
            inner.stats.total_memory_bytes = 0;
        }
    }

    /// Get current size
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .map(|inner| inner.entries.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Invalidate cache entries matching a key prefix (selective eviction)
    /// This replaces the old "clear entire cache" behavior with granular eviction
    ///
    /// # Example
    /// ```ignore
    /// cache.invalidate_prefix("grep_file:/workspace/src/");
    /// // Only removes entries for that specific file, not entire cache
    /// ```
    pub fn invalidate_prefix(&self, prefix: &str) {
        let Ok(mut inner) = self.inner.write() else {
            return;
        };

        let keys_to_remove: Vec<K> = inner
            .entries
            .keys()
            .filter(|key| key.to_cache_key().starts_with(prefix))
            .cloned()
            .collect();

        for key in keys_to_remove {
            Self::remove_inner(&mut inner, &key);
        }
    }

    /// Invalidate entries for a specific target path (e.g., file path)
    /// This is a convenience wrapper for file-based invalidation
    ///
    /// # Example
    /// ```ignore
    /// cache.invalidate_path("/workspace/src/main.rs");
    /// // Removes all cache entries related to this file
    /// ```
    pub fn invalidate_path(&self, path: &str) {
        self.invalidate_prefix(&format!("{}:", path));
    }

    /// Invalidate cache entries matching a key suffix (selective eviction)
    ///
    /// # Example
    /// ```ignore
    /// cache.invalidate_suffix(":/workspace/src/main.rs");
    /// // Only removes entries for that specific file
    /// ```
    pub fn invalidate_suffix(&self, suffix: &str) {
        let Ok(mut inner) = self.inner.write() else {
            return;
        };

        let keys_to_remove: Vec<K> = inner
            .entries
            .keys()
            .filter(|key| key.to_cache_key().ends_with(suffix))
            .cloned()
            .collect();

        for key in keys_to_remove {
            Self::remove_inner(&mut inner, &key);
        }
    }

    /// Invalidate cache entries containing a substring (selective eviction)
    ///
    /// # Example
    /// ```ignore
    /// cache.invalidate_containing("/workspace/src/main.rs");
    /// // Removes entries where the cache key contains this path
    /// ```
    pub fn invalidate_containing(&self, substring: &str) {
        let Ok(mut inner) = self.inner.write() else {
            return;
        };

        let keys_to_remove: Vec<K> = inner
            .entries
            .keys()
            .filter(|key| key.to_cache_key().contains(substring))
            .cloned()
            .collect();

        for key in keys_to_remove {
            Self::remove_inner(&mut inner, &key);
        }
    }

    /// Get total memory used by cache in bytes
    pub fn total_memory_bytes(&self) -> u64 {
        self.inner
            .read()
            .map(|inner| inner.stats.total_memory_bytes)
            .unwrap_or(0)
    }

    /// Estimate entry cost in bytes (for memory-aware decisions)
    /// This is a heuristic based on entry metadata
    pub fn estimate_entry_cost(entry: &CacheEntry<V>) -> u64 {
        // Base: entry metadata + overhead
        let base_overhead: u64 = 100; // Approximate Arc, SystemTime, etc.
        let value_size = entry.size_bytes;
        base_overhead + value_size
    }

    /// Reduce TTL for all entries in cache (for pressure-based tuning)
    /// Returns the new TTL that was set
    pub fn reduce_ttl(&self, factor: f64) -> Duration {
        let Ok(mut inner) = self.inner.write() else {
            return Duration::ZERO;
        };
        let new_ttl = Duration::from_secs_f64(inner.ttl.as_secs_f64() * factor);
        inner.ttl = new_ttl;
        new_ttl
    }

    /// Evict entries under memory pressure (aggressive cleanup)
    ///
    /// When memory pressure increases:
    /// 1. Remove all expired entries first
    /// 2. Evict least useful entries until target percentage reached
    /// 3. Use access count and age for ranking
    pub fn evict_under_pressure(&self, target_reduction_percent: u32) -> usize {
        let Ok(mut inner) = self.inner.write() else {
            return 0;
        };

        // Clamp percentage to 0-100
        let target_percent = std::cmp::min(100, target_reduction_percent);

        // Remove expired entries first (most efficient cleanup)
        let expired_before = inner.entries.len();
        Self::remove_expired_entries_inner(&mut inner);
        let expired_removed = expired_before - inner.entries.len();

        // Calculate target size
        let current_size = inner.entries.len();
        let target_size = (current_size * (100 - target_percent) as usize) / 100;

        // Evict until we reach target
        let mut evicted_count = expired_removed;
        while inner.entries.len() > target_size && !inner.entries.is_empty() {
            Self::evict_one_inner(&mut inner);
            evicted_count += 1;
        }

        evicted_count
    }

    /// Clear a percentage of least-used entries (for aggressive cleanup under critical pressure)
    /// Returns number of entries removed
    pub fn clear_least_used(&self, percent_to_clear: u32) -> usize {
        let Ok(mut inner) = self.inner.write() else {
            return 0;
        };

        let percent = std::cmp::min(100, percent_to_clear);
        let entries_to_remove = (inner.entries.len() * percent as usize) / 100;

        let mut removed = 0usize;
        for _ in 0..entries_to_remove {
            if inner.entries.is_empty() {
                break;
            }
            Self::evict_one_inner(&mut inner);
            removed += 1;
        }

        removed
    }

    /// Get entries sorted by "usefulness" (access count and recency)
    /// Higher score = more useful (keep these)
    pub fn entries_by_usefulness(&self) -> Vec<(K, CacheEntry<V>)> {
        let Ok(inner) = self.inner.read() else {
            return Vec::new();
        };

        let now = SystemTime::now();
        let mut entries: Vec<(K, CacheEntry<V>, u64)> = inner
            .entries
            .iter()
            .map(|(k, entry)| {
                // Score = access_count * recency_factor
                let age_secs = now
                    .duration_since(entry.last_accessed)
                    .unwrap_or_default()
                    .as_secs();

                // Recency factor: recent entries get higher score
                let recency_factor = std::cmp::max(1, 3600 / (age_secs + 1));
                let usefulness_score = entry.access_count * recency_factor as u64;

                (k.clone(), entry.clone(), usefulness_score)
            })
            .collect();

        // Sort by usefulness descending (highest first)
        entries.sort_by_key(|(_, _, score)| std::cmp::Reverse(*score));
        entries.into_iter().map(|(k, e, _)| (k, e)).collect()
    }
}

/// Helper function to estimate JSON value size without allocation
pub fn estimate_json_size(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Null => 4,
        serde_json::Value::Bool(_) => 5,
        serde_json::Value::Number(n) => n.to_string().len() as u64,
        serde_json::Value::String(s) => s.len() as u64,
        serde_json::Value::Array(arr) => arr.iter().map(estimate_json_size).sum(),
        serde_json::Value::Object(obj) => obj
            .iter()
            .map(|(k, v)| k.len() as u64 + estimate_json_size(v) + 3) // +3 for quotes and colon
            .sum(),
    }
}

/// Helper function to create cache key from serializable data
pub fn create_cache_key<T: Serialize>(data: &T) -> Result<String> {
    let json_bytes = serde_json::to_vec(data)?;

    // Use a simple hash function instead of blake3 to avoid dependency
    let mut hash = 0u64;
    for (i, byte) in json_bytes.iter().enumerate() {
        hash = hash.wrapping_mul(31).wrapping_add(*byte as u64);
        hash = hash.rotate_left((i % 64) as u32);
    }

    Ok(format!("{:016x}", hash))
}

/// Context-aware cache that limits results to MAX_CONTEXT_ITEMS
pub struct ContextAwareCache<K, V> {
    inner: UnifiedCache<K, V>,
}

impl<K, V> ContextAwareCache<K, V>
where
    K: CacheKey,
    V: CacheValue,
{
    pub fn new(max_size: usize, ttl: Duration, eviction_policy: EvictionPolicy) -> Self {
        Self {
            inner: UnifiedCache::new(max_size, ttl, eviction_policy),
        }
    }

    /// Get results with automatic context limitation
    pub fn get_context_limited<F>(&self, keys: &[K], mut process_fn: F) -> Vec<V>
    where
        F: FnMut(&K) -> Option<V>,
    {
        let mut results = Vec::with_capacity(MAX_CONTEXT_ITEMS.min(keys.len()));
        let mut overflow_count = 0;

        for key in keys {
            if results.len() >= MAX_CONTEXT_ITEMS {
                overflow_count += 1;
                continue;
            }

            if let Some(value) = self.inner.get(key) {
                results.push((*value).clone());
            } else if let Some(value) = process_fn(key) {
                // Cache the result for future use
                let size = std::mem::size_of_val(&value) as u64;
                self.inner.insert(key.clone(), value.clone(), size);
                results.push(value);
            }
        }

        // Add overflow indication if needed
        if overflow_count > 0 {
            // This would need to be handled by the caller to add overflow indication
            // For now, we just limit the results
        }

        results
    }

    pub fn stats(&self) -> CacheStats {
        self.inner.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct TestKey(String);

    impl CacheKey for TestKey {
        fn to_cache_key(&self) -> String {
            self.0.clone()
        }
    }

    #[test]
    fn test_cache_basic_operations() {
        let cache = UnifiedCache::new(10, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);
        let key = TestKey("test".into());
        let value: String = "test_value".into();

        // Insert and retrieve
        cache.insert(key.clone(), value.clone(), 100);
        assert_eq!(*cache.get(&key).unwrap(), value);

        // Check stats
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.current_size, 1);
    }

    #[test]
    fn test_cache_expiration() {
        let cache = UnifiedCache::new(10, Duration::from_millis(100), EvictionPolicy::Lru);
        let key = TestKey("test".into());
        let value: String = "test_value".into();

        cache.insert(key.clone(), value, 100);
        assert!(cache.get(&key).is_some());

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_context_limited_cache() {
        let cache = ContextAwareCache::new(100, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);
        let keys: Vec<TestKey> = (0..10).map(|i| TestKey(i.to_string())).collect();

        let results = cache.get_context_limited(&keys, |key| Some(format!("value_{}", key.0)));

        // Should be limited to MAX_CONTEXT_ITEMS (5)
        assert_eq!(results.len(), MAX_CONTEXT_ITEMS);
        assert_eq!(results[0], "value_0");
        assert_eq!(results[4], "value_4");
    }

    #[test]
    fn test_pressure_aware_total_memory() {
        let cache = UnifiedCache::new(10, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);

        // Insert three entries with known sizes
        cache.insert(TestKey("k1".into()), "v1".to_string(), 100);
        cache.insert(TestKey("k2".into()), "v2".to_string(), 200);
        cache.insert(TestKey("k3".into()), "v3".to_string(), 300);

        // Total should be 600 bytes
        assert_eq!(cache.total_memory_bytes(), 600);
    }

    #[test]
    fn test_pressure_aware_reduce_ttl() {
        let cache: UnifiedCache<TestKey, String> =
            UnifiedCache::new(10, Duration::from_secs(300), EvictionPolicy::Lru);

        // Reduce by 40% (Warning pressure)
        let new_ttl = cache.reduce_ttl(0.4);
        assert_eq!(new_ttl.as_secs(), 120); // 300 * 0.4 = 120s

        // Reduce by 10% (Critical pressure)
        let new_ttl = cache.reduce_ttl(0.1);
        assert_eq!(new_ttl.as_secs(), 12); // 120 * 0.1 = 12s
    }

    #[test]
    fn test_pressure_aware_evict_under_pressure() {
        let cache: UnifiedCache<TestKey, String> =
            UnifiedCache::new(20, Duration::from_secs(3600), EvictionPolicy::Lru);

        // Insert 10 entries
        for i in 0..10 {
            cache.insert(TestKey(format!("key_{}", i)), format!("value_{}", i), 100);
        }

        assert_eq!(cache.len(), 10);

        // Evict to 50% (remove 5 entries)
        let removed = cache.evict_under_pressure(50);
        assert_eq!(removed, 5);
        assert_eq!(cache.len(), 5);
    }

    #[test]
    fn test_pressure_aware_clear_least_used() {
        let cache: UnifiedCache<TestKey, String> =
            UnifiedCache::new(20, Duration::from_secs(3600), EvictionPolicy::Lru);

        // Insert 10 entries
        for i in 0..10 {
            cache.insert(TestKey(format!("key_{}", i)), format!("value_{}", i), 100);
        }

        // Access some entries to mark them as used
        let _ = cache.get(&TestKey("key_0".into()));
        let _ = cache.get(&TestKey("key_1".into()));

        assert_eq!(cache.len(), 10);

        // Clear 30% least used
        let removed = cache.clear_least_used(30);
        assert!(removed <= 4, "Should remove ~3 entries (30% of 10)");
        assert!(cache.len() >= 6, "Should have ~7 entries left");
    }

    #[test]
    fn test_pressure_aware_entries_by_usefulness() {
        let cache: UnifiedCache<TestKey, String> =
            UnifiedCache::new(20, Duration::from_secs(3600), EvictionPolicy::Lru);

        // Insert and access entries with different patterns
        cache.insert(TestKey("hot".into()), "value".to_string(), 100);
        cache.insert(TestKey("cold".into()), "value".to_string(), 100);
        cache.insert(TestKey("warm".into()), "value".to_string(), 100);

        // Access "hot" multiple times
        for _ in 0..5 {
            let _ = cache.get(&TestKey("hot".into()));
        }

        // Access "warm" once
        let _ = cache.get(&TestKey("warm".into()));

        // "cold" is never accessed after insert

        let usefulness = cache.entries_by_usefulness();
        assert_eq!(usefulness.len(), 3);

        // "hot" should be first (most useful)
        assert_eq!(usefulness[0].0.0, "hot");
    }

    #[test]
    fn test_pressure_aware_estimate_entry_cost() {
        let entry = CacheEntry::new("test_value".to_string(), 1000);
        let cost = UnifiedCache::<TestKey, String>::estimate_entry_cost(&entry);

        // Cost should be at least the value size + overhead
        assert!(cost >= 1000);
        assert!(cost <= 1200); // Should be close to 1100 (1000 + 100 overhead)
    }
}
