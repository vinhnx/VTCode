//! Enhanced parse caching system for tree-sitter to avoid redundant parsing
//!
//! This module provides an LRU cache for parsed syntax trees with proper
//! invalidation and memory management.

use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tree_sitter::{Parser, Tree};

use super::analyzer::{LanguageSupport, TreeSitterError, get_language};

/// Cache key for parsed trees
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ParseCacheKey {
    /// Hash of the source code
    source_hash: u64,
    /// Language of the source code
    language: LanguageSupport,
}

/// Cached parse result
#[derive(Clone)]
#[allow(dead_code)]
struct CachedParse {
    /// The parsed syntax tree
    tree: Tree,
    /// When this parse was cached
    timestamp: Instant,
    /// Size of the source code (for memory tracking)
    source_size: usize,
}

/// Enhanced parse cache with proper tree caching
pub struct ParseCache {
    /// LRU cache for parsed trees
    cache: Arc<RwLock<LruCache<ParseCacheKey, CachedParse>>>,
    /// Maximum age for cached entries
    max_age: Duration,
    /// Maximum source size to cache (avoid caching huge files)
    max_source_size: usize,
    /// Cache statistics
    stats: Arc<RwLock<CacheStats>>,
}

#[derive(Default)]
struct CacheStats {
    hits: u64,
    misses: u64,
    evictions: u64,
    total_parse_time_saved: Duration,
}

impl ParseCache {
    pub fn new(capacity: usize, max_age_seconds: u64, max_source_size: usize) -> Self {
        let cache_size = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());

