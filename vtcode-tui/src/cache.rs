use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(120);

pub trait CacheKey: Eq + Hash + Clone + Send + Sync + 'static {}
impl<T> CacheKey for T where T: Eq + Hash + Clone + Send + Sync + 'static {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicy {
    Lru,
}

struct CacheEntry<V> {
    value: Arc<V>,
    inserted_at: Instant,
}

pub struct UnifiedCache<K, V>
where
    K: CacheKey,
    V: Clone + Send + Sync + 'static,
{
    capacity: usize,
    ttl: Duration,
    policy: EvictionPolicy,
    entries: HashMap<K, CacheEntry<V>>,
    lru_order: VecDeque<K>,
}

impl<K, V> UnifiedCache<K, V>
where
    K: CacheKey,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(capacity: usize, ttl: Duration, policy: EvictionPolicy) -> Self {
        Self {
            capacity,
            ttl,
            policy,
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
        }
    }

    pub fn insert(&mut self, key: K, value: V, _estimated_size: u64) {
        if self.capacity == 0 {
            return;
        }

        self.prune_expired();
        self.touch_key(&key);

        self.entries.insert(
            key.clone(),
            CacheEntry {
                value: Arc::new(value),
                inserted_at: Instant::now(),
            },
        );

        self.lru_order.push_back(key);
        self.enforce_capacity();
    }

    pub fn get(&mut self, key: &K) -> Option<Arc<V>> {
        self.prune_expired();

        if self.entries.contains_key(key) {
            self.touch_key(key);
            self.lru_order.push_back(key.clone());
        }

        self.entries.get(key).map(|entry| Arc::clone(&entry.value))
    }

    pub fn get_owned(&mut self, key: &K) -> Option<V> {
        self.get(key).as_deref().cloned()
    }

    fn enforce_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.lru_order.pop_front() {
                if self.entries.remove(&oldest).is_some() {
                    break;
                }
            } else {
                break;
            }
        }

        if matches!(self.policy, EvictionPolicy::Lru) {
            while self.entries.len() > self.capacity {
                if let Some(oldest) = self.lru_order.pop_front() {
                    self.entries.remove(&oldest);
                } else {
                    break;
                }
            }
        }
    }

    fn prune_expired(&mut self) {
        let ttl = self.ttl;
        self.entries
            .retain(|_, entry| entry.inserted_at.elapsed() < ttl);
        self.lru_order.retain(|key| self.entries.contains_key(key));
    }

    fn touch_key(&mut self, key: &K) {
        self.lru_order.retain(|k| k != key);
    }
}
