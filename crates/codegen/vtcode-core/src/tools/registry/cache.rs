use serde_json::json;

use crate::tools::cache::{FILE_CACHE, FileCache};

use super::ToolRegistry;

impl ToolRegistry {
    /// Access the file cache used by this registry.
    ///
    /// Returns the global `FileCache` instance. In a future phase, this will
    /// return a per-registry cache instance to enable test isolation.
    pub fn file_cache(&self) -> &'static FileCache {
        &FILE_CACHE
    }

    pub async fn cache_stats(&self) -> serde_json::Value {
        let stats = self.file_cache().stats().await;
        json!({
            "hits": stats.hits,
            "misses": stats.misses,
            "entries": stats.entries,
            "file_entries": stats.file_entries,
            "directory_entries": stats.directory_entries,
            "total_size_bytes": stats.total_size_bytes,
            "file_size_bytes": stats.file_size_bytes,
            "directory_size_bytes": stats.directory_size_bytes,
            "hit_rate": if stats.hits + stats.misses > 0 {
                stats.hits as f64 / (stats.hits + stats.misses) as f64
            } else { 0.0 }
        })
    }

    pub async fn clear_cache(&self) {
        self.file_cache().clear().await;
    }
}
