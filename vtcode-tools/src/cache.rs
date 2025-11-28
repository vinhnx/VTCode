//! LRU cache with TTL enforcement and observability hooks.
//!
//! Provides a production-ready cache for tool results, pattern data, and LLM responses.
//! Includes metrics collection and optional logging.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// Arc brings shared ownership of cached values
/// Cache entry with TTL tracking.
#[derive(Debug)]
struct CacheEntry<V> {
    value: Arc<V>,
    inserted_at: Instant,
    accessed_at: Instant,
    access_count: u64,
}

impl<V> CacheEntry<V> {
    fn is_expired(&self, ttl: Duration) -> bool {
        self.inserted_at.elapsed() > ttl
    }

    fn update_access(&mut self) {
        self.accessed_at = Instant::now();
        self.access_count += 1;
    }
}

/// Statistics about cache performance.
#[derive(Clone, Copy, Debug, Default)]
pub struct CacheStats {
    /// Total hit count across all entries.
    pub hits: u64,
    /// Total miss count.
    pub misses: u64,
    /// Total evictions due to capacity.
    pub evictions: u64,
    /// Total expirations.
    pub expirations: u64,
}

impl CacheStats {
    /// Hit rate as percentage (0.0 to 100.0).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// Observability hook for cache events.
#[async_trait::async_trait]
pub trait CacheObserver: Send + Sync {
    async fn on_hit(&self, key: &str, access_count: u64);
    async fn on_miss(&self, key: &str);
    async fn on_evict(&self, key: &str, reason: EvictionReason);
}

/// Why an entry was evicted.
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum EvictionReason {
    Capacity,
    Expired,
    Manual,
}

/// Noop observer (default).
pub struct NoopObserver;

#[async_trait::async_trait]
impl CacheObserver for NoopObserver {
    async fn on_hit(&self, _: &str, _: u64) {}
    async fn on_miss(&self, _: &str) {}
    async fn on_evict(&self, _: &str, _: EvictionReason) {}
}

/// LRU cache with TTL, capacity limits, and observability.
pub struct LruCache<V> {
    /// Maximum entries before LRU eviction.
    capacity: usize,
    /// TTL for all entries.
    ttl: Duration,
    /// The actual cache.
    entries: Arc<RwLock<HashMap<String, CacheEntry<V>>>>,
    /// Access order for LRU tracking (stored separately for efficiency).
    access_order: Arc<RwLock<VecDeque<String>>>,
    /// Stats tracking.
    stats: Arc<RwLock<CacheStats>>,
    /// Observability hook.
    observer: Arc<dyn CacheObserver>,
}

impl<V: Send + Sync> LruCache<V> {
    /// Create a new cache with capacity and TTL.
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self::with_observer(capacity, ttl, Arc::new(NoopObserver))
    }

