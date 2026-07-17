//! LRU cache with TTL enforcement and observability hooks.
//!
//! Provides a production-ready cache for tool results, pattern data, and LLM responses.
//! Includes metrics collection and optional logging.

use hashbrown::{HashMap, HashSet};
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

/// Combined cache state: entries and access order protected by a single lock.
///
/// This eliminates the deadlock risk from acquiring multiple locks simultaneously.
#[derive(Debug)]
struct CacheState<V> {
    entries: HashMap<String, CacheEntry<V>>,
    access_order: VecDeque<String>,
}

impl<V> CacheEntry<V> {
    #[inline]
    fn is_expired(&self, ttl: Duration) -> bool {
        self.inserted_at.elapsed() > ttl
    }

    #[inline]
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
    #[inline]
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
    /// Combined cache state: entries and access order protected by a single lock.
    state: Arc<RwLock<CacheState<V>>>,
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
            state: Arc::new(RwLock::new(CacheState {
                entries: HashMap::new(),
                access_order: VecDeque::new(),
            })),
            stats: Arc::new(RwLock::new(CacheStats::default())),
            observer,
        }
    }

    /// Get a value from the cache.
    pub async fn get(&self, key: &str) -> Option<Arc<V>> {
        // Perform all state mutations under a single lock acquisition.
        enum GetOutcome<V> {
            Hit { value: Arc<V>, access_count: u64 },
            Miss,
            Expired,
        }

        let outcome = {
            let mut state = self.state.write().await;

            // Check for expiration first.
            if let Some(entry) = state.entries.get(key) {
                if entry.is_expired(self.ttl) {
                    state.entries.remove(key);
                    state.access_order.retain(|k| k != key);
                    GetOutcome::Expired
                } else {
                    // Valid hit - extract value and access count, then update entry.
                    let value = Arc::clone(&entry.value);
                    let access_count = entry.access_count;

                    // Update the entry's access metadata.
                    if let Some(entry) = state.entries.get_mut(key) {
                        entry.update_access();
                    }

                    // Move to back of access order (most recently used).
                    state.access_order.retain(|k| k != key);
                    state.access_order.push_back(key.to_string());

                    GetOutcome::Hit { value, access_count }
                }
            } else {
                GetOutcome::Miss
            }
        };

        // Handle observer callbacks outside the lock.
        match outcome {
            GetOutcome::Hit { value, access_count } => {
                let mut stats = self.stats.write().await;
                stats.hits += 1;
                self.observer.on_hit(key, access_count).await;
                Some(value)
            }
            GetOutcome::Expired => {
                let mut stats = self.stats.write().await;
                stats.expirations += 1;
                stats.misses += 1;
                self.observer.on_evict(key, EvictionReason::Expired).await;
                None
            }
            GetOutcome::Miss => {
                let mut stats = self.stats.write().await;
                stats.misses += 1;
                self.observer.on_miss(key).await;
                None
            }
        }
    }

    /// Get a value as an owned clone from the cache (compatibility helper).
    pub async fn get_owned(&self, key: &str) -> Option<V>
    where
        V: Clone,
    {
        self.get(key).await.map(|arc| V::clone(&arc))
    }

    /// Alias to return `Arc<V>` explicitly (clarifies intent).
    pub async fn get_arc(&self, key: &str) -> Option<Arc<V>> {
        self.get(key).await
    }

    /// Insert a value into the cache.
    pub async fn insert(&self, key: String, value: V) {
        self.insert_arc(key, Arc::new(value)).await;
    }

    /// Insert an Arc-wrapped value into the cache to avoid extra cloning.
    pub async fn insert_arc(&self, key: String, value: Arc<V>) {
        let capacity_evicted = {
            let mut state = self.state.write().await;
            let mut evicted: Option<String> = None;

            // If at capacity and key doesn't exist, evict LRU entry.
            if state.entries.len() >= self.capacity && !state.entries.contains_key(&key) {
                if let Some(lru_key) = state.access_order.pop_front() {
                    state.entries.remove(&lru_key);
                    evicted = Some(lru_key);
                }
            }

            let entry = CacheEntry {
                value: Arc::clone(&value),
                inserted_at: Instant::now(),
                accessed_at: Instant::now(),
                access_count: 0,
            };

            state.entries.insert(key.clone(), entry);
            state.access_order.retain(|existing| existing != &key);
            state.access_order.push_back(key);
            evicted
        };

        // Avoid awaiting external observer hooks while cache lock is held.
        if let Some(evicted_key) = capacity_evicted {
            self.observer.on_evict(&evicted_key, EvictionReason::Capacity).await;
            let mut stats = self.stats.write().await;
            stats.evictions += 1;
        }
    }

    /// Remove a specific key.
    pub async fn remove(&self, key: &str) -> Option<Arc<V>> {
        let removed = {
            let mut state = self.state.write().await;
            state.access_order.retain(|k| k != key);
            state.entries.remove(key).map(|e| e.value)
        };

        if removed.is_some() {
            self.observer.on_evict(key, EvictionReason::Manual).await;
        }
        removed
    }

    /// Clear all entries.
    pub async fn clear(&self) {
        let mut state = self.state.write().await;
        state.entries.clear();
        state.access_order.clear();
        let mut stats = self.stats.write().await;
        *stats = CacheStats::default();
    }

    /// Get current cache statistics.
    pub async fn stats(&self) -> CacheStats {
        *self.stats.read().await
    }

    /// Get number of entries in cache.
    pub async fn len(&self) -> usize {
        self.state.read().await.entries.len()
    }

    /// Check if cache is empty.
    pub async fn is_empty(&self) -> bool {
        self.state.read().await.entries.is_empty()
    }

    /// Get all keys in cache (excluding expired).
    pub async fn keys(&self) -> Vec<String> {
        let state = self.state.read().await;
        state
            .entries
            .iter()
            .filter(|(_, entry)| !entry.is_expired(self.ttl))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Remove expired entries.
    pub async fn prune_expired(&self) {
        let expired = {
            let mut state = self.state.write().await;

            let mut expired = Vec::new();
            state.entries.retain(|key, entry| {
                let keep = !entry.is_expired(self.ttl);
                if !keep {
                    expired.push(key.clone());
                }
                keep
            });

            if !expired.is_empty() {
                let expired_set: HashSet<_> = expired.iter().cloned().collect();
                state.access_order.retain(|k| !expired_set.contains(k));
            }

            expired
        };

        if expired.is_empty() {
            return;
        }

        for key in &expired {
            self.observer.on_evict(key, EvictionReason::Expired).await;
        }

        let mut stats = self.stats.write().await;
        stats.expirations += expired.len() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_operations() {
        let cache: LruCache<String> = LruCache::new(3, Duration::from_secs(60));

        cache.insert_arc("a".into(), Arc::new("value_a".into())).await;
        cache.insert_arc("b".into(), Arc::new("value_b".into())).await;

        assert_eq!(cache.get("a").await.map(|v| (*v).clone()), Some("value_a".into()));
        assert_eq!(cache.get("b").await.map(|v| (*v).clone()), Some("value_b".into()));
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
        assert_eq!(cache.get("a").await.map(|v| (*v).clone()), Some("value".into()));

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
