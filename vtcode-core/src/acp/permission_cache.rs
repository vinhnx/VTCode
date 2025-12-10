/// ACP Permission caching - avoid re-prompting for same file in same session
///
/// Caches file-level permission grants so the agent doesn't repeatedly
/// ask the user for access to the same file during a single session.
///
/// This module uses a generic `PermissionCache<K>` to eliminate duplicate code
/// between file-based and tool-based permission caching.
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
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
    /// Explicitly denied by policy
    Denied,
    /// Temporary denial from execution failure (not policy-based)
    /// Should be invalidated on retry
    TemporaryDenial,
}

/// Cache statistics (shared between all permission cache types)
#[derive(Debug, Clone, Default)]
pub struct PermissionCacheStats {
    pub cached_entries: usize,
    pub hits: usize,
    pub misses: usize,
    pub total_requests: usize,
    pub hit_rate: f64,
}

impl PermissionCacheStats {
    #[inline]
    fn compute(entries: usize, hits: usize, misses: usize) -> Self {
        let total_requests = hits + misses;
        let hit_rate = if total_requests > 0 {
            (hits as f64) / (total_requests as f64)
        } else {
            0.0
        };
        Self {
            cached_entries: entries,
            hits,
            misses,
            total_requests,
            hit_rate,
        }
    }
}

/// Generic permission cache that works for any hashable key type.
/// Eliminates duplication between file-based and tool-based caches.
#[derive(Debug)]
pub struct PermissionCache<K: Eq + Hash> {
    grants: HashMap<K, PermissionGrant>,
    hits: usize,
    misses: usize,
}

impl<K: Eq + Hash> PermissionCache<K> {
    /// Create new permission cache
    #[inline]
    pub fn new() -> Self {
        Self {
            grants: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// Check if we have a cached permission for this key
    #[inline]
    pub fn get_permission<Q>(&mut self, key: &Q) -> Option<PermissionGrant>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(grant) = self.grants.get(key) {
            self.hits += 1;
            Some(*grant)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Cache a permission grant
    #[inline]
    pub fn cache_grant(&mut self, key: K, grant: PermissionGrant) {
        self.grants.insert(key, grant);
    }

    /// Invalidate permission for a key
    #[inline]
    pub fn invalidate<Q>(&mut self, key: &Q)
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.grants.remove(key);
    }

    /// Clear only temporary denials (for retries)
    pub fn clear_temporary_denials(&mut self) {
        self.grants
            .retain(|_, grant| *grant != PermissionGrant::TemporaryDenial);
    }

    /// Clear all cached permissions
    pub fn clear(&mut self) {
        self.grants.clear();
        self.hits = 0;
        self.misses = 0;
    }

    /// Get cache statistics
    #[inline]
    pub fn stats(&self) -> PermissionCacheStats {
        PermissionCacheStats::compute(self.grants.len(), self.hits, self.misses)
    }

    /// Check if key is denied by policy (not execution failure)
    #[inline]
    pub fn is_denied<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        matches!(self.grants.get(key), Some(PermissionGrant::Denied))
    }

    /// Check if key has a temporary denial (execution failure, not policy)
    #[inline]
    pub fn is_temporarily_denied<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        matches!(self.grants.get(key), Some(PermissionGrant::TemporaryDenial))
    }

    /// Check if we can use cached permission without prompting
    #[inline]
    pub fn can_use_cached<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        matches!(
            self.grants.get(key),
            Some(PermissionGrant::Session | PermissionGrant::Permanent | PermissionGrant::Denied)
        )
    }
}

impl<K: Eq + Hash> Default for PermissionCache<K> {
    fn default() -> Self {
        Self::new()
    }
}

// Type aliases for backwards compatibility
/// Session-scoped permission cache for ACP hosts (Zed, VS Code, etc.)
pub type AcpPermissionCache = PermissionCache<PathBuf>;

/// Tool-level permission cache - caches approvals for tool execution
pub type ToolPermissionCache = PermissionCache<String>;

/// Alias for ToolPermissionCacheStats (same struct)
pub type ToolPermissionCacheStats = PermissionCacheStats;

/// Extension methods for ToolPermissionCache to maintain API compatibility
impl ToolPermissionCache {
    /// Cache a permission grant for a tool (accepts impl Into<String>)
    #[inline]
    pub fn cache_grant_tool(&mut self, tool_name: impl Into<String>, grant: PermissionGrant) {
        self.cache_grant(tool_name.into(), grant);
    }
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

        // use reference instead of cloning PathBuf
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

        cache.cache_grant(path1, PermissionGrant::Session);

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
        cache.cache_grant(allowed_path, PermissionGrant::Session);

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
        let temp_denied_path = test_path("temp_denied.rs");

        cache.cache_grant(once_path.clone(), PermissionGrant::Once);
        cache.cache_grant(session_path.clone(), PermissionGrant::Session);
        cache.cache_grant(denied_path.clone(), PermissionGrant::Denied);
        cache.cache_grant(
            temp_denied_path.clone(),
            PermissionGrant::TemporaryDenial,
        );

        // "Once" and "TemporaryDenial" grants can't be reused
        assert!(!cache.can_use_cached(&once_path));
        assert!(!cache.can_use_cached(&temp_denied_path));

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

    #[test]
    fn test_distinguishes_denied_from_temporary_denial() {
        let mut cache = AcpPermissionCache::new();
        let denied_path = test_path("denied.rs");
        let temp_denied_path = test_path("temp_denied.rs");

        cache.cache_grant(denied_path.clone(), PermissionGrant::Denied);
        cache.cache_grant(
            temp_denied_path.clone(),
            PermissionGrant::TemporaryDenial,
        );

        // Both should be identified correctly
        assert!(cache.is_denied(&denied_path));
        assert!(!cache.is_denied(&temp_denied_path));

        assert!(!cache.is_temporarily_denied(&denied_path));
        assert!(cache.is_temporarily_denied(&temp_denied_path));
    }

    #[test]
    fn test_clear_temporary_denials_preserves_policy_denials() {
        let mut cache = AcpPermissionCache::new();
        let policy_denied = test_path("policy_denied.rs");
        let temp_denied = test_path("temp_denied.rs");
        let allowed = test_path("allowed.rs");

        cache.cache_grant(policy_denied.clone(), PermissionGrant::Denied);
        cache.cache_grant(temp_denied.clone(), PermissionGrant::TemporaryDenial);
        cache.cache_grant(allowed.clone(), PermissionGrant::Session);

        cache.clear_temporary_denials();

        // Policy denials and session grants should remain
        assert!(cache.is_denied(&policy_denied));
        assert_eq!(
            cache.get_permission(&allowed),
            Some(PermissionGrant::Session)
        );

        // Temporary denials should be gone
        assert!(!cache.is_temporarily_denied(&temp_denied));
        assert_eq!(cache.get_permission(&temp_denied), None);
    }
}