    /// Create a cache with a custom observer.
    pub fn with_observer(capacity: usize, ttl: Duration, observer: Arc<dyn CacheObserver>) -> Self {
        Self {
            capacity,
            ttl,
            entries: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(VecDeque::new())),
            stats: Arc::new(RwLock::new(CacheStats::default())),
            observer,
        }
    }

    /// Get a value from the cache.
    pub async fn get(&self, key: &str) -> Option<Arc<V>> {
        let mut entries = self.entries.write().await;

        // Check for expiration.
        if let Some(entry) = entries.get(key) {
            if entry.is_expired(self.ttl) {
                entries.remove(key);

                // Update stats once instead of locking twice.
                {
                    let mut stats = self.stats.write().await;
                    stats.expirations += 1;
                    stats.misses += 1;
                }

                self.observer.on_evict(key, EvictionReason::Expired).await;
                let mut order = self.access_order.write().await;
                order.retain(|k| k != key);
                return None;
            }
        }

        // Return value if exists and not expired.
        if let Some(entry) = entries.get_mut(key) {
            entry.update_access();
            self.observer.on_hit(key, entry.access_count).await;
            let mut stats = self.stats.write().await;
            stats.hits += 1;

            // Update LRU order: move accessed key to back (most recently used)
            let mut order = self.access_order.write().await;
            order.retain(|k| k != key);
            order.push_back(key.to_string());

            return Some(Arc::clone(&entry.value));
        }

        {
            let mut stats = self.stats.write().await;
            stats.misses += 1;
        }
        self.observer.on_miss(key).await;
        None
    }

    /// Get a value as an owned clone from the cache (compatibility helper).
    pub async fn get_owned(&self, key: &str) -> Option<V>
    where
        V: Clone,
    {
        self.get(key).await.map(|arc| (*arc).clone())
    }

    /// Alias to return Arc<V> explicitly (clarifies intent)
    pub async fn get_arc(&self, key: &str) -> Option<Arc<V>> {
        self.get(key).await
    }

    /// Insert a value into the cache.
    pub async fn insert(&self, key: String, value: V) {
        self.insert_arc(key, Arc::new(value)).await;
    }

    /// Insert an Arc-wrapped value into the cache to avoid extra cloning.
    pub async fn insert_arc(&self, key: String, value: Arc<V>) {
        let mut entries = self.entries.write().await;

        // If at capacity and key doesn't exist, evict LRU entry.
        if entries.len() >= self.capacity && !entries.contains_key(&key) {
            let mut order = self.access_order.write().await;
            if let Some(lru_key) = order.front().cloned() {
                entries.remove(&lru_key);
                self.observer
                    .on_evict(&lru_key, EvictionReason::Capacity)
                    .await;
                let mut stats = self.stats.write().await;
                stats.evictions += 1;
                order.pop_front();
            }
        }

        let entry = CacheEntry {
            value: value.clone(),
            inserted_at: Instant::now(),
            accessed_at: Instant::now(),
            access_count: 0,
        };

        entries.insert(key.clone(), entry);
        let mut order = self.access_order.write().await;
        order.push_back(key);
    }

    /// Remove a specific key.
    pub async fn remove(&self, key: &str) -> Option<Arc<V>> {
        let mut entries = self.entries.write().await;
        let mut order = self.access_order.write().await;
        order.retain(|k| k != key);
        self.observer.on_evict(key, EvictionReason::Manual).await;
        entries.remove(key).map(|e| e.value)
    }

    /// Clear all entries.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        let mut order = self.access_order.write().await;
        entries.clear();
        order.clear();
        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();
    }

    /// Get current cache statistics.
    pub async fn stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    /// Get number of entries in cache.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Check if cache is empty.
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    /// Get all keys in cache (excluding expired).
    pub async fn keys(&self) -> Vec<String> {
        let entries = self.entries.read().await;
        entries
            .iter()
            .filter(|(_, entry)| !entry.is_expired(self.ttl))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Remove expired entries.
    pub async fn prune_expired(&self) {
        let mut entries = self.entries.write().await;
        let mut order = self.access_order.write().await;

        let expired: Vec<_> = entries
            .iter()
            .filter(|(_, entry)| entry.is_expired(self.ttl))
            .map(|(k, _)| k.clone())
            .collect();

        let mut expired_count = 0usize;
        for key in expired {
            entries.remove(&key);
            order.retain(|k| k != &key);
            self.observer.on_evict(&key, EvictionReason::Expired).await;
            expired_count += 1;
        }

        if expired_count > 0 {
            let mut stats = self.stats.write().await;
            stats.expirations += expired_count as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_operations() {
        let cache: LruCache<String> = LruCache::new(3, Duration::from_secs(60));

        cache
            .insert_arc("a".into(), Arc::new("value_a".into()))
            .await;
        cache
            .insert_arc("b".into(), Arc::new("value_b".into()))
            .await;

        assert_eq!(
            cache.get("a").await.map(|v| (*v).clone()),
            Some("value_a".into())
        );
        assert_eq!(
            cache.get("b").await.map(|v| (*v).clone()),
            Some("value_b".into())
        );
        assert_eq!(cache.get("c").await, None);
    }

    #[tokio::test]
    async fn test_capacity_eviction() {
        let cache: LruCache<i32> = LruCache::new(2, Duration::from_secs(60));

        cache.insert("a".into(), 1).await;
        cache.insert("b".into(), 2).await;
        cache.insert("c".into(), 3).await; // Should evict "a"

        assert_eq!(cache.get("a").await, None);
        assert_eq!(cache.get("b").await.map(|v| *v), Some(2));
        assert_eq!(cache.get("c").await.map(|v| *v), Some(3));
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        let cache: LruCache<String> = LruCache::new(10, Duration::from_millis(50));

        cache.insert_arc("a".into(), Arc::new("value".into())).await;
        assert_eq!(
            cache.get("a").await.map(|v| (*v).clone()),
            Some("value".into())
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(cache.get("a").await, None);
    }

    #[tokio::test]
    async fn test_stats() {
        let cache: LruCache<String> = LruCache::new(10, Duration::from_secs(60));

        cache.insert_arc("a".into(), Arc::new("value".into())).await;
        cache.get("a").await; // hit
        cache.get("b").await; // miss

        let stats = cache.stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_prune_expired() {
        let cache: LruCache<i32> = LruCache::new(10, Duration::from_millis(50));

        cache.insert("a".into(), 1).await;
        cache.insert("b".into(), 2).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        cache.prune_expired().await;

        assert_eq!(cache.len().await, 0);
    }

    #[tokio::test]
    async fn insert_arc_avoids_clone() {
        let cache = LruCache::new(2, Duration::from_secs(60));
        let v = Arc::new(42);
        cache.insert_arc("k1".to_string(), Arc::clone(&v)).await;
        let got = cache.get("k1").await;
        assert_eq!(got.unwrap().as_ref(), &42);
    }
}