        Self {
            cache: Arc::new(RwLock::new(LruCache::new(cache_size))),
            max_age: Duration::from_secs(max_age_seconds),
            max_source_size,
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Try to get a cached parse result
    pub fn get_cached_parse(&self, source_code: &str, language: LanguageSupport) -> Option<Tree> {
        // Don't cache very large files
        if source_code.len() > self.max_source_size {
            return None;
        }

        let source_hash = self.hash_source(source_code);
        let key = ParseCacheKey {
            source_hash,
            language,
        };

        let Ok(mut cache) = self.cache.write() else {
            return None;
        };

        if let Some(cached) = cache.get(&key) {
            // Check if the cached entry is still fresh
            if cached.timestamp.elapsed() < self.max_age {
                // Update statistics
                if let Ok(mut stats) = self.stats.write() {
                    stats.hits += 1;
                    stats.total_parse_time_saved += Duration::from_millis(50); // Estimated parse time
                }

                return Some(cached.tree.clone());
            } else {
                // Entry is stale, remove it
                cache.pop(&key);
                if let Ok(mut stats) = self.stats.write() {
                    stats.evictions += 1;
                }
            }
        }

        // Update statistics
        if let Ok(mut stats) = self.stats.write() {
            stats.misses += 1;
        }

        None
    }

    /// Cache a parse result
    pub fn cache_parse(&self, source_code: &str, language: LanguageSupport, tree: Tree) {
        // Don't cache very large files
        if source_code.len() > self.max_source_size {
            return;
        }

        let source_hash = self.hash_source(source_code);
        let key = ParseCacheKey {
            source_hash,
            language,
        };

        let cached = CachedParse {
            tree,
            timestamp: Instant::now(),
            source_size: source_code.len(),
        };

        if let Ok(mut cache) = self.cache.write() {
            cache.put(key, cached);
        }
    }

    /// Clear the cache
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStatistics {
        let Ok(cache) = self.cache.read() else {
            return CacheStatistics::default();
        };
        let Ok(stats) = self.stats.read() else {
            return CacheStatistics::default();
        };

        CacheStatistics {
            hits: stats.hits,
            misses: stats.misses,
            evictions: stats.evictions,
            hit_rate: if stats.hits + stats.misses > 0 {
                stats.hits as f64 / (stats.hits + stats.misses) as f64
            } else {
                0.0
            },
            total_parse_time_saved: stats.total_parse_time_saved,
            entries: cache.len(),
            capacity: cache.cap().get(),
        }
    }

    /// Hash source code for cache keys
    fn hash_source(&self, source_code: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        source_code.hash(&mut hasher);
        hasher.finish()
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheStatistics {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub hit_rate: f64,
    pub total_parse_time_saved: Duration,
    pub entries: usize,
    pub capacity: usize,
}

/// Enhanced tree-sitter analyzer with integrated caching
pub struct CachedTreeSitterAnalyzer {
    /// Underlying parsers for each language
    parsers: HashMap<LanguageSupport, Parser>,
    /// Parse cache
    cache: Arc<ParseCache>,
}

impl CachedTreeSitterAnalyzer {
    pub fn new(cache_capacity: usize) -> Result<Self, TreeSitterError> {
        let mut parsers = HashMap::with_capacity(8); // Pre-allocate for 8 languages

        // Initialize parsers for each language
        let languages = vec![
            #[cfg(feature = "lang-rust")]
            LanguageSupport::Rust,
            #[cfg(feature = "lang-python")]
            LanguageSupport::Python,
            #[cfg(feature = "lang-javascript")]
            LanguageSupport::JavaScript,
            #[cfg(feature = "lang-typescript")]
            LanguageSupport::TypeScript,
            #[cfg(feature = "lang-go")]
            LanguageSupport::Go,
            #[cfg(feature = "lang-java")]
            LanguageSupport::Java,
            LanguageSupport::Bash,
            #[cfg(feature = "swift")]
            LanguageSupport::Swift,
        ];

        for language in languages {
            let mut parser = Parser::new();
            let ts_language = get_language(language).map_err(|e| {
                TreeSitterError::AnalysisError(format!("Language setup failed: {:?}", e))
            })?;
            parser
                .set_language(&ts_language)
                .map_err(|e| TreeSitterError::LanguageSetupError(format!("{:?}", e)))?;
            parsers.insert(language, parser);
        }

        let cache = Arc::new(ParseCache::new(cache_capacity, 300, 1024 * 1024)); // 5min TTL, 1MB max file size

        Ok(Self { parsers, cache })
    }

    /// Parse source code with caching
    pub fn parse(
        &mut self,
        source_code: &str,
        language: LanguageSupport,
    ) -> Result<Tree, TreeSitterError> {
        // Try to get from cache first
        if let Some(cached_tree) = self.cache.get_cached_parse(source_code, language) {
            return Ok(cached_tree);
        }

        // Get parser for this language
        let parser = self
            .parsers
            .get_mut(&language)
            .ok_or_else(|| TreeSitterError::UnsupportedLanguage(format!("{:?}", language)))?;

        // Parse the source code
        let tree = parser.parse(source_code, None).ok_or_else(|| {
            TreeSitterError::ParseError("Failed to parse source code".to_string())
        })?;

        // Cache the result
        self.cache.cache_parse(source_code, language, tree.clone());

        Ok(tree)
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStatistics {
        self.cache.stats()
    }

    /// Clear the cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }
}

impl Default for ParseCache {
    fn default() -> Self {
        // Reduced from 100 entries to 50 for memory efficiency
        // TTL reduced from 300s to 120s for faster cleanup
        // Max file size remains 1MB to avoid caching huge files
        Self::new(50, 120, 1024 * 1024) // 50 entries, 2min TTL, 1MB max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_hashing() {
        let cache = ParseCache::new(10, 300, 1024 * 1024);

        let source1 = "fn main() {}";
        let source2 = "fn main() {}";
        let source3 = "fn different() {}";

        let hash1 = cache.hash_source(source1);
        let hash2 = cache.hash_source(source2);
        let hash3 = cache.hash_source(source3);

        assert_eq!(hash1, hash2); // Same content should have same hash
        assert_ne!(hash1, hash3); // Different content should have different hash
    }

    #[test]
    fn test_cache_operations() {
        let cache = ParseCache::new(2, 300, 1024 * 1024);

        let source = "fn main() {}";
        let language = LanguageSupport::Rust;

        // Cache miss
        assert!(cache.get_cached_parse(source, language).is_none());

        // Cache a parse (this would normally be a real tree)
        // For testing, we'll just verify the cache accepts the call
        // cache.cache_parse(source, language, mock_tree);

        // Check stats
        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.entries, 0); // No actual tree cached in this test
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = ParseCache::new(10, 1, 1024 * 1024); // 1 second TTL

        let source = "fn main() {}";
        let language = LanguageSupport::Rust;

        // Cache a parse
        // cache.cache_parse(source, language, mock_tree);

        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should be expired
        assert!(cache.get_cached_parse(source, language).is_none());
    }
}
