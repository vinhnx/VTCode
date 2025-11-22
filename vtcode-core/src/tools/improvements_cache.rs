//! Production-ready LRU cache with TTL enforcement
//!
//! - Thread-safe concurrent access via Arc<RwLock>
//! - LRU eviction policy with configurable max size
//! - TTL enforcement (automatic and on-access)
//! - Observability hooks for cache operations
//! - Async-compatible for tokio-based systems

use crate::tools::improvements_errors::{EventType, ObservabilityContext};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Cache entry with TTL and LRU metadata
#[derive(Clone)]
struct CacheEntry<V> {
    value: V,
    created_at: Instant,
    last_accessed: Instant,
    ttl: Duration,
}

impl<V> CacheEntry<V> {
    fn new(value: V, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.last_accessed.elapsed() > self.ttl
    }

    fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// LRU cache with TTL support
///
/// Thread-safe, configurable size, automatic TTL enforcement.
pub struct LruCache<K: Clone + Eq + std::hash::Hash, V: Clone> {
    max_size: usize,
    default_ttl: Duration,
    entries: Arc<RwLock<HashMap<K, CacheEntry<V>>>>,
    obs_context: Arc<ObservabilityContext>,
}

impl<K: Clone + Eq + std::hash::Hash + std::fmt::Debug, V: Clone + std::fmt::Debug> LruCache<K, V> {
    /// Create new cache
    pub fn new(max_size: usize, default_ttl: Duration) -> Self {
        Self {
            max_size,
            default_ttl,
            entries: Arc::new(RwLock::new(HashMap::new())),
            obs_context: Arc::new(ObservabilityContext::noop()),
        }
    }

    /// With observability context
    pub fn with_observability(mut self, obs: Arc<ObservabilityContext>) -> Self {
        self.obs_context = obs;
        self
    }

    /// Get value from cache (updates LRU metadata)
    pub fn get(&self, key: &K) -> Option<V> {
        let mut entries = self.entries.write().unwrap();

        // Check if key exists
        if entries.contains_key(key) {
            let entry = entries.get(key).unwrap();

            // Check TTL
            if entry.is_expired() {
                let age = entry.age();
                entries.remove(key);
                self.obs_context.event(
                    EventType::CacheEvicted,
                    "cache",
                    format!("entry expired after {:?}", age),
                    None,
                );
                return None;
            }

            // Get value and update LRU
            let value = entry.value.clone();
            if let Some(entry) = entries.get_mut(key) {
                entry.last_accessed = Instant::now();
            }

            self.obs_context
                .event(EventType::CacheHit, "cache", "cache hit", Some(1.0));

            Some(value)
        } else {
            self.obs_context
                .event(EventType::CacheMiss, "cache", "cache miss", Some(0.0));
            None
        }
    }

    /// Put value in cache
    pub fn put(&self, key: K, value: V) -> Result<(), String> {
        let mut entries = self.entries.write().unwrap();

        // Evict if at capacity
        if entries.len() >= self.max_size && !entries.contains_key(&key) {
            // Find least recently used entry
            if let Some(lru_key) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&lru_key);
                self.obs_context
                    .event(EventType::CacheEvicted, "cache", "evicted LRU entry", None);
            }
        }

        entries.insert(key, CacheEntry::new(value, self.default_ttl));
        Ok(())
    }

    /// Get cache size
    pub fn size(&self) -> usize {
        let entries = self.entries.read().unwrap();
        entries.len()
    }

    /// Clear expired entries
    pub fn evict_expired(&self) -> usize {
        let mut entries = self.entries.write().unwrap();
        let before = entries.len();

        entries.retain(|_, entry| !entry.is_expired());

        let evicted = before - entries.len();
        if evicted > 0 {
            self.obs_context.event(
                EventType::CacheEvicted,
                "cache",
                format!("evicted {} expired entries", evicted),
                None,
            );
        }
        evicted
    }

    /// Clear all entries
    pub fn clear(&self) {
        self.entries.write().unwrap().clear();
    }

    /// Get cache stats
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.read().unwrap();
        let expired_count = entries.values().filter(|e| e.is_expired()).count();

        CacheStats {
            total_entries: entries.len(),
            max_size: self.max_size,
            expired_entries: expired_count,
            utilization_percent: (entries.len() as f32 / self.max_size as f32) * 100.0,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub max_size: usize,
    pub expired_entries: usize,
    pub utilization_percent: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic_get_put() {
        let cache = LruCache::new(10, Duration::from_secs(60));

        cache.put("key1", "value1").unwrap();
        assert_eq!(cache.get(&"key1"), Some("value1"));
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let cache = LruCache::new(10, Duration::from_millis(100));

        cache.put("key1", "value1").unwrap();
        assert_eq!(cache.get(&"key1"), Some("value1"));

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(cache.get(&"key1"), None);
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache = LruCache::new(3, Duration::from_secs(60));

        // Fill cache
        cache.put("key1", "value1").unwrap();
        cache.put("key2", "value2").unwrap();
        cache.put("key3", "value3").unwrap();

        // Access key1 to mark it recently used
        cache.get(&"key1");

        // Add new entry (should evict key2 as LRU)
        cache.put("key4", "value4").unwrap();

        assert_eq!(cache.get(&"key1"), Some("value1"));
        assert_eq!(cache.get(&"key2"), None); // Evicted
        assert_eq!(cache.get(&"key3"), Some("value3"));
        assert_eq!(cache.get(&"key4"), Some("value4"));
    }

    #[test]
    fn test_cache_stats() {
        let cache = LruCache::new(10, Duration::from_secs(60));

        cache.put("key1", "value1").unwrap();
        cache.put("key2", "value2").unwrap();

        let stats = cache.stats();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.max_size, 10);
        assert!(stats.utilization_percent > 15.0 && stats.utilization_percent < 25.0);
    }

    #[test]
    fn test_cache_clear() {
        let cache = LruCache::new(10, Duration::from_secs(60));

        cache.put("key1", "value1").unwrap();
        assert_eq!(cache.size(), 1);

        cache.clear();
        assert_eq!(cache.size(), 0);
    }

    #[test]
    fn test_cache_evict_expired() {
        let cache = LruCache::new(10, Duration::from_millis(100));

        cache.put("key1", "value1").unwrap();
        cache.put("key2", "value2").unwrap();

        std::thread::sleep(Duration::from_millis(150));

        let evicted = cache.evict_expired();
        assert_eq!(evicted, 2);
        assert_eq!(cache.size(), 0);
    }
}
