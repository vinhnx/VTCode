//! Caching system for tool results

use super::types::{EnhancedCacheEntry, EnhancedCacheStats};
use once_cell::sync::Lazy;
use quick_cache::sync::Cache;
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::RwLock;

use crate::cache::estimate_json_size;
use vtcode_config::FileReadCacheConfig;

/// Global file cache instance
pub static FILE_CACHE: Lazy<FileCache> = Lazy::new(|| FileCache::new(1000));

static FILE_READ_CACHE_CONFIG: Lazy<RwLock<FileReadCacheConfig>> =
    Lazy::new(|| RwLock::new(FileReadCacheConfig::default()));

/// Enhanced file cache with quick-cache for high-performance caching
///
/// Uses a `parking_lot::Mutex` for stats access — the critical sections contain
/// no `.await`, so the cheaper sync mutex avoids async-mutex overhead on the
/// hot get/put path.
/// Stores `Arc<Value>` internally for zero-copy cache hits.
/// See: <https://ratatui.rs/faq/>
pub struct FileCache {
    file_cache: Arc<Cache<String, EnhancedCacheEntry<Arc<Value>>>>,
    directory_cache: Arc<Cache<String, EnhancedCacheEntry<Arc<Value>>>>,
    stats: Arc<parking_lot::Mutex<EnhancedCacheStats>>,
    max_size_bytes: AtomicUsize,
    ttl_millis: AtomicU64,
}

