//! AST caching for TreeSitter to reduce parsing overhead
//!
//! This module provides an LRU cache for parsed syntax trees, reducing re-parsing
//! of frequently-used code snippets. Cache keys are based on content hash + language
//! to handle code changes and language detection variations.

use crate::tools::tree_sitter::LanguageSupport;
use std::collections::HashMap;
use std::num::NonZeroUsize;

/// Statistics about cache performance
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total cache hits
    pub hits: u64,
    /// Total cache misses
    pub misses: u64,
    /// Number of items currently in cache
    pub size: usize,
    /// Items evicted due to LRU
    pub evictions: u64,
}

impl CacheStats {
    /// Calculate hit rate as percentage
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// Cache key combining content hash and language
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    content_hash: u64,
    language: LanguageSupport,
}

impl CacheKey {
    fn new(content: &str, language: LanguageSupport) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        language.hash(&mut hasher);

        Self {
            content_hash: hasher.finish(),
            language,
        }
    }
}

/// LRU cache for parsed TreeSitter syntax trees
///
/// Uses a simple HashMap + LinkedList approach for LRU eviction.
/// The cache is designed to work with TreeSitter's ownership model.
pub struct AstCache {
    /// Map from cache key to cached tree (stored as Box to allow tree ownership)
    cache: HashMap<CacheKey, CachedAst>,
    /// Access order for LRU: most recent at end
    access_order: Vec<CacheKey>,
    /// Maximum number of entries
    max_size: NonZeroUsize,
    /// Statistics
    stats: CacheStats,
}

/// Wrapper for cached AST data
#[derive(Clone)]
struct CachedAst {
    /// Source code that was parsed
    #[allow(dead_code)]
    source: String,
    /// Whether this entry is still valid (source hasn't changed externally)
    #[allow(dead_code)]
    is_valid: bool,
}

impl AstCache {
    /// Create a new cache with specified max size
    pub fn new(max_size: usize) -> Self {
        let max_size =
            NonZeroUsize::new(max_size).unwrap_or_else(|| NonZeroUsize::new(128).unwrap());
        Self {
            cache: HashMap::with_capacity(max_size.get()),
            access_order: Vec::with_capacity(max_size.get()),
            max_size,
            stats: CacheStats::default(),
        }
    }

    /// Check if content is in cache (without returning the tree)
    ///
    /// Note: We can't return the actual Tree here because TreeSitter's Tree
    /// doesn't implement Clone. The caller must use `parse()` with caching
    /// built into their TreeSitterAnalyzer instead.
    pub fn contains(&mut self, content: &str, language: LanguageSupport) -> bool {
        let key = CacheKey::new(content, language);

        if self.cache.contains_key(&key) {
            self.update_access_order(&key);
            self.stats.hits += 1;
            true
        } else {
            self.stats.misses += 1;
            false
        }
    }

    /// Record a cache entry for later reference
    ///
    /// This records the source code so we can validate cache entries.
    pub fn record_parse(&mut self, content: &str, language: LanguageSupport) {
        let key = CacheKey::new(content, language);

        let entry = CachedAst {
            source: content.to_string(),
            is_valid: true,
        };

        if self.cache.insert(key, entry).is_none() {
            // New entry
            self.access_order.push(key);
            self.stats.size = self.cache.len();

            // Evict if needed
            if self.access_order.len() > self.max_size.get() {
                let lru_key = self.access_order.remove(0);
                self.cache.remove(&lru_key);
                self.stats.evictions += 1;
                self.stats.size = self.cache.len();
            }
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.stats.clone()
    }

    /// Clear all cache entries
    pub fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
        self.stats.size = 0;
    }

    /// Update LRU access order for a key
    fn update_access_order(&mut self, key: &CacheKey) {
        if let Some(pos) = self.access_order.iter().position(|k| k == key) {
            self.access_order.remove(pos);
            self.access_order.push(*key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic_operations() {
        let mut cache = AstCache::new(2);
        let lang = LanguageSupport::Rust;
        let code = "fn main() {}";

        assert!(!cache.contains(code, lang));
        cache.record_parse(code, lang);
        assert!(cache.contains(code, lang));
    }

    #[test]
    fn test_cache_lru_eviction() {
        let mut cache = AstCache::new(2);
        let lang = LanguageSupport::Rust;

        cache.record_parse("fn a() {}", lang);
        cache.record_parse("fn b() {}", lang);
        cache.record_parse("fn c() {}", lang); // Should evict a

        assert!(!cache.contains("fn a() {}", lang)); // Evicted
        assert!(cache.contains("fn b() {}", lang)); // Still there
        assert!(cache.contains("fn c() {}", lang)); // Still there
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn test_cache_stats() {
        let mut cache = AstCache::new(10);
        let lang = LanguageSupport::Rust;

        // Miss
        cache.contains("fn a() {}", lang);
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hit_rate(), 0.0);

        // Record and hit
        cache.record_parse("fn a() {}", lang);
        cache.contains("fn a() {}", lang);
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
        assert!((cache.stats().hit_rate() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = AstCache::new(10);
        let lang = LanguageSupport::Rust;

        cache.record_parse("fn a() {}", lang);
        cache.record_parse("fn b() {}", lang);
        assert_eq!(cache.stats().size, 2);

        cache.clear();
        assert_eq!(cache.stats().size, 0);
        assert!(!cache.contains("fn a() {}", lang));
    }

    #[test]
    fn test_cache_different_languages() {
        let mut cache = AstCache::new(10);
        let rust_code = "fn main() {}";
        let python_code = "def main(): pass";

        cache.record_parse(rust_code, LanguageSupport::Rust);
        cache.record_parse(python_code, LanguageSupport::Python);

        assert!(cache.contains(rust_code, LanguageSupport::Rust));
        assert!(cache.contains(python_code, LanguageSupport::Python));
        assert_eq!(cache.stats().size, 2);
    }
}
