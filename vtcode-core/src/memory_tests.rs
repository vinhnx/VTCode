//! Memory optimization regression tests
//!
//! These tests verify that memory usage stays within expected bounds
//! after optimizations have been applied.

#[cfg(test)]
mod memory_profiling {
    use crate::cache::{CacheKey, DEFAULT_CACHE_TTL, EvictionPolicy, UnifiedCache};

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct MemTestKey(String);

    impl CacheKey for MemTestKey {
        fn to_cache_key(&self) -> String {
            self.0.clone()
        }
    }

    /// Verify cache evicts entries when capacity is exceeded
    #[test]
    fn test_cache_capacity_enforcement() {
        let max_entries = 100;
        let mut cache: UnifiedCache<MemTestKey, String> =
            UnifiedCache::new(max_entries, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);

        // Insert more entries than capacity
        for i in 0..150 {
            let key = MemTestKey(format!("key_{}", i));
            let value = format!("value_{}", i);
            let size_bytes = value.len() as u64;
            cache.insert(key, value, size_bytes);
        }

        // Verify cache size never exceeds capacity
        assert!(
            cache.len() <= max_entries,
            "Cache size {} exceeds max capacity {}",
            cache.len(),
            max_entries
        );

        // Verify evictions happened
        let stats = cache.stats();
        assert!(stats.evictions > 0, "Expected evictions but got none");
    }

    /// Verify expired entries are cleaned up
    #[test]
    fn test_cache_expiration_cleanup() {
        use std::time::Duration;

        let mut cache: UnifiedCache<MemTestKey, String> =
            UnifiedCache::new(100, Duration::from_millis(50), EvictionPolicy::Lru);

        let key = MemTestKey("test".into());
        cache.insert(key.clone(), "value".into(), 100);

        // Entry should exist
        assert!(cache.get(&key).is_some());

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(100));

        // Entry should be expired and cleaned up
        assert!(cache.get(&key).is_none());
    }

    /// Verify cache hit rate tracking
    #[test]
    fn test_cache_hit_rate_metrics() {
        let mut cache: UnifiedCache<MemTestKey, String> =
            UnifiedCache::new(10, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);
        let key1 = MemTestKey("key1".into());
        let key2 = MemTestKey("key2".into());

        cache.insert(key1.clone(), "value1".into(), 100);
        cache.insert(key2.clone(), "value2".into(), 100);

        // Miss: key doesn't exist
        let _ = cache.get(&MemTestKey("nonexistent".into()));

        // Hits
        let _ = cache.get(&key1);
        let _ = cache.get(&key2);

        let stats = cache.stats();
        assert_eq!(stats.hits, 2, "Expected 2 cache hits");
        assert_eq!(stats.misses, 1, "Expected 1 cache miss");
        assert!(stats.total_memory_bytes > 0, "Expected memory tracking");
    }

    /// Verify memory tracking accuracy
    #[test]
    fn test_cache_memory_tracking() {
        let mut cache: UnifiedCache<MemTestKey, String> =
            UnifiedCache::new(100, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);

        let test_size_bytes = 1024;
        let key1 = MemTestKey("key1".into());
        let value1 = "x".repeat(1000); // Approximately test_size_bytes

        cache.insert(key1, value1, test_size_bytes as u64);

        let stats = cache.stats();
        assert_eq!(
            stats.total_memory_bytes, test_size_bytes as u64,
            "Memory tracking mismatch"
        );
    }

    /// Verify LRU eviction policy works correctly
    #[test]
    fn test_lru_eviction_policy() {
        let mut cache: UnifiedCache<MemTestKey, String> =
            UnifiedCache::new(2, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);

        let key1 = MemTestKey("key1".into());
        let key2 = MemTestKey("key2".into());
        let key3 = MemTestKey("key3".into());

        cache.insert(key1.clone(), "value1".into(), 100);
        std::thread::sleep(std::time::Duration::from_millis(10));

        cache.insert(key2.clone(), "value2".into(), 100);
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Access key1 to make it more recently used than key2
        let _ = cache.get(&key1);
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Insert key3, should evict key2 (least recently used)
        cache.insert(key3.clone(), "value3".into(), 100);

        assert!(cache.get(&key1).is_some(), "key1 should still be in cache");
        assert!(cache.get(&key2).is_none(), "key2 should have been evicted");
        assert!(cache.get(&key3).is_some(), "key3 should be in cache");
    }

    /// Benchmark cache operations for performance regression testing
    #[test]
    #[ignore]
    fn bench_cache_operations() {
        let mut cache: UnifiedCache<MemTestKey, String> =
            UnifiedCache::new(1_000, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);
        let start = std::time::Instant::now();

        // Insert 10,000 entries
        for i in 0..10_000 {
            let key = MemTestKey(format!("key_{}", i));
            let value = format!("value_{}", i);
            cache.insert(key, value.clone(), value.len() as u64);
        }

        let insert_time = start.elapsed();

        let start = std::time::Instant::now();
        // Read operations
        for i in 0..1_000 {
            let key = MemTestKey(format!("key_{}", i));
            let _ = cache.get(&key);
        }

        let read_time = start.elapsed();

        println!("Insert 10k entries: {:?}", insert_time);
        println!("Read 1k entries: {:?}", read_time);
        println!("Cache size: {}", cache.len());
        println!("Cache stats: {:?}", cache.stats());

        // Verify performance is reasonable (no hard deadline, just logging)
        assert!(insert_time.as_millis() < 5000, "Inserts taking too long");
        assert!(read_time.as_millis() < 1000, "Reads taking too long");
    }
}
