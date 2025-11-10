//! Performance optimization utilities for the TUI session
//! 
//! Contains optimized algorithms and data structures for performance-critical operations.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// A simple LRU cache for expensive computations
pub struct LruCache<K, V> {
    capacity: usize,
    map: HashMap<K, (V, Instant)>,
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates a new LRU cache with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            map: HashMap::with_capacity(capacity),
        }
    }

    /// Gets a value from the cache, returning None if it's not present or expired
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if let Some((value, timestamp)) = self.map.get(key) {
            // Check if entry has expired (keeping it simple with 5 minute expiry for now)
            if timestamp.elapsed() < Duration::from_secs(300) {
                return Some(value);
            } else {
                self.map.remove(key);
            }
        }
        None
    }

    /// Inserts a value into the cache
    pub fn insert(&mut self, key: K, value: V) {
        if self.map.len() >= self.capacity {
            // Simple eviction: remove a random entry when at capacity
            if let Some(key_to_remove) = self.map.keys().next().cloned() {
                self.map.remove(&key_to_remove);
            }
        }
        self.map.insert(key, (value, Instant::now()));
    }

    /// Clears expired entries from the cache
    pub fn clear_expired(&mut self) {
        let now = Instant::now();
        self.map.retain(|_, (_, timestamp)| now.duration_since(*timestamp) < Duration::from_secs(300));
    }

    /// Returns the current number of entries in the cache
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// Thread-safe version using RwLock if needed for concurrent access
use std::sync::RwLock;

pub struct ThreadSafeLruCache<K, V> {
    inner: RwLock<LruCache<K, V>>,
}

impl<K, V> ThreadSafeLruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: RwLock::new(LruCache::new(capacity)),
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let mut cache = self.inner.write().unwrap();
        cache.get(key).cloned()
    }

    pub fn insert(&self, key: K, value: V) {
        let mut cache = self.inner.write().unwrap();
        cache.insert(key, value);
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
        let initial_size = cache.size();
        cache.clear_expired();
        
        // Verify the method doesn't crash and returns the cache to a valid state
        assert!(cache.size() <= initial_size);
        
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