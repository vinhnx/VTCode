use super::*;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CloneCountingKey {
    name: &'static str,
    clones: Arc<AtomicUsize>,
}

impl Clone for CloneCountingKey {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::Relaxed);
        Self { name: self.name, clones: Arc::clone(&self.clones) }
    }
}

impl PartialEq for CloneCountingKey {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for CloneCountingKey {}

impl Hash for CloneCountingKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl CacheKey for CloneCountingKey {
    fn to_cache_key(&self) -> String {
        self.name.to_owned()
    }
}

struct RankedEntry {
    name: &'static str,
    created_rank: u64,
    accessed_rank: u64,
    access_count: u64,
    size_bytes: u64,
}

struct EvictionFixture {
    cache: UnifiedCache<CloneCountingKey, &'static str>,
    clones: Arc<AtomicUsize>,
}

impl EvictionFixture {
    fn new(policy: EvictionPolicy) -> Self {
        let fixture = Self {
            cache: UnifiedCache::new(4, Duration::from_secs(3600), policy),
            clones: Arc::new(AtomicUsize::new(0)),
        };
        let base = SystemTime::now() - Duration::from_secs(100);
        for entry in [
            RankedEntry {
                name: "a",
                created_rank: 3,
                accessed_rank: 1,
                access_count: 4,
                size_bytes: 11,
            },
            RankedEntry {
                name: "b",
                created_rank: 1,
                accessed_rank: 4,
                access_count: 3,
                size_bytes: 23,
            },
            RankedEntry {
                name: "c",
                created_rank: 4,
                accessed_rank: 2,
                access_count: 1,
                size_bytes: 37,
            },
            RankedEntry {
                name: "d",
                created_rank: 2,
                accessed_rank: 3,
                access_count: 2,
                size_bytes: 41,
            },
        ] {
            fixture.cache.insert(fixture.key(entry.name), entry.name, entry.size_bytes);
            let mut inner = fixture.cache.inner.write().unwrap();
            let cached_entry = inner.entries.get_mut(&fixture.key(entry.name)).unwrap();
            cached_entry.created_at = base + Duration::from_secs(entry.created_rank);
            cached_entry.last_accessed = base + Duration::from_secs(entry.accessed_rank);
            cached_entry.access_count = entry.access_count;
        }
        fixture
    }

    fn key(&self, name: &'static str) -> CloneCountingKey {
        CloneCountingKey { name, clones: Arc::clone(&self.clones) }
    }

    fn assert_remaining(&self, expected_names: &[&str], expected_bytes: u64, expected_evictions: u64) {
        let inner = self.cache.inner.read().unwrap();
        let mut names: Vec<_> = inner.entries.keys().map(|key| key.name).collect();
        names.sort_unstable();
        assert_eq!(names, expected_names);
        assert_eq!(inner.stats.current_size, expected_names.len());
        assert_eq!(inner.stats.total_memory_bytes, expected_bytes);
        assert_eq!(inner.stats.evictions, expected_evictions);
        assert_eq!(self.clones.load(Ordering::Relaxed), expected_evictions as usize);
    }
}

#[test]
fn selective_removal_preserves_survivors_and_accounting_without_cloning_keys() {
    let fixture = EvictionFixture::new(EvictionPolicy::Lru);
    let mut visited = Vec::new();
    assert_eq!(
        fixture.cache.remove_where(|key| {
            visited.push(key.name);
            matches!(key.name, "a" | "c")
        }),
        2
    );
    visited.sort_unstable();
    assert_eq!(visited, ["a", "b", "c", "d"]);
    fixture.assert_remaining(&["b", "d"], 64, 0);
    assert_eq!(*fixture.cache.get(&fixture.key("b")).unwrap(), "b");
    assert_eq!(*fixture.cache.get(&fixture.key("d")).unwrap(), "d");

    assert_eq!(fixture.cache.remove_where(|key| key.name == "absent"), 0);
    fixture.assert_remaining(&["b", "d"], 64, 0);
    assert_eq!(fixture.cache.remove_where(|_| true), 2);
    fixture.assert_remaining(&[], 0, 0);
    assert_eq!(fixture.cache.remove_where(|_| panic!("empty cache has no keys")), 0);
}

#[test]
fn selective_removal_releases_only_matching_cached_values() {
    let clones = Arc::new(AtomicUsize::new(0));
    let key = |name| CloneCountingKey { name, clones: Arc::clone(&clones) };
    let cache = UnifiedCache::new(4, Duration::from_secs(3600), EvictionPolicy::Lru);
    let removed = Arc::new("removed value".to_owned());
    let survivor = Arc::new("surviving value".to_owned());
    let removed_weak = Arc::downgrade(&removed);
    let survivor_weak = Arc::downgrade(&survivor);
    cache.insert_arc(key("remove"), removed, 13);
    cache.insert_arc(key("keep"), survivor, 29);

    assert_eq!(cache.remove_where(|key| key.name == "remove"), 1);
    assert!(removed_weak.upgrade().is_none());
    assert!(survivor_weak.upgrade().is_some());
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.total_memory_bytes(), 29);
    assert_eq!(cache.stats().evictions, 0);
    assert_eq!(clones.load(Ordering::Relaxed), 0);

    assert_eq!(cache.remove_where(|_| true), 1);
    assert!(survivor_weak.upgrade().is_none());
    assert!(cache.is_empty());
    assert_eq!(cache.total_memory_bytes(), 0);
}

