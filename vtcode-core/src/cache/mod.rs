//! Unified caching system for VT Code
//! 
//! This module provides a consolidated caching framework that replaces
//! the multiple duplicate cache implementations throughout the codebase.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Default TTL for cache entries (5 minutes)
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub current_size: usize,
    pub max_size: usize,
    pub total_memory_bytes: u64,
}

impl Default for CacheStats {
    fn default() -> Self {
        Self {
            hits: 0,
            misses: 0,
            evictions: 0,
            current_size: 0,
            max_size: 0,
            total_memory_bytes: 0,
        }
    }
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
pub struct UnifiedCache<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
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
            entries: HashMap::with_capacity(max_size),
            max_size,
            ttl,
            stats: CacheStats {
                max_size,
                ..Default::default()
            },
            eviction_policy,
        }
    }

    /// Get value from cache with zero-copy access by default
    pub fn get(&mut self, key: &K) -> Option<Arc<V>> {
        match self.entries.get_mut(key) {
            Some(entry) => {
                if entry.is_expired(self.ttl) {
                    self.remove(key);
                    self.stats.misses += 1;
                    None
                } else {
                    entry.mark_accessed();
                    self.stats.hits += 1;
                    Some(Arc::clone(&entry.value))
                }
            }
            None => {
                self.stats.misses += 1;
                None
            }
        }
    }

    /// Get owned value (explicitly clones when needed)
    pub fn get_owned(&mut self, key: &K) -> Option<V> {
        self.get(key).map(|arc| (*arc).clone())
    }

    /// Insert value into cache with automatic eviction
    pub fn insert(&mut self, key: K, value: V, size_bytes: u64) {
        // Remove expired entries first
        self.remove_expired_entries();

        // Evict if necessary
        while self.entries.len() >= self.max_size {
            self.evict_one();
        }

        let entry = CacheEntry::new(value, size_bytes);
        self.entries.insert(key, entry);
        self.stats.current_size = self.entries.len();
        self.stats.total_memory_bytes += size_bytes;
    }

    /// Remove expired entries based on TTL
    fn remove_expired_entries(&mut self) {
        let expired_keys: Vec<K> = self
            .entries
            .iter()
            .filter_map(|(k, v)| if v.is_expired(self.ttl) { Some(k.clone()) } else { None })
            .collect();

        for key in expired_keys {
            self.remove(&key);
        }
    }

    /// Evict one entry based on the eviction policy
    fn evict_one(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let key_to_remove = match self.eviction_policy {
            EvictionPolicy::Lru => self.find_lru_entry(),
            EvictionPolicy::Lfu => self.find_lfu_entry(),
            EvictionPolicy::Fifo => self.find_fifo_entry(),
            EvictionPolicy::TtlOnly => self.find_oldest_entry(),
        };

        if let Some(key) = key_to_remove {
            self.remove(&key);
            self.stats.evictions += 1;
        }
    }

    fn find_lru_entry(&self) -> Option<K> {
        self.entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(k, _)| k.clone())
    }

    fn find_lfu_entry(&self) -> Option<K> {
        self.entries
            .iter()
            .min_by_key(|(_, entry)| entry.access_count)
            .map(|(k, _)| k.clone())
    }

    fn find_fifo_entry(&self) -> Option<K> {
        self.entries
            .iter()
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(k, _)| k.clone())
    }

    fn find_oldest_entry(&self) -> Option<K> {
        self.find_fifo_entry()
    }

    fn remove(&mut self, key: &K) {
        if let Some(entry) = self.entries.remove(key) {
            self.stats.total_memory_bytes -= entry.size_bytes;
            self.stats.current_size = self.entries.len();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
        self.stats.current_size = 0;
        self.stats.total_memory_bytes = 0;
    }

    /// Get current size
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
    pub fn get_context_limited<F>(&mut self, keys: &[K], mut process_fn: F) -> Vec<V>
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

    pub fn stats(&self) -> &CacheStats {
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
        let mut cache = UnifiedCache::new(10, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);
        let key = TestKey("test".to_string());
        let value = "test_value".to_string();

        // Insert and retrieve
        cache.insert(key.clone(), value.clone(), 100);
        assert_eq!(*cache.get(&key).unwrap(), value);

        // Check stats
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1); // One miss from initial get
        assert_eq!(stats.current_size, 1);
    }

    #[test]
    fn test_cache_expiration() {
        let mut cache = UnifiedCache::new(10, Duration::from_millis(100), EvictionPolicy::Lru);
        let key = TestKey("test".to_string());
        let value = "test_value".to_string();

        cache.insert(key.clone(), value, 100);
        assert!(cache.get(&key).is_some());

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_context_limited_cache() {
        let mut cache = ContextAwareCache::new(100, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);
        let keys: Vec<TestKey> = (0..10).map(|i| TestKey(i.to_string())).collect();

        let results = cache.get_context_limited(&keys, |key| {
            Some(format!("value_{}", key.0))
        });

        // Should be limited to MAX_CONTEXT_ITEMS (5)
        assert_eq!(results.len(), MAX_CONTEXT_ITEMS);
        assert_eq!(results[0], "value_0");
        assert_eq!(results[4], "value_4");
    }
}