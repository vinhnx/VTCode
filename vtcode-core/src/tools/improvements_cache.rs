//! **DEPRECATED**: Use `crate::cache::UnifiedCache` instead
//!
//! This module is deprecated as of Dec 2025. All usage has been migrated to UnifiedCache.
//!
#![allow(deprecated)]
//! **Migration completed**:
//! - [DONE] improvements_registry_ext.rs → UnifiedCache
//! - [DONE] async_middleware.rs → UnifiedCache
//! - [DONE] ui/tui/session/performance.rs → UnifiedCache
//!
//! **Why deprecated**: Duplicate implementation of LRU caching.
//! UnifiedCache provides the same functionality with better integration.
//!
//! **For new code**: Use `crate::cache::UnifiedCache` with `EvictionPolicy::Lru`
//!
//! ---
//! Original documentation (for reference):
//!
//! Production-ready LRU cache with TTL enforcement
//! - Thread-safe concurrent access via Arc<RwLock>
//! - LRU eviction policy with configurable max size
//! - TTL enforcement (automatic and on-access)
//! - Observability hooks for cache operations

use crate::tools::improvements_errors::{EventType, ObservabilityContext};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::warn;

/// Cache entry with TTL and LRU metadata
#[derive(Clone)]
struct CacheEntry<V> {
    value: Arc<V>,
    created_at: Instant,
    last_accessed: Instant,
    ttl: Duration,
}

impl<V> CacheEntry<V> {
    fn new(value: V, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            value: Arc::new(value),
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

/// **DEPRECATED**: Use `crate::cache::UnifiedCache` instead
///
/// LRU cache with TTL support (kept for backward compatibility only)
#[deprecated(since = "0.1.0", note = "Use crate::cache::UnifiedCache instead")]
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
    /// Return a shared Arc<V> to avoid deep copies if caller wants to reuse it.
    pub fn get_arc<Q>(&self, key: &Q) -> Option<Arc<V>>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let mut entries = self.entries.write().unwrap_or_else(|poisoned| {
            warn!("improvements cache write lock poisoned during get_arc; recovering");
            poisoned.into_inner()
        });

        // Use get directly instead of contains_key + get
        if let Some(entry) = entries.get(key) {
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
            let value = Arc::clone(&entry.value);
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
        let mut entries = self.entries.write().unwrap_or_else(|poisoned| {
            warn!("improvements cache write lock poisoned during put; recovering");
            poisoned.into_inner()
        });

        // Check if key already exists - if so, just update (no eviction needed)
        if let std::collections::hash_map::Entry::Occupied(mut e) = entries.entry(key.clone()) {
            e.insert(CacheEntry::new(value, self.default_ttl));
            return Ok(());
        }

        // Key doesn't exist, check capacity and evict if needed
        if entries.len() >= self.max_size {
            // Find least recently used entry
            let lru_key = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(k, _)| k.clone());
            if let Some(lru_key) = lru_key {
                entries.remove(&lru_key);
                self.obs_context
                    .event(EventType::CacheEvicted, "cache", "evicted LRU entry", None);
            }
        }

        entries.insert(key, CacheEntry::new(value, self.default_ttl));
        Ok(())
    }

    /// Insert an Arc<V> directly into the cache without cloning the value.
    pub fn put_arc(&self, key: K, value: Arc<V>) -> Result<(), String> {
        let mut entries = self.entries.write().unwrap_or_else(|poisoned| {
            warn!("improvements cache write lock poisoned during put_arc; recovering");
            poisoned.into_inner()
        });

        let now = Instant::now();
        let new_entry = CacheEntry {
            value: Arc::clone(&value),
            created_at: now,
            last_accessed: now,
            ttl: self.default_ttl,
        };

        // Check if key already exists - if so, just update (no eviction needed)
        if let std::collections::hash_map::Entry::Occupied(mut e) = entries.entry(key.clone()) {
            e.insert(new_entry);
            return Ok(());
        }

        // Key doesn't exist, check capacity and evict if needed
        if entries.len() >= self.max_size {
            let lru_key = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(k, _)| k.clone());
            if let Some(lru_key) = lru_key {
                entries.remove(&lru_key);
                self.obs_context
                    .event(EventType::CacheEvicted, "cache", "evicted LRU entry", None);
            }
        }