#[test]
fn expiration_retains_live_entries_and_accounting_without_cloning_keys() {
    let fixture = EvictionFixture::new(EvictionPolicy::Lru);
    {
        let mut inner = fixture.cache.inner.write().unwrap();
        for name in ["a", "c"] {
            inner.entries.get_mut(&fixture.key(name)).unwrap().created_at = SystemTime::UNIX_EPOCH;
        }
    }

    // Insertion expires a and c before checking capacity, leaving room for new.
    fixture.cache.insert(fixture.key("new"), "new value", 53);
    fixture.assert_remaining(&["b", "d", "new"], 117, 0);
    assert_eq!(*fixture.cache.get(&fixture.key("b")).unwrap(), "b");
    assert_eq!(*fixture.cache.get(&fixture.key("new")).unwrap(), "new value");
}

#[test]
fn capacity_eviction_clones_only_the_victim_for_each_policy() {
    for policy in [
        EvictionPolicy::Lru,
        EvictionPolicy::Lfu,
        EvictionPolicy::Fifo,
        EvictionPolicy::TtlOnly,
    ] {
        let fixture = EvictionFixture::new(policy);
        fixture.cache.insert(fixture.key("new"), "new value", 53);
        // Four entries at capacity evict one: oldest access a, least frequent c,
        // or oldest creation b. The new value must survive in every policy.
        match policy {
            EvictionPolicy::Lru => fixture.assert_remaining(&["b", "c", "d", "new"], 154, 1),
            EvictionPolicy::Lfu => fixture.assert_remaining(&["a", "b", "d", "new"], 128, 1),
            EvictionPolicy::Fifo | EvictionPolicy::TtlOnly => {
                fixture.assert_remaining(&["a", "c", "d", "new"], 142, 1);
            }
        }
    }
}

#[test]
fn bulk_eviction_preserves_each_policy_and_clones_only_victims() {
    for policy in [
        EvictionPolicy::Lru,
        EvictionPolicy::Lfu,
        EvictionPolicy::Fifo,
        EvictionPolicy::TtlOnly,
    ] {
        for pressure in [false, true] {
            let fixture = EvictionFixture::new(policy);
            let removed = if pressure {
                fixture.cache.evict_under_pressure(50)
            } else {
                fixture.cache.clear_least_used(50)
            };
            assert_eq!(removed, 2);
            // Rankings deliberately disagree so choosing the wrong policy fails.
            match policy {
                EvictionPolicy::Lru => fixture.assert_remaining(&["b", "d"], 64, 2),
                EvictionPolicy::Lfu => fixture.assert_remaining(&["a", "b"], 34, 2),
                EvictionPolicy::Fifo | EvictionPolicy::TtlOnly => fixture.assert_remaining(&["a", "c"], 48, 2),
            }
        }
    }
}

#[test]
fn pressure_cleanup_counts_expiration_separately_and_rounds_live_target_down() {
    let fixture = EvictionFixture::new(EvictionPolicy::Lru);
    fixture
        .cache
        .inner
        .write()
        .unwrap()
        .entries
        .get_mut(&fixture.key("a"))
        .unwrap()
        .created_at = SystemTime::UNIX_EPOCH;

    // Remove expired a (11 bytes), then two of three live entries: c and d.
    assert_eq!(fixture.cache.evict_under_pressure(50), 3);
    fixture.assert_remaining(&["b"], 23, 2);
}

#[test]
fn pressure_zero_still_expires_entries_without_counting_evictions() {
    let fixture = EvictionFixture::new(EvictionPolicy::Lru);
    fixture
        .cache
        .inner
        .write()
        .unwrap()
        .entries
        .get_mut(&fixture.key("a"))
        .unwrap()
        .created_at = SystemTime::UNIX_EPOCH;
    assert_eq!(fixture.cache.evict_under_pressure(0), 1);
    fixture.assert_remaining(&["b", "c", "d"], 101, 0);

    for entry in fixture.cache.inner.write().unwrap().entries.values_mut() {
        entry.created_at = SystemTime::UNIX_EPOCH;
    }
    assert_eq!(fixture.cache.evict_under_pressure(0), 3);
    fixture.assert_remaining(&[], 0, 0);
}

#[test]
fn bulk_eviction_preserves_zero_rounding_clamping_and_empty_boundaries() {
    for pressure in [false, true] {
        let fixture = EvictionFixture::new(EvictionPolicy::Lru);
        let evict = |percent| {
            if pressure {
                fixture.cache.evict_under_pressure(percent)
            } else {
                fixture.cache.clear_least_used(percent)
            }
        };
        assert_eq!(evict(0), 0);
        fixture.assert_remaining(&["a", "b", "c", "d"], 112, 0);
        // Clear rounds the removal count down; pressure rounds the target down.
        assert_eq!(evict(1), usize::from(pressure));
        if pressure {
            fixture.assert_remaining(&["b", "c", "d"], 101, 1);
        } else {
            fixture.assert_remaining(&["a", "b", "c", "d"], 112, 0);
        }
        let remaining = 4 - usize::from(pressure);
        assert_eq!(evict(100), remaining);
        fixture.assert_remaining(&[], 0, 4);
        assert_eq!(evict(u32::MAX), 0);
        fixture.assert_remaining(&[], 0, 4);

        let clamped_fixture = EvictionFixture::new(EvictionPolicy::Lru);
        let clamped_removed = if pressure {
            clamped_fixture.cache.evict_under_pressure(u32::MAX)
        } else {
            clamped_fixture.cache.clear_least_used(u32::MAX)
        };
        assert_eq!(clamped_removed, 4);
        clamped_fixture.assert_remaining(&[], 0, 4);
    }
}
