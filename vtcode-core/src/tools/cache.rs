//! Caching system for tool results

use super::types::{EnhancedCacheEntry, EnhancedCacheStats};
use once_cell::sync::Lazy;
use quick_cache::sync::Cache;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Global file cache instance
pub static FILE_CACHE: Lazy<FileCache> = Lazy::new(|| FileCache::new(1000));

/// Enhanced file cache with quick-cache for high-performance caching
///
/// Uses `tokio::sync::Mutex` for async-safe stats access across `.await` boundaries.
/// See: https://ratatui.rs/faq/#when-should-i-use-tokio-and-async--await-
pub struct FileCache {
    file_cache: Arc<Cache<String, EnhancedCacheEntry<Value>>>,
    directory_cache: Arc<Cache<String, EnhancedCacheEntry<Value>>>,
    stats: Arc<Mutex<EnhancedCacheStats>>,
    max_size_bytes: usize,
    ttl: Duration,
}

impl FileCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            file_cache: Arc::new(Cache::new(capacity)),
            directory_cache: Arc::new(Cache::new(capacity / 2)),
            stats: Arc::new(Mutex::new(EnhancedCacheStats::default())),
            max_size_bytes: 50 * 1024 * 1024, // 50MB default
            ttl: Duration::from_secs(300),    // 5 minutes default
        }
    }

    /// Get cached file content
    pub async fn get_file(&self, key: &str) -> Option<Value> {
        let mut stats = self.stats.lock().await;

        if let Some(entry) = self.file_cache.get(key) {
            // Check if entry is still valid
            if entry.timestamp.elapsed() < self.ttl {
                // Note: quick-cache handles access tracking automatically
                stats.hits += 1;
                return Some(entry.data.clone());
            } else {
                // Entry expired, remove it
                self.file_cache.remove(key);
                stats.expired_evictions += 1;
            }
        }

        stats.misses += 1;
        None
    }

    /// Cache file content
    pub async fn put_file(&self, key: String, value: Value) {
        let size_bytes = serde_json::to_string(&value).unwrap_or_default().len();
        let entry = EnhancedCacheEntry::new(value, size_bytes);

        let mut stats = self.stats.lock().await;

        // Check memory limits (quick-cache handles eviction automatically, but we track stats)
        if stats.total_size_bytes + size_bytes > self.max_size_bytes {
            stats.memory_evictions += 1;
        }

        self.file_cache.insert(key, entry);
        stats.entries = self.file_cache.len();
        stats.total_size_bytes += size_bytes;
    }

    /// Get cached directory listing
    pub async fn get_directory(&self, key: &str) -> Option<Value> {
        let mut stats = self.stats.lock().await;

        if let Some(entry) = self.directory_cache.get(key) {
            if entry.timestamp.elapsed() < self.ttl {
                stats.hits += 1;
                return Some(entry.data.clone());
            } else {
                self.directory_cache.remove(key);
                stats.expired_evictions += 1;
            }
        }

        stats.misses += 1;
        None
    }

    /// Cache directory listing
    pub async fn put_directory(&self, key: String, value: Value) {
        let size_bytes = serde_json::to_string(&value).unwrap_or_default().len();
        let entry = EnhancedCacheEntry::new(value, size_bytes);

        let mut stats = self.stats.lock().await;

        self.directory_cache.insert(key, entry);
        stats.entries += self.directory_cache.len();
        stats.total_size_bytes += size_bytes;
    }

    /// Get cache statistics
    pub async fn stats(&self) -> EnhancedCacheStats {
        self.stats.lock().await.clone()
    }

    /// Clear all caches
    pub async fn clear(&self) {
        self.file_cache.clear();
        self.directory_cache.clear();
        *self.stats.lock().await = EnhancedCacheStats::default();
    }

    /// Get cache capacity information
    pub fn capacity(&self) -> (usize, usize) {
        (
            self.file_cache.capacity().try_into().unwrap_or(0),
            self.directory_cache.capacity().try_into().unwrap_or(0),
        )
    }

    /// Get current cache size
    pub fn len(&self) -> (usize, usize) {
        (self.file_cache.len(), self.directory_cache.len())
    }
}
