/// Caching Layer for Performance Optimization
///
/// Implements intelligent caching for workspace analysis, file content, and command results.
/// Provides cache invalidation strategies and memory-efficient storage.
use std::collections::HashMap;
use std::sync::Arc;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Cache entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry<T: Clone> {
    /// Cached value (Arc to avoid cloning heavy values)
    pub value: Arc<T>,
    /// Timestamp when cached
    pub created_at: u64,
    /// Last accessed timestamp
    pub accessed_at: u64,
    /// Time-to-live in seconds
    pub ttl_seconds: u64,
    /// Number of accesses
    pub access_count: usize,
}

impl<T: Clone> CacheEntry<T> {
    /// Create a new cache entry with default TTL (1 hour)
    pub fn new(value: T) -> Self {
        let now = now_timestamp();
        Self {
            value: Arc::new(value),
            created_at: now,
            accessed_at: now,
            ttl_seconds: 3600,
            access_count: 0,
        }
    }

    /// Create a cache entry with custom TTL
    pub fn with_ttl(value: T, ttl_seconds: u64) -> Self {
        let now = now_timestamp();
        Self {
            value: Arc::new(value),
            created_at: now,
            accessed_at: now,
            ttl_seconds,
            access_count: 0,
        }
    }

    /// Construct from an existing Arc<T> without cloning the underlying value
    pub fn from_arc(value: Arc<T>) -> Self {
        let now = now_timestamp();
        Self {
            value,
            created_at: now,
            accessed_at: now,
            ttl_seconds: 3600,
            access_count: 0,
        }
    }

    /// Construct from an existing Arc<T> with a specific TTL
    pub fn from_arc_with_ttl(value: Arc<T>, ttl_seconds: u64) -> Self {
        let now = now_timestamp();
        Self {
            value,
            created_at: now,
            accessed_at: now,
            ttl_seconds,
            access_count: 0,
        }
    }

    /// Check if entry is expired
    pub fn is_expired(&self) -> bool {
        let now = now_timestamp();
        now > self.created_at + self.ttl_seconds
    }

    /// Get time remaining before expiration (in seconds)
    pub fn time_to_live(&self) -> u64 {
        let now = now_timestamp();
        let expiry = self.created_at + self.ttl_seconds;
        expiry.saturating_sub(now)
    }

    /// Update access metadata
    pub fn touch(&mut self) {
        self.accessed_at = now_timestamp();
        self.access_count += 1;
    }
}

/// Get current Unix timestamp
fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Workspace analysis cache
#[derive(Debug, Clone)]
pub struct WorkspaceAnalysisCache {
    /// Map of workspace root to cached analysis
    cache: HashMap<PathBuf, CacheEntry<WorkspaceAnalysisData>>,
    /// Maximum cache size in MB
    max_size_mb: usize,
    /// Current cache size in MB
    current_size_mb: usize,
}

/// Cached workspace analysis data
#[derive(Debug, Clone)]
pub struct WorkspaceAnalysisData {
    /// Number of files
    pub file_count: usize,
    /// Number of directories
    pub dir_count: usize,
    /// Languages found
    pub languages: Vec<String>,
    /// Total size in bytes
    pub total_size: u64,
    /// Config files found
    pub config_files: Vec<PathBuf>,
}

