//! Performance optimization utilities for the TUI session
//!
//! Contains optimized algorithms and data structures for performance-critical operations.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Reuse the centralized LRU cache implementation from tools to avoid duplicate code.
use crate::tools::improvements_cache::LruCache as CentralLruCache;

// Thread-safe version using RwLock if needed for concurrent access

pub struct ThreadSafeLruCache<K, V> {
    inner: Arc<CentralLruCache<K, V>>,
}

impl<K, V> ThreadSafeLruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(capacity: usize) -> Self {
        let cache = CentralLruCache::new(capacity, Duration::from_secs(300));
        Self { inner: Arc::new(cache) }
    }

    /// Returns a cloned owned V value for compatibility.
    pub fn get(&self, key: &K) -> Option<V> {
        self.inner.get_owned(key)
    }

    pub fn insert(&self, key: K, value: V) {
        let _ = self.inner.put(key, value);
    }

    /// Efficient insert with Arc to avoid cloning large values
    pub fn insert_arc(&self, key: K, value: Arc<V>) {
        let _ = self.inner.put_arc(key, value);
    }

    /// Gets the shared Arc<V> if present.
    pub fn get_arc(&self, key: &K) -> Option<Arc<V>> {
        self.inner.get_arc(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_lru_cache_basic_operations() {
        let mut cache = LruCache::new(2);

        cache.insert("key1", "value1");
        cache.insert("key2", "value2");

        assert_eq!(cache.get(&"key1"), Some(&"value1"));
        assert_eq!(cache.get(&"key2"), Some(&"value2"));
        assert_eq!(cache.get(&"key3"), None);
    }

    #[test]
    fn test_lru_cache_capacity_limit() {
        let mut cache = LruCache::new(2);

        cache.insert("key1", "value1");
        cache.insert("key2", "value2");
        cache.insert("key3", "value3"); // This should evict an entry

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_lru_cache_empty() {
        let cache = LruCache::<String, String>::new(2);

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_lru_cache_clear_expired() {
        let mut cache = LruCache::new(2);
        cache.insert("key1", "value1");

        // Sleep to ensure expiration
        std::thread::sleep(Duration::from_secs(1));
        cache.clear_expired();

        // This test is a bit tricky since we need to adjust the expiration time
        // Test that the method properly handles expired entries
        let initial_size = cache.len();
        cache.clear_expired();

        // Verify the method doesn't crash and returns the cache to a valid state
        assert!(cache.len() <= initial_size);

        // Test that we can still use the cache after clearing expired items
        assert!(cache.get(&"key1").is_none()); // Should be expired and cleared
    }

    #[test]
    fn test_thread_safe_cache() {
        let cache = ThreadSafeLruCache::new(2);

        cache.insert("key1", "value1");
        let result = cache.get(&"key1");

        assert_eq!(result, Some("value1"));
    }

    #[test]
    fn test_thread_safe_cache_concurrent_access() {
        let cache = std::sync::Arc::new(ThreadSafeLruCache::new(100));

        let cache_clone = cache.clone();
        let handle1 = thread::spawn(move || {
            for i in 0..50 {
                cache_clone.insert(format!("key{}", i), format!("value{}", i));
            }
        });

        let cache_clone2 = cache.clone();
        let handle2 = thread::spawn(move || {
            for i in 50..100 {
                cache_clone2.insert(format!("key{}", i), format!("value{}", i));
            }
        });

        handle1.join().unwrap();
        handle2.join().unwrap();

        // Verify that values were properly stored
        for i in 0..100 {
            let value = cache.get(&format!("key{}", i));
            assert_eq!(value, Some(format!("value{}", i)));
        }
    }

    #[test]
    fn test_lru_cache_edge_cases() {
        // Test with capacity 0
        let mut cache = LruCache::new(0);
        cache.insert("key1", "value1");
        assert_eq!(cache.len(), 0);

        // Test with capacity 1
        let mut cache = LruCache::new(1);
        cache.insert("key1", "value1");
        cache.insert("key2", "value2"); // Should evict key1
        assert_eq!(cache.get(&"key1"), None);
        assert_eq!(cache.get(&"key2"), Some(&"value2"));
    }
}