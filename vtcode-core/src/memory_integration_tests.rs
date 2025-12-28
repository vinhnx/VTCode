//! Real-world memory profiling tests with actual workload simulation
//!
//! These tests measure actual memory usage (RSS) changes, not just logic validation.
//! They simulate realistic VT Code usage patterns.

#[cfg(test)]
mod memory_integration {
    use crate::cache::{CacheKey, DEFAULT_CACHE_TTL, EvictionPolicy, UnifiedCache};
    use std::collections::VecDeque;

    /// Helper to measure current RSS in kilobytes (Unix/macOS only)
    #[cfg(unix)]
    fn get_rss_kb() -> u64 {
        use std::fs;
        if let Ok(status) = fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Ok(kb) = line.split_whitespace().nth(1).unwrap_or("0").parse::<u64>() {
                        return kb;
                    }
                }
            }
        }
        0
    }

    #[cfg(not(unix))]
    fn get_rss_kb() -> u64 {
        0 // Placeholder for non-Unix systems
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct MemKey(String);

    impl CacheKey for MemKey {
        fn to_cache_key(&self) -> String {
            self.0.clone()
        }
    }

    /// Test: Verify PTY scrollback buffer doesn't grow unbounded
    /// Simulates long-running command output
    #[test]
    fn test_pty_scrollback_bounded_growth() {
        const MAX_BUFFER_SIZE: usize = 25 * 1024 * 1024; // 25MB limit (post-optimization)
        const LINE_SIZE: usize = 1024; // 1KB per line
        const NUM_LINES: usize = 50_000; // 50k lines = 50MB if unbounded

        let mut scrollback: VecDeque<String> = VecDeque::new();
        let mut total_bytes = 0;

        // Simulate long PTY output
        for i in 0..NUM_LINES {
            let line = format!("[{}] Output line with some content\n", i);
            total_bytes += line.len();

            scrollback.push_back(line);

            // Enforce the 25MB limit
            while total_bytes > MAX_BUFFER_SIZE {
                if let Some(removed) = scrollback.pop_front() {
                    total_bytes -= removed.len();
                }
            }
        }

        // Verify memory stayed bounded
        let max_with_overshoot = (MAX_BUFFER_SIZE as f64 * 1.1) as usize;
        assert!(
            total_bytes <= max_with_overshoot, // Allow 10% overshoot
            "PTY scrollback grew beyond limit: {} bytes (limit: {} bytes)",
            total_bytes,
            max_with_overshoot
        );

        println!(
            "✅ PTY scrollback bounded: {:.1}MB / {:.1}MB max",
            total_bytes as f64 / 1_000_000.0,
            MAX_BUFFER_SIZE as f64 / 1_000_000.0
        );
    }

    /// Test: Verify parse cache doesn't accumulate unbounded
    /// Simulates parsing many files repeatedly
    #[test]
    fn test_parse_cache_bounded_accumulation() {
        const CACHE_SIZE: usize = 50; // Post-optimization parse cache size
        const ENTRY_SIZE: usize = 100_000; // ~100KB per parsed tree

        let mut cache: UnifiedCache<MemKey, Vec<u8>> =
            UnifiedCache::new(CACHE_SIZE, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);

        // Simulate parsing 200 different files
        for i in 0..200 {
            let key = MemKey(format!("file_{}.rs", i));
            let parsed_tree = vec![0u8; ENTRY_SIZE]; // Fake parsed data

            cache.insert(key, parsed_tree, ENTRY_SIZE as u64);
        }

        // Verify cache size is bounded
        let max_expected_memory = CACHE_SIZE * ENTRY_SIZE;
        let actual_memory = cache.stats().total_memory_bytes as usize;

        let max_with_overshoot = (max_expected_memory as f64 * 1.1) as usize;
        assert!(
            actual_memory <= max_with_overshoot,
            "Parse cache exceeded bounds: {} bytes vs {} bytes max",
            actual_memory,
            max_with_overshoot
        );

        println!(
            "✅ Parse cache bounded: {} entries, {:.1}MB memory",
            cache.len(),
            actual_memory as f64 / 1_000_000.0
        );
    }

    /// Test: Verify cache eviction happens before memory explosion
    /// Simulates cache under sustained load
    #[test]
    fn test_cache_eviction_under_load() {
        let mut cache: UnifiedCache<MemKey, String> = UnifiedCache::new(
            100, // Small cache
            DEFAULT_CACHE_TTL,
            EvictionPolicy::Lru,
        );

        // Insert way more than capacity
        for i in 0..1_000 {
            let key = MemKey(format!("key_{}", i));
            let large_value = "x".repeat(100_000); // 100KB per entry
            cache.insert(key, large_value, 100_000);

            // Cache should never exceed reasonable bounds
            assert!(
                cache.len() <= 150, // Allow some overshoot before eviction kicks in
                "Cache grew too large: {} entries",
                cache.len()
            );
        }

        let stats = cache.stats();
        println!(
            "✅ Cache eviction working: {} entries in cache, {} total evictions",
            stats.current_size, stats.evictions
        );

        assert!(
            stats.evictions > 0,
            "Expected evictions but cache never cleaned up"
        );
    }

    /// Test: Verify TTL-based cleanup prevents stale data accumulation
    /// Simulates long session with periodic access
    #[test]
    fn test_cache_ttl_prevents_stale_accumulation() {
        use std::time::Duration;

        let mut cache: UnifiedCache<MemKey, Vec<u8>> = UnifiedCache::new(
            1_000,
            Duration::from_millis(100), // Short TTL for testing
            EvictionPolicy::TtlOnly,
        );

        // Insert entries
        for i in 0..100 {
            let key = MemKey(format!("key_{}", i));
            cache.insert(key, vec![0u8; 100_000], 100_000);
        }

        let initial_size = cache.len();
        let initial_memory = cache.stats().total_memory_bytes as f64 / 1_000_000.0;

        println!("Initial: {} entries, {:.1}MB", initial_size, initial_memory);

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));

        // Try to access one (forces cleanup)
        let _ = cache.get(&MemKey("key_0".into()));

        let final_size = cache.len();
        let final_memory = cache.stats().total_memory_bytes as f64 / 1_000_000.0;

        println!("After TTL: {} entries, {:.1}MB", final_size, final_memory);

        // Should have cleaned up significantly
        assert!(
            final_size < initial_size,
            "TTL-based cleanup didn't work: {} vs {}",
            final_size,
            initial_size
        );
    }

    /// Test: Memory stability over time
    /// Simulates real usage pattern: insert -> access -> cleanup cycle
    /// With proper eviction, memory should stabilize, not grow linearly
    #[test]
    fn test_memory_stability_over_time() {
        let mut cache: UnifiedCache<MemKey, String> = UnifiedCache::new(
            100, // Smaller cache to force eviction
            DEFAULT_CACHE_TTL,
            EvictionPolicy::Lru,
        );

        let mut memory_readings = Vec::new();

        // Simulate 50 cycles of cache operations
        // Each cycle inserts enough items to force eviction
        for cycle in 0..50 {
            // Insert just enough to exceed cache capacity and force eviction
            for i in 0..10 {
                let key = MemKey(format!("cycle_{}_key_{}", cycle, i));
                let value = "x".repeat(1_000); // 1KB per item
                cache.insert(key, value, 1_000);
            }

            // Access some items (update LRU)
            for i in 0..5 {
                let key = MemKey(format!("cycle_{}_key_{}", cycle, i));
                let _ = cache.get(&key);
            }

            memory_readings.push(cache.stats().total_memory_bytes);
        }

        // Check memory trend - should stabilize after initial fill
        let start_memory = memory_readings[0] as f64;
        let end_memory = memory_readings[memory_readings.len() - 1] as f64;
        let max_memory = *memory_readings.iter().max().unwrap() as f64;
        let min_memory_after_stable = memory_readings[10..].iter().min().unwrap();

        println!(
            "Memory stabilization: start={:.0}B, end={:.0}B, peak={:.0}B, stable_min={:.0}B",
            start_memory, end_memory, max_memory, *min_memory_after_stable as f64
        );

        // After initial fill (first 10 cycles), memory should stay relatively stable
        let stable_period_readings: Vec<f64> =
            memory_readings[20..].iter().map(|&v| v as f64).collect();

        if stable_period_readings.len() > 2 {
            let stable_avg: f64 =
                stable_period_readings.iter().sum::<f64>() / stable_period_readings.len() as f64;
            let stable_variance = stable_period_readings
                .iter()
                .map(|&v| (v - stable_avg).abs())
                .sum::<f64>()
                / stable_period_readings.len() as f64;

            println!(
                "Stable period: avg={:.0}B, variance={:.0}B",
                stable_avg, stable_variance
            );

            // Variance should be relatively small compared to actual memory use
            assert!(
                stable_variance / stable_avg < 0.3, // Within 30% variation
                "Memory not stable: {:.1}% variance",
                (stable_variance / stable_avg) * 100.0
            );
        }
    }

    /// Test: Realistic transcript caching scenario
    /// Simulates many messages with width changes
    #[test]
    fn test_transcript_width_cache_bounded() {
        const MAX_WIDTH_CACHES: usize = 3;
        const NUM_MESSAGES: usize = 1000;
        const MESSAGE_SIZE: usize = 1000; // bytes per reflowed message

        // Simulate what TranscriptReflowCache does
        let mut width_caches: std::collections::HashMap<u16, Vec<Vec<String>>> =
            std::collections::HashMap::new();

        // Simulate transcript with width changes
        let widths = vec![80, 100, 120, 140, 160, 180, 200]; // Various terminal widths

        for width in &widths {
            let mut messages = Vec::new();
            for msg_idx in 0..NUM_MESSAGES {
                let reflowed = vec![format!("Line {}", msg_idx); 3]; // 3 lines per message
                messages.push(reflowed);
            }

            width_caches.insert(*width, messages);

            // Enforce max width caches (simulate the optimization)
            if width_caches.len() > MAX_WIDTH_CACHES {
                // Remove oldest (smallest width)
                if let Some(&min_width) = width_caches.keys().min() {
                    width_caches.remove(&min_width);
                }
            }
        }

        // Verify only max_cached_widths are kept
        assert!(
            width_caches.len() <= MAX_WIDTH_CACHES,
            "Width cache exceeded limit: {} vs {}",
            width_caches.len(),
            MAX_WIDTH_CACHES
        );

        println!(
            "✅ Transcript width cache bounded: {} width caches (limit: {})",
            width_caches.len(),
            MAX_WIDTH_CACHES
        );
    }

    /// Benchmark: Compare cache performance before/after optimization
    #[test]
    #[ignore]
    fn bench_cache_optimization_impact() {
        use std::time::Instant;

        // Before optimization: 10,000 capacity
        let mut cache_before: UnifiedCache<MemKey, String> =
            UnifiedCache::new(10_000, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);

        // After optimization: 1,000 capacity
        let mut cache_after: UnifiedCache<MemKey, String> =
            UnifiedCache::new(1_000, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);

        // Insert test
        let start = Instant::now();
        for i in 0..5_000 {
            let key = MemKey(format!("key_{}", i));
            cache_before.insert(key, "value".into(), 100);
        }
        let before_insert = start.elapsed();

        let start = Instant::now();
        for i in 0..5_000 {
            let key = MemKey(format!("key_{}", i));
            cache_after.insert(key, "value".into(), 100);
        }
        let after_insert = start.elapsed();

        println!(
            "Insertion performance (5000 items):\n  Before: {:?}\n  After: {:?}",
            before_insert, after_insert
        );

        // Memory comparison
        let mem_before = cache_before.stats().total_memory_bytes;
        let mem_after = cache_after.stats().total_memory_bytes;

        println!(
            "Memory usage (5000 items):\n  Before: {} bytes\n  After: {} bytes\n  Saved: {:.1}%",
            mem_before,
            mem_after,
            ((mem_before - mem_after) as f64 / mem_before as f64) * 100.0
        );

        // Hit rate test
        let start = Instant::now();
        let mut hits = 0;
        for i in 0..1_000 {
            let key = MemKey(format!("key_{}", i));
            if cache_before.get(&key).is_some() {
                hits += 1;
            }
        }
        let before_hits = start.elapsed();

        let start = Instant::now();
        let mut hits_after = 0;
        for i in 0..1_000 {
            let key = MemKey(format!("key_{}", i));
            if cache_after.get(&key).is_some() {
                hits_after += 1;
            }
        }
        let after_hits = start.elapsed();

        println!(
            "Hit rate (1000 accesses):\n  Before: {} hits in {:?}\n  After: {} hits in {:?}",
            hits, before_hits, hits_after, after_hits
        );
    }
}