impl WorkspaceAnalysisCache {
    /// Create a new workspace analysis cache
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            max_size_mb: 100,
            current_size_mb: 0,
        }
    }

    /// Create with custom size limit
    pub fn with_size_limit(max_mb: usize) -> Self {
        Self {
            cache: HashMap::new(),
            max_size_mb: max_mb,
            current_size_mb: 0,
        }
    }

    /// Cache workspace analysis
    pub fn cache_analysis(&mut self, workspace: PathBuf, data: WorkspaceAnalysisData) {
        let entry = CacheEntry::with_ttl(data, 1800); // 30 minutes TTL
        self.cache.insert(workspace, entry);
    }

    /// Insert an Arc-wrapped analysis entry to avoid cloning large analysis structs
    pub fn cache_analysis_arc(&mut self, workspace: PathBuf, data: Arc<WorkspaceAnalysisData>) {
        let entry = CacheEntry::from_arc_with_ttl(data, 1800);
        self.cache.insert(workspace, entry);
    }

    /// Get cached analysis
    pub fn get_analysis(&mut self, workspace: &PathBuf) -> Option<WorkspaceAnalysisData> {
        if let Some(entry) = self.cache.get_mut(workspace) {
            if !entry.is_expired() {
                entry.touch();
                return Some((*entry.value).clone());
            } else {
                // Remove expired entry
                self.cache.remove(workspace);
            }
        }
        None
    }

    /// Return an Arc reference to the cached analysis to avoid cloning.
    pub fn get_analysis_shared(&mut self, workspace: &PathBuf) -> Option<Arc<WorkspaceAnalysisData>> {
        if let Some(entry) = self.cache.get_mut(workspace) {
            if !entry.is_expired() {
                entry.touch();
                return Some(Arc::clone(&entry.value));
            } else {
                self.cache.remove(workspace);
            }
        }
        None
    }

    /// Clear all cache
    pub fn clear(&mut self) {
        self.cache.clear();
        self.current_size_mb = 0;
    }

    /// Remove expired entries
    pub fn prune_expired(&mut self) {
        self.cache.retain(|_, entry| !entry.is_expired());
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.cache.len(),
            size_mb: self.current_size_mb,
            max_size_mb: self.max_size_mb,
            utilization: if self.max_size_mb > 0 {
                (self.current_size_mb as f32 / self.max_size_mb as f32) * 100.0
            } else {
                0.0
            },
        }
    }
}

impl Default for WorkspaceAnalysisCache {
    fn default() -> Self {
        Self::new()
    }
}

/// File content cache with size limits
#[derive(Debug, Clone)]
pub struct FileContentCache {
    /// Map of file path to cached content
    cache: HashMap<PathBuf, CacheEntry<String>>,
    /// Maximum cache size in MB
    max_size_mb: usize,
    /// Current cache size in MB
    current_size_mb: usize,
}

impl FileContentCache {
    /// Create a new file content cache
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            max_size_mb: 50,
            current_size_mb: 0,
        }
    }

    /// Create with custom size limit
    pub fn with_size_limit(max_mb: usize) -> Self {
        Self {
            cache: HashMap::new(),
            max_size_mb: max_mb,
            current_size_mb: 0,
        }
    }

    /// Cache file content
    pub fn cache_content(&mut self, path: PathBuf, content: String) {
        let content_size = (content.len() as f32 / 1024.0 / 1024.0).ceil() as usize;

        // Evict least recently used if cache is full
        if self.current_size_mb + content_size > self.max_size_mb {
            self._evict_lru();
        }

        let entry = CacheEntry::with_ttl(content, 600); // 10 minutes TTL
        self.cache.insert(path, entry);
        self.current_size_mb += content_size;
    }

    /// Insert an Arc-wrapped content value to avoid cloning the String value
    pub fn cache_content_arc(&mut self, path: PathBuf, content: Arc<String>) {
        let content_size = (content.len() as f32 / 1024.0 / 1024.0).ceil() as usize;
        if self.current_size_mb + content_size > self.max_size_mb {
            self._evict_lru();
        }
        let entry = CacheEntry::from_arc_with_ttl(content, 600);
        self.cache.insert(path, entry);
        self.current_size_mb += content_size;
    }

    /// Get cached content
    pub fn get_content(&mut self, path: &PathBuf) -> Option<String> {
        if let Some(entry) = self.cache.get_mut(path) {
            if !entry.is_expired() {
                entry.touch();
                return Some((*entry.value).clone());
            } else {
                self.cache.remove(path);
            }
        }
        None
    }

    /// Return a shared Arc<String> reference to cached content to avoid copies.
    pub fn get_content_shared(&mut self, path: &PathBuf) -> Option<Arc<String>> {
        if let Some(entry) = self.cache.get_mut(path) {
            if !entry.is_expired() {
                entry.touch();
                return Some(Arc::clone(&entry.value));
            } else {
                self.cache.remove(path);
            }
        }
        None
    }

    /// Clear all cache
    pub fn clear(&mut self) {
        self.cache.clear();
        self.current_size_mb = 0;
    }

    /// Remove expired entries
    pub fn prune_expired(&mut self) {
        let before_count = self.cache.len();
        self.cache.retain(|_, entry| !entry.is_expired());
        let after_count = self.cache.len();

        // Recalculate size (simplified - assumes roughly equal size)
        if after_count == 0 {
            self.current_size_mb = 0;
        } else {
            self.current_size_mb =
                (self.current_size_mb as f32 * after_count as f32 / before_count as f32) as usize;
        }
    }

    /// Evict least recently used entry
    fn _evict_lru(&mut self) {
        if let Some((path, _)) = self
            .cache
            .iter()
            .min_by_key(|(_, entry)| entry.accessed_at)
            .map(|(k, v)| (k.clone(), v.clone()))
        {
            self.cache.remove(&path);
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.cache.len(),
            size_mb: self.current_size_mb,
            max_size_mb: self.max_size_mb,
            utilization: if self.max_size_mb > 0 {
                (self.current_size_mb as f32 / self.max_size_mb as f32) * 100.0
            } else {
                0.0
            },
        }
    }
}

