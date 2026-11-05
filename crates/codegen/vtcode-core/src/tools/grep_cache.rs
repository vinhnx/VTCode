//! Caching layer for grep file search results to avoid redundant searches
//!
//! This module provides a cache for search results, keyed by search parameters
//! to eliminate duplicate searches for identical patterns and paths.
//!
//! Uses `UnifiedCache` from `crate::cache` for LRU eviction, TTL, and stats.

use super::grep_file::{GrepSearchInput, GrepSearchResult};
use crate::cache::{CacheKey, DEFAULT_CACHE_TTL, EvictionPolicy, UnifiedCache, estimate_json_size};
use std::sync::Arc;

/// Cache key for search results - includes all parameters that affect search results
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SearchCacheKey {
    pattern: String,
    path: String,
    case_sensitive: bool,
    max_results: usize,
    glob_pattern: Option<String>,
    type_pattern: Option<String>,
    max_result_bytes: Option<usize>,
    respect_ignore_files: bool,
    search_hidden: bool,
    search_binary: bool,
    literal: bool,
}

impl CacheKey for SearchCacheKey {
    fn to_cache_key(&self) -> String {
        // Encode every discriminating field so `remove_where` prefix/suffix/
        // contains invalidation buckets entries correctly. The previous key
        // omitted 7 fields, so invalidation could drop or retain the wrong
        // entries for queries that differed only in those fields.
        format!(
            "grep\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{:?}\u{1f}{:?}\u{1f}{:?}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.pattern,
            self.path,
            self.case_sensitive,
            self.max_results,
            self.glob_pattern,
            self.type_pattern,
            self.max_result_bytes,
            self.respect_ignore_files,
            self.search_hidden,
            self.search_binary,
            self.literal,
        )
    }
}

impl From<&GrepSearchInput> for SearchCacheKey {
    fn from(input: &GrepSearchInput) -> Self {
        Self {
            pattern: input.pattern.clone(),
            path: input.path.clone(),
            case_sensitive: input.case_sensitive.unwrap_or(false),
            max_results: input.max_results.unwrap_or(5), // AGENTS.md requires max 5 results
            glob_pattern: input.glob_pattern.clone(),
            type_pattern: input.type_pattern.clone(),
            max_result_bytes: input.max_result_bytes,
            respect_ignore_files: input.respect_ignore_files.unwrap_or(true),
            search_hidden: input.search_hidden.unwrap_or(false),
            search_binary: input.search_binary.unwrap_or(false),
            literal: input.literal.unwrap_or(false),
        }
    }
}

/// Thread-safe cache for search results backed by `UnifiedCache`
pub struct GrepSearchCache {
    cache: UnifiedCache<SearchCacheKey, GrepSearchResult>,
}

impl GrepSearchCache {
    /// Create a new cache with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: UnifiedCache::new(capacity, DEFAULT_CACHE_TTL, EvictionPolicy::Lru),
        }
    }

    /// Get cached result if available
    pub fn get(&self, input: &GrepSearchInput) -> Option<Arc<GrepSearchResult>> {
        let key = SearchCacheKey::from(input);
        self.cache.get(&key)
    }

    /// Cache a search result
    pub fn put(&self, input: &GrepSearchInput, result: GrepSearchResult) {
        let key = SearchCacheKey::from(input);
        let size_bytes = size_of::<GrepSearchResult>() as u64
            + result.query.len() as u64
            + result.matches.iter().map(estimate_json_size).sum::<u64>();
        self.cache.insert(key, result, size_bytes);
    }

    /// Check if this search should be cached (only results with real hits).
    ///
    /// `GrepSearchResult::matches` holds raw ripgrep JSON stream events, so
    /// a no-hit search is still non-empty: it carries the per-file
    /// `begin`/`end` records and the final `summary`. Caching those would
    /// turn every miss into a stale silent negative — the cache has no
    /// file-change invalidation (TTL and LRU eviction only), so matches
    /// created within the TTL window would stay hidden. Only searches with
    /// at least one `match`-type event are cached: a stale positive at least
    /// surfaces old hits the agent can notice when opening the file, while a
    /// stale negative hides new ones with no signal. Repeated no-match
    /// queries therefore re-run ripgrep — the accepted cost until the cache
    /// gains a write-path invalidation hook.
    pub fn should_cache(result: &GrepSearchResult) -> bool {
        result
            .matches
            .iter()
            .any(|entry| entry.get("type").and_then(serde_json::Value::as_str) == Some("match"))
    }

    /// Clear the cache
    pub fn clear(&self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        let stats = self.cache.stats();
        (stats.current_size, stats.max_size)
    }
}

