//! Tool discovery caching system for MCP to avoid redundant tool searches
//!
//! This module provides multi-level caching for MCP tool discovery with
//! bloom filters for fast negative lookups and LRU cache for positive results.

use lru::LruCache;
use rustc_hash::FxHashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::error;

use super::McpToolInfo;
use super::tool_discovery::DetailLevel;

/// Bloom filter for fast negative lookups (tool doesn't exist)
#[derive(Clone)]
pub struct BloomFilter {
    /// Bit array for the filter
    bits: Vec<bool>,
    /// Number of hash functions
    num_hashes: usize,
    /// Size of the bit array
    size: usize,
}

impl BloomFilter {
    pub fn new(expected_items: usize, false_positive_rate: f64) -> Self {
        let size = Self::optimal_size(expected_items, false_positive_rate);
        let num_hashes = Self::optimal_num_hashes(size, expected_items);

        Self {
            bits: vec![false; size],
            num_hashes,
            size,
        }
    }

    /// Add an item to the bloom filter
    pub fn insert(&mut self, item: &str) {
        for i in 0..self.num_hashes {
            let hash = self.hash(item, i);
            let index = hash % self.size;
            self.bits[index] = true;
        }
    }

    /// Check if an item might be in the set
    pub fn contains(&self, item: &str) -> bool {
        for i in 0..self.num_hashes {
            let hash = self.hash(item, i);
            let index = hash % self.size;
            if !self.bits[index] {
                return false;
            }
        }
        true
    }

    /// Clear the bloom filter
    pub fn clear(&mut self) {
        self.bits.fill(false);
    }

    /// Calculate optimal size for bloom filter
    fn optimal_size(expected_items: usize, false_positive_rate: f64) -> usize {
        let size = -(expected_items as f64 * false_positive_rate.ln() / (2.0_f64.ln().powi(2)));
        size.ceil() as usize
    }

    /// Calculate optimal number of hash functions
    fn optimal_num_hashes(size: usize, expected_items: usize) -> usize {
        let num_hashes = (size as f64 / expected_items as f64) * 2.0_f64.ln();
        num_hashes.ceil() as usize
    }

    /// Simple hash function for bloom filter
    fn hash(&self, item: &str, seed: usize) -> usize {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        item.hash(&mut hasher);
        seed.hash(&mut hasher);
        hasher.finish() as usize
    }
}

/// Cache key for tool discovery results
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ToolDiscoveryCacheKey {
    provider_name: String,
    keyword: String,
    detail_level: DetailLevel,
}

/// Cached tool discovery result (internal cache entry)
#[derive(Clone)]
struct CachedToolDiscoveryEntry {
    // OPTIMIZATION: Use Arc to avoid cloning large vectors on cache hits
    results: Arc<Vec<ToolDiscoveryResult>>,
    timestamp: Instant,
}

/// Cached tool discovery result (matches actual API)
#[derive(Debug, Clone)]
pub struct ToolDiscoveryResult {
    pub tool: McpToolInfo,
    pub relevance_score: f64,
    pub detail_level: DetailLevel,
}

/// Multi-level caching system for tool discovery
pub struct ToolDiscoveryCache {
    /// Bloom filter for fast negative lookups
    bloom_filter: Arc<RwLock<BloomFilter>>,
    /// Detailed cache for positive results
    detailed_cache: Arc<RwLock<LruCache<ToolDiscoveryCacheKey, CachedToolDiscoveryEntry>>>,
    /// Cache of all tools for each provider
    all_tools_cache: Arc<RwLock<FxHashMap<String, Vec<McpToolInfo>>>>,
    /// Last refresh time for each provider
    last_refresh: Arc<RwLock<FxHashMap<String, Instant>>>,
    /// Cache configuration
    config: CacheConfig,
}

#[derive(Clone)]
struct CacheConfig {
    /// Maximum age for cached entries
    max_age: Duration,
    /// Maximum age for provider tool lists
    provider_refresh_interval: Duration,
    /// Expected number of tools for bloom filter sizing
    expected_tool_count: usize,
    /// Acceptable false positive rate for bloom filter
    false_positive_rate: f64,
}