impl Default for FileContentCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Command result cache
#[derive(Debug, Clone)]
pub struct CommandResultCache {
    /// Map of command hash to cached result
    cache: HashMap<String, CacheEntry<String>>,
    /// Maximum number of entries
    max_entries: usize,
}

impl CommandResultCache {
    /// Create a new command result cache
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            max_entries: 100,
        }
    }

    /// Cache command result
    pub fn cache_result(&mut self, key: String, result: String) {
        if self.cache.len() >= self.max_entries {
            // Evict oldest entry
            if let Some((oldest_key, _)) = self
                .cache
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(k, v)| (k.clone(), v.clone()))
            {
                self.cache.remove(&oldest_key);
            }
        }

        let entry = CacheEntry::with_ttl(result, 3600); // 1 hour TTL
        self.cache.insert(key, entry);
    }

    /// Insert an Arc-wrapped command result to avoid cloning
    pub fn cache_result_arc(&mut self, key: String, result: Arc<String>) {
        if self.cache.len() >= self.max_entries {
            if let Some((oldest_key, _)) = self
                .cache
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(k, v)| (k.clone(), v.clone()))
            {
                self.cache.remove(&oldest_key);
            }
        }
        let entry = CacheEntry::from_arc_with_ttl(result, 3600);
        self.cache.insert(key, entry);
    }

    /// Get cached result
    pub fn get_result(&mut self, key: &str) -> Option<String> {
        if let Some(entry) = self.cache.get_mut(key) {
            if !entry.is_expired() {
                entry.touch();
                return Some((*entry.value).clone());
            } else {
                self.cache.remove(key);
            }
        }
        None
    }

    /// Get a shared Arc<String> reference to the cached result to avoid clones.
    pub fn get_result_shared(&mut self, key: &str) -> Option<Arc<String>> {
        if let Some(entry) = self.cache.get_mut(key) {
            if !entry.is_expired() {
                entry.touch();
                return Some(Arc::clone(&entry.value));
            } else {
                self.cache.remove(key);
            }
        }
        None
    }

    /// Clear all cache
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Remove expired entries
    pub fn prune_expired(&mut self) {
        self.cache.retain(|_, entry| !entry.is_expired());
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.cache.len(),
            size_mb: 0, // Simplified
            max_size_mb: 0,
            utilization: (self.cache.len() as f32 / self.max_entries as f32) * 100.0,
        }
    }
}

impl Default for CommandResultCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of entries
    pub entries: usize,
    /// Current size in MB
    pub size_mb: usize,
    /// Maximum size in MB
    pub max_size_mb: usize,
    /// Cache utilization percentage
    pub utilization: f32,
}

impl CacheStats {
    /// Check if cache is nearly full
    pub fn is_nearly_full(&self) -> bool {
        self.utilization > 80.0
    }