impl FileCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            file_cache: Arc::new(Cache::new(capacity)),
            directory_cache: Arc::new(Cache::new(capacity / 2)),
            stats: Arc::new(parking_lot::Mutex::new(EnhancedCacheStats::default())),
            max_size_bytes: AtomicUsize::new(50 * 1024 * 1024), // 50MB default
            ttl_millis: AtomicU64::new(300_000),                // 5 minutes default
        }
    }

    #[inline]
    fn ttl(&self) -> Duration {
        Duration::from_millis(self.ttl_millis.load(Ordering::Relaxed))
    }

    #[inline]
    fn max_size_bytes(&self) -> usize {
        self.max_size_bytes.load(Ordering::Relaxed)
    }

    /// Get cached file content (clones the value for backwards compatibility)
    pub async fn get_file(&self, key: &str) -> Option<Value> {
        self.get_file_arc(key).await.map(|arc| (*arc).clone())
    }

    /// Get cached file content as Arc for zero-copy access
    pub async fn get_file_arc(&self, key: &str) -> Option<Arc<Value>> {
        let mut stats = self.stats.lock();

        if let Some(entry) = self.file_cache.get(key) {
            // Check if entry is still valid
            if entry.timestamp.elapsed() < self.ttl() {
                // Note: quick-cache handles access tracking automatically
                stats.hits += 1;
                return Some(Arc::clone(&entry.data));
            } else {
                // Entry expired, remove it
                let size = entry.size_bytes;
                self.file_cache.remove(key);
                stats.expired_evictions += 1;
                stats.total_size_bytes = stats.total_size_bytes.saturating_sub(size);
                stats.file_size_bytes = stats.file_size_bytes.saturating_sub(size);
            }
        }

        stats.misses += 1;
        None
    }

    /// Calculate byte size of a JSON value for cache tracking.
    /// Walks the Value tree without allocating, unlike the previous
    /// implementation that serialized to a temporary String.
    #[inline]
    fn estimate_value_size(value: &Value) -> usize {
        estimate_json_size(value) as usize
    }

    /// Cache file content
    pub fn put_file(&self, key: String, value: Value) -> impl Future<Output = ()> + '_ {
        self.put_file_arc(key, Arc::new(value))
    }

    /// Cache file content with pre-wrapped Arc for zero-copy insertion
    pub async fn put_file_arc(&self, key: String, value: Arc<Value>) {
        let size_bytes = Self::estimate_value_size(&value);
        let entry = EnhancedCacheEntry::new(value, size_bytes);

        self.file_cache.insert(key, entry);

        let mut stats = self.stats.lock();
        stats.file_entries = self.file_cache.len();
        stats.entries = stats.file_entries + stats.directory_entries;
        stats.file_size_bytes += size_bytes;
        stats.total_size_bytes = stats.file_size_bytes + stats.directory_size_bytes;
    }

    /// Get cached directory listing (clones for backwards compatibility)
    pub async fn get_directory(&self, key: &str) -> Option<Value> {
        self.get_directory_arc(key).await.map(|arc| (*arc).clone())
    }

    /// Get cached directory listing as Arc for zero-copy access
    pub async fn get_directory_arc(&self, key: &str) -> Option<Arc<Value>> {
        let mut stats = self.stats.lock();

        if let Some(entry) = self.directory_cache.get(key) {
            if entry.timestamp.elapsed() < self.ttl() {
                stats.hits += 1;
                return Some(Arc::clone(&entry.data));
            } else {
                let size = entry.size_bytes;
                self.directory_cache.remove(key);
                stats.expired_evictions += 1;
                stats.total_size_bytes = stats.total_size_bytes.saturating_sub(size);
                stats.directory_size_bytes = stats.directory_size_bytes.saturating_sub(size);
            }
        }

        stats.misses += 1;
        None
    }

    /// Cache directory listing
    pub fn put_directory(&self, key: String, value: Value) -> impl Future<Output = ()> + '_ {
        self.put_directory_arc(key, Arc::new(value))
    }

    /// Cache directory listing with pre-wrapped Arc
    pub async fn put_directory_arc(&self, key: String, value: Arc<Value>) {
        let size_bytes = Self::estimate_value_size(&value);
        let entry = EnhancedCacheEntry::new(value, size_bytes);

        self.directory_cache.insert(key, entry);

        let mut stats = self.stats.lock();
        stats.directory_entries = self.directory_cache.len();
        stats.entries = stats.file_entries + stats.directory_entries;
        stats.directory_size_bytes += size_bytes;
        stats.total_size_bytes = stats.file_size_bytes + stats.directory_size_bytes;
    }

    /// Get cache statistics
    pub async fn stats(&self) -> EnhancedCacheStats {
        self.stats.lock().clone()
    }

    /// Clear all caches
    pub async fn clear(&self) {
        self.file_cache.clear();
        self.directory_cache.clear();
        *self.stats.lock() = EnhancedCacheStats::default();
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

    /// Check memory pressure and enforce limits with tiered eviction
    pub async fn check_pressure_and_evict(&self) {
        let mut stats = self.stats.lock();

        let current_size = stats.total_size_bytes;
        let max_size = self.max_size_bytes();

        if current_size > max_size {
            // Tier 1: Clear directory cache first (cheaper to rebuild)
            self.directory_cache.clear();
            stats.directory_entries = 0;
            stats.directory_size_bytes = 0;

            // Recalculate total
            stats.total_size_bytes = stats.file_size_bytes;
            stats.entries = stats.file_entries;

            // If still very over limit (e.g. 150%), clear everything
            if stats.total_size_bytes as f64 > max_size as f64 * 1.5 {
                self.file_cache.clear();
                stats.file_entries = 0;
                stats.file_size_bytes = 0;
                stats.total_size_bytes = 0;
                stats.entries = 0;
                stats.memory_evictions += 1;
            } else if stats.total_size_bytes > max_size {
                // Moderately over: clear file cache as safety measure
                self.file_cache.clear();
                stats.file_entries = 0;
                stats.file_size_bytes = 0;
                stats.total_size_bytes = 0;
                stats.entries = 0;
                stats.memory_evictions += 1;
            }
        } else if current_size as f64 > max_size as f64 * 0.9 {
            // Tier 3: Soft limit - proactive directory pruning
            self.directory_cache.clear();
            stats.directory_entries = 0;
            stats.directory_size_bytes = 0;
            stats.total_size_bytes = stats.file_size_bytes;
            stats.entries = stats.file_entries;
        }
    }

    /// Set explicit memory limit in bytes
    pub fn set_capacity_limit(&self, max_bytes: usize) {
        self.max_size_bytes.store(max_bytes, Ordering::Relaxed);
    }

    /// Update cache policy from configuration
    pub fn apply_read_cache_config(&self, config: &FileReadCacheConfig) {
        self.max_size_bytes
            .store(config.max_size_bytes, Ordering::Relaxed);
        self.ttl_millis
            .store(config.ttl_secs.saturating_mul(1000), Ordering::Relaxed);
    }
}

/// Configure global file cache from optimization settings.
pub fn configure_file_cache(config: &FileReadCacheConfig) {
    *FILE_READ_CACHE_CONFIG.write() = config.clone();
    FILE_CACHE.apply_read_cache_config(config);
}

/// Get a clone of the current file read cache config
pub fn file_read_cache_config() -> FileReadCacheConfig {
    FILE_READ_CACHE_CONFIG.read().clone()
}