impl Default for GrepSearchCache {
    fn default() -> Self {
        Self::new(100) // Default to 100 entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_input(pattern: &str, path: &str) -> GrepSearchInput {
        GrepSearchInput {
            pattern: pattern.to_string(),
            path: path.to_string(),
            case_sensitive: Some(true),
            literal: None,
            glob_pattern: Some("*.rs".to_string()),
            context_lines: None,
            include_hidden: None,
            max_results: Some(5), // AGENTS.md requires max 5 results
            respect_ignore_files: None,
            max_file_size: None,
            search_hidden: None,
            search_binary: None,
            files_with_matches: None,
            type_pattern: None,
            invert_match: None,
            word_boundaries: None,
            line_number: None,
            column: None,
            only_matching: None,
            trim: None,
            max_result_bytes: None,
            timeout: None,
            extra_ignore_globs: None,
        }
    }

    #[test]
    fn test_cache_key_equality() {
        let input1 = make_test_input("test", "/path");
        let input2 = make_test_input("test", "/path");

        let key1 = SearchCacheKey::from(&input1);
        let key2 = SearchCacheKey::from(&input2);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_operations() {
        let cache = GrepSearchCache::new(10);

        let input = make_test_input("test", "/path");

        let result = GrepSearchResult {
            query: "test".to_string(),
            matches: vec![serde_json::json!({"file": "test.rs", "line": 1})],
            truncated: false,
            total_matches: None,
        };

        // Cache miss
        assert!(cache.get(&input).is_none());

        // Cache result
        cache.put(&input, result.clone());

        // Cache hit
        let cached = cache.get(&input).unwrap();
        assert_eq!(cached.query, result.query);
        assert_eq!(cached.matches.len(), result.matches.len());
        assert_eq!(cached.truncated, result.truncated);
    }

    #[test]
    fn empty_results_are_not_cached_so_stale_negatives_cannot_mask_new_matches() {
        let cache = GrepSearchCache::new(10);
        let input = make_test_input("never-matches", "/path");
        let result = GrepSearchResult {
            query: "never-matches".to_string(),
            matches: vec![],
            truncated: false,
            total_matches: None,
        };

        // The cache has no file-change invalidation (TTL/LRU only), so a
        // cached miss would silently hide matches created inside the TTL
        // window. A bare empty result must fail the caching gate.
        assert!(!GrepSearchCache::should_cache(&result));

        // A no-hit search is NOT empty: the backend keeps ripgrep's
        // per-file begin/end (and summary) stream events in `matches`.
        // Those alone must also fail the gate — this is the shape a real
        // miss actually arrives in.
        let no_hit_events = GrepSearchResult {
            matches: vec![
                serde_json::json!({"type": "begin", "data": {"path": {"text": "/ws/a.rs"}}}),
                serde_json::json!({"type": "end", "data": {"path": {"text": "/ws/a.rs"}}}),
            ],
            ..result.clone()
        };
        assert!(!GrepSearchCache::should_cache(&no_hit_events));

        // A result carrying at least one real `match` event stays cacheable
        // and round-trips: the gate is match presence, not event count.
        let with_matches = GrepSearchResult {
            matches: vec![serde_json::json!({
                "type": "match",
                "data": {"path": {"text": "/ws/a.rs"}, "lines": {"text": "needle\n"}, "line_number": 1}
            })],
            ..result.clone()
        };
        assert!(GrepSearchCache::should_cache(&with_matches));

        cache.put(&input, with_matches);
        let cached = cache.get(&input).expect("result with a real match must be served from cache");
        assert_eq!(cached.matches.len(), 1);
    }
}