    /// Format for display
    pub fn format_display(&self) -> String {
        format!(
            "Cache: {} entries, {} MB / {} MB ({:.1}%)",
            self.entries, self.size_mb, self.max_size_mb, self.utilization
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_entry_creation() {
        let entry = CacheEntry::new("test".to_string());
        assert_eq!(entry.value, "test");
        assert_eq!(entry.access_count, 0);
    }

    #[test]
    fn test_cache_entry_with_ttl() {
        let entry = CacheEntry::with_ttl("test".to_string(), 60);
        assert_eq!(entry.ttl_seconds, 60);
    }

    #[test]
    fn test_cache_entry_not_expired() {
        let entry = CacheEntry::with_ttl("test".to_string(), 3600);
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_cache_entry_touch() {
        let mut entry = CacheEntry::new("test".to_string());
        let initial_count = entry.access_count;
        entry.touch();
        assert_eq!(entry.access_count, initial_count + 1);
    }

    #[test]
    fn test_workspace_cache_creation() {
        let cache = WorkspaceAnalysisCache::new();
        assert_eq!(cache.cache.len(), 0);
    }

    #[test]
    fn test_workspace_cache_store_and_retrieve() {
        let mut cache = WorkspaceAnalysisCache::new();
        let path = PathBuf::from("/test/workspace");
        let data = WorkspaceAnalysisData {
            file_count: 100,
            dir_count: 10,
            languages: vec!["rust".to_string()],
            total_size: 1024,
            config_files: vec![],
        };

        cache.cache_analysis_arc(path.clone(), Arc::new(data.clone()));
        let retrieved = cache.get_analysis(&path);

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().file_count, 100);
    }

    #[test]
    fn test_workspace_cache_store_and_retrieve_arc() {
        let mut cache = WorkspaceAnalysisCache::new();
        let path = PathBuf::from("/test/workspace");
        let data = Arc::new(WorkspaceAnalysisData {
            file_count: 200,
            dir_count: 20,
            languages: vec!["rust".to_string()],
            total_size: 2048,
            config_files: vec![],
        });

        cache.cache_analysis_arc(path.clone(), Arc::clone(&data));
        let retrieved = cache.get_analysis_shared(&path);

        assert!(retrieved.is_some());
        assert_eq!(Arc::strong_count(&retrieved.unwrap()), 2);
    }

    #[test]
    fn test_workspace_cache_clear() {
        let mut cache = WorkspaceAnalysisCache::new();
        let path = PathBuf::from("/test");
        let data = WorkspaceAnalysisData {
            file_count: 10,
            dir_count: 2,
            languages: vec![],
            total_size: 0,
            config_files: vec![],
        };

        cache.cache_analysis_arc(path, Arc::new(data));
        assert_eq!(cache.cache.len(), 1);

        cache.clear();
        assert_eq!(cache.cache.len(), 0);
    }

    #[test]
    fn test_file_content_cache_creation() {
        let cache = FileContentCache::new();
        assert_eq!(cache.cache.len(), 0);
    }

    #[test]
    fn test_file_content_cache_store_and_retrieve() {
        let mut cache = FileContentCache::new();
        let path = PathBuf::from("/test/file.rs");
        let content = "fn main() {}".to_string();

        cache.cache_content_arc(path.clone(), Arc::new(content.clone()));
        let retrieved = cache.get_content(&path);

        assert_eq!(retrieved, Some(content));
    }

    #[test]
    fn test_file_content_cache_store_and_retrieve_arc() {
        let mut cache = FileContentCache::new();
        let path = PathBuf::from("/test/file.rs");
        let content = Arc::new("fn main() {}".to_string());

        cache.cache_content_arc(path.clone(), Arc::clone(&content));
        let retrieved = cache.get_content_shared(&path);

        assert!(retrieved.is_some());
        assert_eq!(Arc::strong_count(&retrieved.unwrap()), 2);
    }

    #[test]
    fn test_command_result_cache_creation() {
        let cache = CommandResultCache::new();
        assert_eq!(cache.cache.len(), 0);
    }

    #[test]
    fn test_command_result_cache_store_and_retrieve() {
        let mut cache = CommandResultCache::new();
        let key = "command_hash".to_string();
        let result = "result".to_string();

        cache.cache_result_arc(key.clone(), Arc::new(result.clone()));
        let retrieved = cache.get_result(&key);

        assert_eq!(retrieved, Some(result));
    }

    #[test]
    fn test_command_result_cache_store_and_retrieve_arc() {
        let mut cache = CommandResultCache::new();
        let key = "command_hash".to_string();
        let result = Arc::new("result".to_string());

        cache.cache_result_arc(key.clone(), Arc::clone(&result));
        let retrieved = cache.get_result_shared(&key);

        assert!(retrieved.is_some());
        assert_eq!(Arc::strong_count(&retrieved.unwrap()), 2);
    }

    #[test]
    fn test_cache_stats() {
        let cache = WorkspaceAnalysisCache::new();
        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
    }

    #[test]
    fn test_cache_stats_nearly_full() {
        let mut cache = WorkspaceAnalysisCache::with_size_limit(100);
        cache.current_size_mb = 85;
        let stats = cache.stats();
        assert!(stats.is_nearly_full());
    }

    #[test]
    fn test_cache_stats_format() {
        let stats = CacheStats {
            entries: 10,
            size_mb: 50,
            max_size_mb: 100,
            utilization: 50.0,
        };
        let display = stats.format_display();
        assert!(display.contains("10 entries"));
        assert!(display.contains("50 MB"));
    }
}