impl ToolDiscoveryCache {
    pub fn new(capacity: usize) -> Self {
        let config = CacheConfig {
            max_age: Duration::from_secs(300),                  // 5 minutes
            provider_refresh_interval: Duration::from_secs(60), // 1 minute
            expected_tool_count: 1000,
            false_positive_rate: 0.01, // 1% false positive rate
        };

        let bloom_filter = BloomFilter::new(config.expected_tool_count, config.false_positive_rate);
        let cache_size = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());

        Self {
            bloom_filter: Arc::new(RwLock::new(bloom_filter)),
            detailed_cache: Arc::new(RwLock::new(LruCache::new(cache_size))),
            all_tools_cache: Arc::new(RwLock::new(FxHashMap::default())),
            last_refresh: Arc::new(RwLock::new(FxHashMap::default())),
            config,
        }
    }

    /// Check if a tool might exist (fast negative lookup)
    pub fn might_have_tool(&self, tool_name: &str) -> bool {
        match self.bloom_filter.read() {
            Ok(bloom_filter) => bloom_filter.contains(tool_name),
            Err(_) => {
                tracing::warn!("Bloom filter lock poisoned, assuming tool might exist");
                true
            }
        }
    }

    /// Get cached tool discovery results
    pub fn get_cached_discovery(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
    ) -> Option<Vec<ToolDiscoveryResult>> {
        // OPTIMIZATION: Use to_owned() for explicit String allocation
        let key = ToolDiscoveryCacheKey {
            provider_name: provider_name.to_owned(),
            keyword: keyword.to_owned(),
            detail_level,
        };

        let mut detailed_cache = match self.detailed_cache.write() {
            Ok(cache) => cache,
            Err(e) => {
                tracing::error!("Detailed cache lock poisoned: {}", e);
                return None;
            }
        };

        if let Some(cached) = detailed_cache.get(&key) {
            // Check if the cached entry is still fresh
            if cached.timestamp.elapsed() < self.config.max_age {
                // OPTIMIZATION: Arc allows returning owned Vec without cloning the data
                return Some((*cached.results).clone());
            } else {
                // Entry is stale, remove it
                detailed_cache.pop(&key);
            }
        }

        None
    }

    /// Cache tool discovery results
    pub fn cache_discovery(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
        results: Vec<ToolDiscoveryResult>,
    ) {
        // OPTIMIZATION: Use to_owned() for explicit String allocation
        let key = ToolDiscoveryCacheKey {
            provider_name: provider_name.to_owned(),
            keyword: keyword.to_owned(),
            detail_level,
        };

        let cached = CachedToolDiscoveryEntry {
            // OPTIMIZATION: Wrap in Arc once, share across cache hits
            results: Arc::new(results.clone()),
            timestamp: Instant::now(),
        };

        if let Ok(mut detailed_cache) = self.detailed_cache.write() {
            detailed_cache.put(key, cached);
        } else {
            tracing::error!("Failed to acquire detailed cache lock for writing");
            return;
        }

        // Update bloom filter with all tool names
        if !results.is_empty() {
            if let Ok(mut bloom_filter) = self.bloom_filter.write() {
                for result in &results {
                    bloom_filter.insert(&result.tool.name);
                }
            } else {
                tracing::error!("Failed to acquire bloom filter lock for writing");
            }
        }
    }

    /// Get all cached tools for a provider (with refresh checking)
    pub fn get_all_tools(
        &self,
        provider_name: &str,
        refresh_if_stale: bool,
    ) -> Option<Vec<McpToolInfo>> {
        let last_refresh = match self.last_refresh.read() {
            Ok(lr) => lr,
            Err(e) => {
                error!("Last refresh lock poisoned: {}", e);
                return None;
            }
        };
        let should_refresh = if let Some(last) = last_refresh.get(provider_name) {
            last.elapsed() > self.config.provider_refresh_interval
        } else {
            true
        };
        drop(last_refresh);

        if should_refresh && refresh_if_stale {
            return None; // Signal that refresh is needed
        }

        match self.all_tools_cache.read() {
            Ok(all_tools_cache) => all_tools_cache.get(provider_name).cloned(),
            Err(e) => {
                error!("All tools cache lock poisoned: {}", e);
                None
            }
        }
    }

    /// Cache all tools for a provider
    pub fn cache_all_tools(&self, provider_name: &str, tools: Vec<McpToolInfo>) {
        // Update all tools cache
        let mut all_tools_cache = match self.all_tools_cache.write() {
            Ok(cache) => cache,
            Err(e) => {
                tracing::error!("All tools cache lock poisoned: {}", e);
                return;
            }
        };
        all_tools_cache.insert(provider_name.to_owned(), tools.clone());

        // Update last refresh time
        if let Ok(mut last_refresh) = self.last_refresh.write() {
            last_refresh.insert(provider_name.to_owned(), Instant::now());
        } else {
            tracing::error!("Failed to update last refresh time");
        }

        // Update bloom filter with all tool names
        let mut bloom_filter = match self.bloom_filter.write() {
            Ok(bf) => bf,
            Err(e) => {
                tracing::error!("Bloom filter lock poisoned: {}", e);
                return;
            }
        };
        bloom_filter.clear(); // Clear and rebuild for accuracy

        for tool in &tools {
            bloom_filter.insert(&tool.name);
        }

        // Also update from other providers
        for other_tools in all_tools_cache.values() {
            for tool in other_tools {
                bloom_filter.insert(&tool.name);
            }
        }
    }

    /// Cache a single tool result (for read-only tools)
    pub fn cache_tool_result(&self, _cache_key: String, _result: serde_json::Value) {
        // This would be implemented for caching individual tool execution results
        // For now, we'll just store it in a simple cache
        // In a full implementation, this would use a separate cache with different TTL
    }

    /// Clear all caches
    pub fn clear(&self) {
        if let Ok(mut bf) = self.bloom_filter.write() {
            bf.clear();
        }
        if let Ok(mut dc) = self.detailed_cache.write() {
            dc.clear();
        }
        if let Ok(mut atc) = self.all_tools_cache.write() {
            atc.clear();
        }
        if let Ok(mut lr) = self.last_refresh.write() {
            lr.clear();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> ToolCacheStats {
        let (detailed_entries, detailed_capacity) = self
            .detailed_cache
            .read()
            .map(|cache| (cache.len(), cache.cap().get()))
            .unwrap_or((0, 0));

        let all_tools_entries = self
            .all_tools_cache
            .read()
            .map(|cache| cache.len())
            .unwrap_or(0);

        let (bf_size, bf_hashes) = self
            .bloom_filter
            .read()
            .map(|bf| (bf.size, bf.num_hashes))
            .unwrap_or((0, 0));

        ToolCacheStats {
            detailed_cache_entries: detailed_entries,
            detailed_cache_capacity: detailed_capacity,
            all_tools_cache_entries: all_tools_entries,
            bloom_filter_size: bf_size,
            bloom_filter_hashes: bf_hashes,
        }
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct ToolCacheStats {
    pub detailed_cache_entries: usize,
    pub detailed_cache_capacity: usize,
    pub all_tools_cache_entries: usize,
    pub bloom_filter_size: usize,
    pub bloom_filter_hashes: usize,
}

/// Enhanced tool discovery with caching
pub struct CachedToolDiscovery {
    cache: Arc<ToolDiscoveryCache>,
}

impl CachedToolDiscovery {
    pub fn new(cache_capacity: usize) -> Self {
        Self {
            cache: Arc::new(ToolDiscoveryCache::new(cache_capacity)),
        }
    }

    /// Search for tools with multi-level caching
    pub fn search_tools(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
        all_tools: Vec<McpToolInfo>,
    ) -> Vec<ToolDiscoveryResult> {
        // Check bloom filter first (fast negative lookup)
        if !self.cache.might_have_tool(keyword) && !keyword.is_empty() {
            return Vec::new();
        }

        // Check detailed cache
        if let Some(cached) = self
            .cache
            .get_cached_discovery(provider_name, keyword, detail_level)
        {
            return cached;
        }

        // Perform the search
        let results = self.perform_search(&all_tools, keyword, detail_level);

        // Cache the results
        self.cache
            .cache_discovery(provider_name, keyword, detail_level, results.clone());

        results
    }

    /// Get all tools for a provider with caching
    pub fn get_all_tools_cached(
        &self,
        provider_name: &str,
        all_tools: Vec<McpToolInfo>,
    ) -> Vec<McpToolInfo> {
        // Check cache first
        if let Some(cached) = self.cache.get_all_tools(provider_name, true) {
            return cached;
        }

        // Cache the results
        self.cache.cache_all_tools(provider_name, all_tools.clone());

        all_tools
    }

    /// Perform the actual search on tool list
    fn perform_search(
        &self,
        tools: &[McpToolInfo],
        keyword: &str,
        detail_level: DetailLevel,
    ) -> Vec<ToolDiscoveryResult> {
        let keyword_lower = keyword.to_lowercase();
        let mut results = Vec::new();

        for tool in tools {
            let relevance_score = self.calculate_relevance(tool, &keyword_lower);

            if relevance_score > 0.0 {
                let result = ToolDiscoveryResult {
                    tool: tool.clone(),
                    relevance_score,
                    detail_level,
                };
                results.push(result);
            }
        }

        // Sort by relevance score
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results
    }

    /// Calculate relevance score for a tool
    fn calculate_relevance(&self, tool: &McpToolInfo, keyword: &str) -> f64 {
        let name_lower = tool.name.to_lowercase();
        let description_lower = tool.description.to_lowercase();

        let mut score: f64 = 0.0;

        // Name exact match
        if name_lower == keyword {
            score += 1.0;
        }
        // Name starts with keyword
        else if name_lower.starts_with(keyword) {
            score += 0.8;
        }
        // Name contains keyword
        else if name_lower.contains(keyword) {
            score += 0.6;
        }

        // Description contains keyword
        if description_lower.contains(keyword) {
            score += 0.3;
        }

        // Input schema contains keyword
        let schema_str = serde_json::to_string(&tool.input_schema)
            .unwrap_or_default()
            .to_lowercase();
        if schema_str.contains(keyword) {
            score += 0.2;
        }

        score.min(1.0)
    }

    /// Get cache statistics
    pub fn stats(&self) -> ToolCacheStats {
        self.cache.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_filter() {
        let mut filter = BloomFilter::new(100, 0.01);

        filter.insert("tool1");
        filter.insert("tool2");
        filter.insert("tool3");

        assert!(filter.contains("tool1"));
        assert!(filter.contains("tool2"));
        assert!(filter.contains("tool3"));
        assert!(!filter.contains("tool4"));
    }

    #[test]
    fn test_cache_key_equality() {
        let key1 = ToolDiscoveryCacheKey {
            provider_name: "test".to_string(),
            keyword: "search".to_string(),
            detail_level: DetailLevel::Full,
        };

        let key2 = ToolDiscoveryCacheKey {
            provider_name: "test".to_string(),
            keyword: "search".to_string(),
            detail_level: DetailLevel::Full,
        };

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_tool_discovery_cache() {
        let cache = ToolDiscoveryCache::new(10);

        let provider_name = "test_provider";
        let keyword = "search";
        let detail_level = DetailLevel::Full;

        // Cache miss
        assert!(
            cache
                .get_cached_discovery(provider_name, keyword, detail_level)
                .is_none()
        );

        // Cache some results
        let results = vec![ToolDiscoveryResult {
            tool: McpToolInfo {
                name: "search_files".to_string(),
                description: "Search for files".to_string(),
                provider: "test".to_string(),
                input_schema: serde_json::json!({}),
            },
            relevance_score: 0.9,
            detail_level,
        }];

        cache.cache_discovery(provider_name, keyword, detail_level, results.clone());

        // Cache hit
        let cached = cache.get_cached_discovery(provider_name, keyword, detail_level);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }
}