        entries.insert(key, new_entry);
        Ok(())
    }

    /// Compatibility helper: returns an owned V by cloning the Arc inside.
    pub fn get_owned<Q>(&self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.get_arc(key).map(|arc| (*arc).clone())
    }

    /// Compatibility alias: returns an owned value (cloned) similar to older cache API
    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.get_owned(key)
    }

    /// Get cache size
    pub fn size(&self) -> usize {
        let entries = self.entries.read().unwrap_or_else(|poisoned| {
            warn!("improvements cache read lock poisoned during size; recovering");
            poisoned.into_inner()
        });
        entries.len()
    }

    /// Clear expired entries
    pub fn evict_expired(&self) -> usize {
        let mut entries = self.entries.write().unwrap_or_else(|poisoned| {
            warn!("improvements cache write lock poisoned during evict_expired; recovering");
            poisoned.into_inner()
        });
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
        self.entries
            .write()
            .unwrap_or_else(|poisoned| {
                warn!("improvements cache write lock poisoned during clear; recovering");
                poisoned.into_inner()
            })
            .clear();
    }

    /// Get cache stats
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.read().unwrap_or_else(|poisoned| {
            warn!("improvements cache read lock poisoned during stats; recovering");
            poisoned.into_inner()
        });
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
#[deprecated(since = "0.1.0", note = "Use crate::cache::UnifiedCache instead")]
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub max_size: usize,
    pub expired_entries: usize,
    pub utilization_percent: f32,
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    #[allow(deprecated)]
    fn test_cache_basic_get_put() {
        let cache = LruCache::new(10, Duration::from_secs(60));

        cache
            .put_arc("key1".to_string(), Arc::new("value1".to_string()))
            .unwrap();
        assert_eq!(cache.get_owned("key1"), Some("value1".to_string()));
    }

    #[test]
    #[allow(deprecated)]
    fn test_cache_ttl_expiration() {
        let cache = LruCache::new(10, Duration::from_millis(100));

        cache
            .put_arc("key1".to_string(), Arc::new("value1".to_string()))
            .unwrap();
        assert_eq!(cache.get_owned("key1"), Some("value1".to_string()));

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(cache.get_owned("key1"), None);
    }

    #[test]
    #[allow(deprecated)]
    fn test_cache_lru_eviction() {
        let cache = LruCache::new(3, Duration::from_secs(60));

        // Fill cache
        cache
            .put_arc("key1".to_string(), Arc::new("value1".to_string()))
            .unwrap();
        cache
            .put_arc("key2".to_string(), Arc::new("value2".to_string()))
            .unwrap();
        cache
            .put_arc("key3".to_string(), Arc::new("value3".to_string()))
            .unwrap();

        // Access key1 to mark it recently used
        cache.get_arc("key1");

        // Add new entry (should evict key2 as LRU)
        cache
            .put_arc("key4".to_string(), Arc::new("value4".to_string()))
            .unwrap();

        assert_eq!(cache.get_owned("key1"), Some("value1".to_string()));
        assert_eq!(cache.get_owned("key2"), None); // Evicted
        assert_eq!(cache.get_owned("key3"), Some("value3".to_string()));
        assert_eq!(cache.get_owned("key4"), Some("value4".to_string()));
    }

    #[test]
    #[allow(deprecated)]
    fn test_cache_stats() {
        let cache = LruCache::new(10, Duration::from_secs(60));

        cache
            .put_arc("key1".to_string(), Arc::new("value1".to_string()))
            .unwrap();
        cache
            .put_arc("key2".to_string(), Arc::new("value2".to_string()))
            .unwrap();

        let stats = cache.stats();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.max_size, 10);
        assert!(stats.utilization_percent > 15.0 && stats.utilization_percent < 25.0);
    }

    #[test]
    #[allow(deprecated)]
    fn test_cache_clear() {
        let cache = LruCache::new(10, Duration::from_secs(60));

        cache
            .put_arc("key1".to_string(), Arc::new("value1".to_string()))
            .unwrap();
        assert_eq!(cache.size(), 1);

        cache.clear();
        assert_eq!(cache.size(), 0);
    }

    #[test]
    #[allow(deprecated)]
    fn test_cache_evict_expired() {
        let cache = LruCache::new(10, Duration::from_millis(100));

        cache
            .put_arc("key1".to_string(), Arc::new("value1".to_string()))
            .unwrap();
        cache
            .put_arc("key2".to_string(), Arc::new("value2".to_string()))
            .unwrap();

        std::thread::sleep(Duration::from_millis(150));

        let evicted = cache.evict_expired();
        assert_eq!(evicted, 2);
        assert_eq!(cache.size(), 0);
    }
}
