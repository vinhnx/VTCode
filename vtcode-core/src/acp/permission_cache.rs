/// ACP Permission caching - avoid re-prompting for same file in same session
///
/// Caches file-level permission grants so the agent doesn't repeatedly
/// ask the user for access to the same file during a single session.
use std::collections::HashMap;
use std::path::PathBuf;

/// Permission grant decision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionGrant {
    /// Allow for this operation only
    Once,
    /// Allow for remainder of session
    Session,
    /// Allow permanently (stored to disk)
    Permanent,
    /// Explicitly denied
    Denied,
}

/// Session-scoped permission cache for ACP hosts (Zed, VS Code, etc.)
#[derive(Debug)]
pub struct AcpPermissionCache {
    /// Map of file path → permission grant
    /// Keyed by canonical path to avoid duplicates
    grants: HashMap<PathBuf, PermissionGrant>,
    /// Track total cache hits for metrics
    hits: usize,
    /// Track total cache misses for metrics
    misses: usize,
}

impl AcpPermissionCache {
    /// Create new permission cache for session
    pub fn new() -> Self {
        Self {
            grants: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// Check if we have a cached permission for this file
    pub fn get_permission(&mut self, path: &PathBuf) -> Option<PermissionGrant> {
        if let Some(grant) = self.grants.get(path) {
            self.hits += 1;
            Some(*grant)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Cache a permission grant
    pub fn cache_grant(&mut self, path: PathBuf, grant: PermissionGrant) {
        self.grants.insert(path, grant);
    }

    /// Invalidate permission for a file (e.g., when workspace trust changes)
    pub fn invalidate(&mut self, path: &PathBuf) {
        self.grants.remove(path);
    }

    /// Clear all cached permissions
    pub fn clear(&mut self) {
        self.grants.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// Get cache statistics
    pub fn stats(&self) -> PermissionCacheStats {
        let total_requests = self.hits + self.misses;
        let hit_rate = if total_requests > 0 {
            (self.hits as f64) / (total_requests as f64)
        } else {
            0.0
        };

        PermissionCacheStats {
            cached_entries: self.grants.len(),
            hits: self.hits,
            misses: self.misses,
            total_requests,
            hit_rate,
        }
    }

    /// Check if path is denied (skip prompting)
    pub fn is_denied(&self, path: &PathBuf) -> bool {
        self.grants
            .get(path)
            .map(|g| *g == PermissionGrant::Denied)
            .unwrap_or(false)
    }

    /// Check if we can use cached permission without prompting
    pub fn can_use_cached(&self, path: &PathBuf) -> bool {
        matches!(
            self.grants.get(path),
            Some(PermissionGrant::Session | PermissionGrant::Permanent | PermissionGrant::Denied)
        )
    }
}

impl Default for AcpPermissionCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct PermissionCacheStats {
    pub cached_entries: usize,
    pub hits: usize,
    pub misses: usize,
    pub total_requests: usize,
    pub hit_rate: f64,
}

/// Tool-level permission cache - caches approvals for tool execution
/// (different from file-level ACP permissions)
#[derive(Debug)]
pub struct ToolPermissionCache {
    /// Map of tool name → permission grant
    grants: HashMap<String, PermissionGrant>,
    /// Track total cache hits for metrics
    hits: usize,
    /// Track total cache misses for metrics
    misses: usize,
}

impl ToolPermissionCache {
    /// Create new tool permission cache for session
    pub fn new() -> Self {
        Self {
            grants: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// Check if we have a cached permission for this tool
    pub fn get_permission(&mut self, tool_name: &str) -> Option<PermissionGrant> {
        if let Some(grant) = self.grants.get(tool_name) {
            self.hits += 1;
            Some(*grant)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Cache a permission grant for a tool
    pub fn cache_grant(&mut self, tool_name: impl Into<String>, grant: PermissionGrant) {
        self.grants.insert(tool_name.into(), grant);
    }

    /// Invalidate permission for a tool
    pub fn invalidate(&mut self, tool_name: &str) {
        self.grants.remove(tool_name);
    }

    /// Clear all cached permissions
    pub fn clear(&mut self) {
        self.grants.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// Check if tool execution is denied (skip prompting)
    pub fn is_denied(&self, tool_name: &str) -> bool {
        self.grants
            .get(tool_name)
            .map(|g| *g == PermissionGrant::Denied)
            .unwrap_or(false)
    }

    /// Check if we can use cached permission without prompting
    pub fn can_use_cached(&self, tool_name: &str) -> bool {
        matches!(
            self.grants.get(tool_name),
            Some(PermissionGrant::Session | PermissionGrant::Permanent | PermissionGrant::Denied)
        )
    }

    /// Get cache statistics
    pub fn stats(&self) -> ToolPermissionCacheStats {
        let total_requests = self.hits + self.misses;
        let hit_rate = if total_requests > 0 {
            (self.hits as f64) / (total_requests as f64)
        } else {
            0.0
        };

        ToolPermissionCacheStats {
            cached_entries: self.grants.len(),
            hits: self.hits,
            misses: self.misses,
            total_requests,
            hit_rate,
        }
    }
}

impl Default for ToolPermissionCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool permission cache statistics
#[derive(Debug, Clone)]
pub struct ToolPermissionCacheStats {
    pub cached_entries: usize,
    pub hits: usize,
    pub misses: usize,
    pub total_requests: usize,
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(name: &str) -> PathBuf {
        PathBuf::from(format!("/workspace/{}", name))
    }

    #[test]
    fn test_creates_empty_cache() {
        let cache = AcpPermissionCache::new();
        let stats = cache.stats();
        assert_eq!(stats.cached_entries, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_caches_permission_grant() {
        let mut cache = AcpPermissionCache::new();
        let path = test_path("file.rs");

        cache.cache_grant(path.clone(), PermissionGrant::Session);
        assert_eq!(cache.get_permission(&path), Some(PermissionGrant::Session));
    }

    #[test]
    fn test_tracks_hits_and_misses() {
        let mut cache = AcpPermissionCache::new();
        let path = test_path("file.rs");

        cache.cache_grant(path.clone(), PermissionGrant::Session);

        // Hit
        let _ = cache.get_permission(&path);
        assert_eq!(cache.stats().hits, 1);

        // Miss
        let _ = cache.get_permission(&test_path("other.rs"));
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn test_calculates_hit_rate() {
        let mut cache = AcpPermissionCache::new();
        let path1 = test_path("file1.rs");
        let path2 = test_path("file2.rs");

        cache.cache_grant(path1.clone(), PermissionGrant::Session);

        // 3 hits
        cache.get_permission(&path1);
        cache.get_permission(&path1);
        cache.get_permission(&path1);

        // 1 miss
        cache.get_permission(&path2);

        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.total_requests, 4);
        assert!((stats.hit_rate - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_invalidates_path() {
        let mut cache = AcpPermissionCache::new();
        let path = test_path("file.rs");

        cache.cache_grant(path.clone(), PermissionGrant::Session);
        assert!(cache.get_permission(&path).is_some());

        cache.invalidate(&path);
        assert!(cache.get_permission(&path).is_none());
    }

    #[test]
    fn test_clears_all() {
        let mut cache = AcpPermissionCache::new();

        cache.cache_grant(test_path("file1.rs"), PermissionGrant::Session);
        cache.cache_grant(test_path("file2.rs"), PermissionGrant::Session);
        cache.get_permission(&test_path("file1.rs"));

        cache.clear();
        let stats = cache.stats();
        assert_eq!(stats.cached_entries, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_identifies_denied_paths() {
        let mut cache = AcpPermissionCache::new();
        let denied_path = test_path("secret.txt");
        let allowed_path = test_path("public.txt");

        cache.cache_grant(denied_path.clone(), PermissionGrant::Denied);
        cache.cache_grant(allowed_path.clone(), PermissionGrant::Session);

        assert!(cache.is_denied(&denied_path));
        assert!(!cache.is_denied(&allowed_path));
        assert!(!cache.is_denied(&test_path("unknown.txt")));
    }

    #[test]
    fn test_can_use_cached_for_session_grants() {
        let mut cache = AcpPermissionCache::new();
        let once_path = test_path("once.rs");
        let session_path = test_path("session.rs");
        let denied_path = test_path("denied.rs");

        cache.cache_grant(once_path.clone(), PermissionGrant::Once);
        cache.cache_grant(session_path.clone(), PermissionGrant::Session);
        cache.cache_grant(denied_path.clone(), PermissionGrant::Denied);

        // "Once" grants can't be reused
        assert!(!cache.can_use_cached(&once_path));

        // Session and Permanent grants can be reused
        assert!(cache.can_use_cached(&session_path));
        assert!(cache.can_use_cached(&denied_path));
    }

    #[test]
    fn test_multiple_paths() {
        let mut cache = AcpPermissionCache::new();

        for i in 0..5 {
            cache.cache_grant(
                test_path(&format!("file{}.rs", i)),
                PermissionGrant::Session,
            );
        }

        assert_eq!(cache.stats().cached_entries, 5);

        for i in 0..5 {
            let grant = cache.get_permission(&test_path(&format!("file{}.rs", i)));
            assert_eq!(grant, Some(PermissionGrant::Session));
        }

        assert_eq!(cache.stats().hits, 5);
    }
}
