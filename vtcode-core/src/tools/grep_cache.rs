//! Caching layer for grep file search results to avoid redundant searches
//!
//! This module provides an LRU cache for search results, keyed by search parameters
//! to eliminate duplicate searches for identical patterns and paths.

use super::grep_file::{GrepSearchInput, GrepSearchResult};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// Cache key for search results - includes all parameters that affect search results
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SearchCacheKey {
    pattern: String,
    path: String,
    case_sensitive: bool,
    max_results: usize,
    glob_pattern: Option<String>,
    type_pattern: Option<String>,
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
        }
    }
}

/// Thread-safe LRU cache for search results
pub struct GrepSearchCache {
    cache: Arc<Mutex<LruCache<SearchCacheKey, Arc<GrepSearchResult>>>>,
}

impl GrepSearchCache {
    /// Create a new cache with the specified capacity
    pub fn new(capacity: usize) -> Self {
        let cache_size = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(cache_size))),
        }
    }

    /// Get cached result if available
    pub fn get(&self, input: &GrepSearchInput) -> Option<Arc<GrepSearchResult>> {
        let key = SearchCacheKey::from(input);
        let mut cache = self.cache.lock().unwrap();
        cache.get(&key).cloned()
    }

    /// Cache a search result
    pub fn put(&self, input: &GrepSearchInput, result: GrepSearchResult) {
        let key = SearchCacheKey::from(input);
        let mut cache = self.cache.lock().unwrap();
        cache.put(key, Arc::new(result));
    }

    /// Check if this search should be cached (only cache successful, non-empty results)
    pub fn should_cache(result: &GrepSearchResult) -> bool {
        !result.matches.is_empty()
    }

    /// Clear the cache
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        let cache = self.cache.lock().unwrap();
        (cache.len(), cache.cap().get())
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
        };

        // Cache miss
        assert!(cache.get(&input).is_none());

        // Cache result
        cache.put(&input, result.clone());

        // Cache hit
        let cached = cache.get(&input).unwrap();
        assert_eq!(cached.query, result.query);
        assert_eq!(cached.matches.len(), result.matches.len());
    }
}
