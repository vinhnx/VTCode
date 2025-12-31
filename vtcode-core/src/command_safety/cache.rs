//! Caching layer for command safety decisions.
//!
//! Caches safety decisions to avoid re-evaluating the same commands.
//! Implements LRU eviction when cache exceeds size limit.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Cached safety decision
#[derive(Clone, Debug)]
pub struct CachedDecision {
    /// True if command is safe
    pub is_safe: bool,
    /// Reason for decision
    pub reason: String,
    /// Access count (for LRU)
    pub access_count: u64,
}

/// Thread-safe cache for command safety decisions
pub struct SafetyDecisionCache {
    cache: Arc<Mutex<HashMap<String, CachedDecision>>>,
    max_size: usize,
}

impl SafetyDecisionCache {
    /// Creates a new cache with given max size
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            max_size,
        }
    }

    /// Creates a default cache (1000 entries)
    pub fn default() -> Self {
        Self::new(1000)
    }

    /// Gets a cached decision
    pub async fn get(&self, command: &str) -> Option<CachedDecision> {
        let mut cache = self.cache.lock().await;
        if let Some(decision) = cache.get_mut(command) {
            decision.access_count += 1;
            return Some(decision.clone());
        }
        None
    }

    /// Sets a cached decision
    pub async fn put(&self, command: String, is_safe: bool, reason: String) {
        let mut cache = self.cache.lock().await;

        // Evict least-used entry if cache is full
        if cache.len() >= self.max_size && !cache.contains_key(&command) {
            if let Some(least_used) = cache
                .iter()
                .min_by_key(|(_, decision)| decision.access_count)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&least_used);
            }
        }

        cache.insert(
            command,
            CachedDecision {
                is_safe,
                reason,
                access_count: 1,
            },
        );
    }

    /// Clears all cached entries
    pub async fn clear(&self) {
        let mut cache = self.cache.lock().await;
        cache.clear();
    }

    /// Returns current cache size
    pub async fn size(&self) -> usize {
        let cache = self.cache.lock().await;
        cache.len()
    }

    /// Returns cache hit rate statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.lock().await;
        let total_accesses: u64 = cache.values().map(|d| d.access_count).sum();
        let entry_count = cache.len();

        CacheStats {
            entry_count,
            total_accesses,
            avg_access_per_entry: if entry_count > 0 {
                total_accesses / entry_count as u64
            } else {
                0
            },
        }
    }
}

impl Clone for SafetyDecisionCache {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            max_size: self.max_size,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entry_count: usize,
    pub total_accesses: u64,
    pub avg_access_per_entry: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_stores_and_retrieves() {
        let cache = SafetyDecisionCache::new(10);
        cache.put("git status".to_string(), true, "git status allowed".to_string()).await;

        let decision = cache.get("git status").await;
        assert!(decision.is_some());
        assert!(decision.unwrap().is_safe);
    }

    #[tokio::test]
    async fn cache_returns_none_for_missing_key() {
        let cache = SafetyDecisionCache::new(10);
        let decision = cache.get("missing").await;
        assert!(decision.is_none());
    }

    #[tokio::test]
    async fn cache_tracks_access_count() {
        let cache = SafetyDecisionCache::new(10);
        cache.put("cmd".to_string(), true, "allowed".to_string()).await;

        let d1 = cache.get("cmd").await.unwrap();
        assert_eq!(d1.access_count, 1);

        let d2 = cache.get("cmd").await.unwrap();
        assert_eq!(d2.access_count, 2);
    }

    #[tokio::test]
    async fn cache_respects_max_size() {
        let cache = SafetyDecisionCache::new(3);

        cache.put("cmd1".to_string(), true, "allowed".to_string()).await;
        cache.put("cmd2".to_string(), true, "allowed".to_string()).await;
        cache.put("cmd3".to_string(), true, "allowed".to_string()).await;

        assert_eq!(cache.size().await, 3);

        // Adding a 4th entry should evict the least-used
        cache.put("cmd4".to_string(), true, "allowed".to_string()).await;
        assert_eq!(cache.size().await, 3);
    }

    #[tokio::test]
    async fn cache_clears() {
        let cache = SafetyDecisionCache::new(10);
        cache.put("cmd".to_string(), true, "allowed".to_string()).await;
        assert_eq!(cache.size().await, 1);

        cache.clear().await;
        assert_eq!(cache.size().await, 0);
    }

    #[tokio::test]
    async fn cache_stats() {
        let cache = SafetyDecisionCache::new(10);
        cache.put("cmd1".to_string(), true, "allowed".to_string()).await;
        cache.put("cmd2".to_string(), true, "allowed".to_string()).await;

        let _d1 = cache.get("cmd1").await;
        let _d2 = cache.get("cmd2").await;
        let _d3 = cache.get("cmd2").await;

        let stats = cache.stats().await;
        assert_eq!(stats.entry_count, 2);
        assert_eq!(stats.total_accesses, 5); // 1+1 initial puts + 1+2 gets
    }
}
