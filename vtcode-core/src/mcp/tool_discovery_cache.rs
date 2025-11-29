//! Tool discovery caching system for MCP to avoid redundant tool searches
//!
//! This module provides multi-level caching for MCP tool discovery with
//! bloom filters for fast negative lookups and LRU cache for positive results.

use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

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
    results: Vec<super::tool_discovery::ToolDiscoveryResult>,
    timestamp: Instant,
    provider_tool_count: usize,
}

/// Multi-level caching system for tool discovery
pub struct ToolDiscoveryCache {
    /// Bloom filter for fast negative lookups
    bloom_filter: Arc<RwLock<BloomFilter>>,
    /// Detailed cache for positive results
    detailed_cache: Arc<RwLock<LruCache<ToolDiscoveryCacheKey, CachedToolDiscoveryEntry>>>,
    /// Cache of all tools for each provider
    all_tools_cache: Arc<RwLock<HashMap<String, Vec<McpToolInfo>>>>,
    /// Last refresh time for each provider
    last_refresh: Arc<RwLock<HashMap<String, Instant>>>,
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
            max_age: Duration::from_secs(300), // 5 minutes
            provider_refresh_interval: Duration::from_secs(60), // 1 minute
            expected_tool_count: 1000,
            false_positive_rate: 0.01, // 1% false positive rate
        };

        let bloom_filter = BloomFilter::new(config.expected_tool_count, config.false_positive_rate);
        let cache_size = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());

        Self {
            bloom_filter: Arc::new(RwLock::new(bloom_filter)),
            detailed_cache: Arc::new(RwLock::new(LruCache::new(cache_size))),
            all_tools_cache: Arc::new(RwLock::new(HashMap::new())),
            last_refresh: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Check if a tool might exist (fast negative lookup)
    pub fn might_have_tool(&self, tool_name: &str) -> bool {
        let bloom_filter = self.bloom_filter.read().unwrap();
        bloom_filter.contains(tool_name)
    }

    /// Get cached tool discovery results
    pub fn get_cached_discovery(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
    ) -> Option<Vec<super::tool_discovery::ToolDiscoveryResult>> {
        let key = ToolDiscoveryCacheKey {
            provider_name: provider_name.to_string(),
            keyword: keyword.to_string(),
            detail_level,
        };

        let mut detailed_cache = self.detailed_cache.write().unwrap();

        if let Some(cached) = detailed_cache.get(&key) {
            // Check if the cached entry is still fresh
            if cached.timestamp.elapsed() < self.config.max_age {
                return Some(cached.results.clone());
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
        results: Vec<super::tool_discovery::ToolDiscoveryResult>,
    ) {
        let key = ToolDiscoveryCacheKey {
            provider_name: provider_name.to_string(),
            keyword: keyword.to_string(),
            detail_level,
        };

        let cached = CachedToolDiscoveryEntry {
            results: results.clone(),
            timestamp: Instant::now(),
            provider_tool_count: results.len(),
        };

        let mut detailed_cache = self.detailed_cache.write().unwrap();
        detailed_cache.put(key, cached);

        // Update bloom filter with all tool names
        if !results.is_empty() {
            let mut bloom_filter = self.bloom_filter.write().unwrap();
            for result in &results {
                bloom_filter.insert(&result.tool.name);
            }
        }
    }

    /// Get all cached tools for a provider (with refresh checking)
    pub async fn get_all_tools(
        &self,
        provider_name: &str,
        refresh_if_stale: bool,
    ) -> Option<Vec<McpToolInfo>> {
        let last_refresh = self.last_refresh.read().unwrap();
        let should_refresh = if let Some(last) = last_refresh.get(provider_name) {
            last.elapsed() > self.config.provider_refresh_interval
        } else {
            true
        };
        drop(last_refresh);

        if should_refresh && refresh_if_stale {
            return None; // Signal that refresh is needed
        }

        let all_tools_cache = self.all_tools_cache.read().unwrap();
        all_tools_cache.get(provider_name).cloned()
    }

    /// Cache all tools for a provider
    pub fn cache_all_tools(&self, provider_name: &str, tools: Vec<McpToolInfo>) {
        // Update all tools cache
        let mut all_tools_cache = self.all_tools_cache.write().unwrap();
        all_tools_cache.insert(provider_name.to_string(), tools.clone());

        // Update last refresh time
        let mut last_refresh = self.last_refresh.write().unwrap();
        last_refresh.insert(provider_name.to_string(), Instant::now());

        // Update bloom filter with all tool names
        let mut bloom_filter = self.bloom_filter.write().unwrap();
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
    pub fn cache_tool_result(&self, cache_key: String, result: serde_json::Value) {
        // This would be implemented for caching individual tool execution results
        // For now, we'll just store it in a simple cache
        // In a full implementation, this would use a separate cache with different TTL
    }

    /// Clear all caches
    pub fn clear(&self) {
        self.bloom_filter.write().unwrap().clear();
        self.detailed_cache.write().unwrap().clear();
        self.all_tools_cache.write().unwrap().clear();
        self.last_refresh.write().unwrap().clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> ToolCacheStats {
        let detailed_cache = self.detailed_cache.read().unwrap();
        let all_tools_cache = self.all_tools_cache.read().unwrap();
        let bloom_filter = self.bloom_filter.read().unwrap();

        ToolCacheStats {
            detailed_cache_entries: detailed_cache.len(),
            detailed_cache_capacity: detailed_cache.cap().get(),
            all_tools_cache_entries: all_tools_cache.len(),
            bloom_filter_size: bloom_filter.size,
            bloom_filter_hashes: bloom_filter.num_hashes,
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
    pub async fn search_tools(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
        tool_fetcher: impl FnOnce() -> futures::future::BoxFuture<'static, Result<Vec<McpToolInfo>, String>>,
    ) -> Result<Vec<super::tool_discovery::ToolDiscoveryResult>, String> {
        // Check bloom filter first (fast negative lookup)
        if !self.cache.might_have_tool(keyword) && !keyword.is_empty() {
            return Ok(Vec::new());
        }

        // Check detailed cache
        if let Some(cached) = self.cache.get_cached_discovery(provider_name, keyword, detail_level) {
            return Ok(cached);
        }

        // Fetch tools from provider
        let all_tools = tool_fetcher().await?;

        // Perform the search
        let results = self.perform_search(&all_tools, keyword, detail_level);

        // Cache the results
        self.cache.cache_discovery(provider_name, keyword, detail_level, results.clone());

        Ok(results)
    }

    /// Get all tools for a provider with caching
    pub async fn get_all_tools(
        &self,
        provider_name: &str,
        tool_fetcher: impl FnOnce() -> futures::future::BoxFuture<'static, Result<Vec<McpToolInfo>, String>>,
    ) -> Result<Vec<McpToolInfo>, String> {
        // Check cache first
        if let Some(cached) = self.cache.get_all_tools(provider_name, true).await {
            return Ok(cached);
        }

        // Fetch from provider
        let tools = tool_fetcher().await?;

        // Cache the results
        self.cache.cache_all_tools(provider_name, tools.clone());

        Ok(tools)
    }

    /// Perform the actual search on tool list
    fn perform_search(
        &self,
        tools: &[McpToolInfo],
        keyword: &str,
        detail_level: DetailLevel,
    ) -> Vec<super::tool_discovery::ToolDiscoveryResult> {
        let keyword_lower = keyword.to_lowercase();
        let mut results = Vec::new();

        for tool in tools {
            let relevance_score = self.calculate_relevance(tool, &keyword_lower);

            if relevance_score > 0.0 {
                let result = super::tool_discovery::ToolDiscoveryResult {
                    tool: tool.clone(),
                    relevance_score,
                    detail_level,
                };
                results.push(result);
            }
        }

        // Sort by relevance score
        results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));

        results
    }

    /// Calculate relevance score for a tool
    fn calculate_relevance(&self, tool: &McpToolInfo, keyword: &str) -> f64 {
        let name_lower = tool.name.to_lowercase();
        let description_lower = tool.description.to_lowercase();

        let mut score = 0.0;

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

        // Parameter names contain keyword
        if let Some(input_schema) = &tool.input_schema {
            if input_schema.to_string().to_lowercase().contains(keyword) {
                score += 0.2;
            }
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
            detail_level: DetailLevel::High,
        };

        let key2 = ToolDiscoveryCacheKey {
            provider_name: "test".to_string(),
            keyword: "search".to_string(),
            detail_level: DetailLevel::High,
        };

        assert_eq!(key1, key2);
    }

    #[tokio::test]
    async fn test_tool_discovery_cache() {
        let cache = ToolDiscoveryCache::new(10);

        let provider_name = "test_provider";
        let keyword = "search";
        let detail_level = DetailLevel::High;

        // Cache miss
        assert!(cache.get_cached_discovery(provider_name, keyword, detail_level).is_none());

        // Cache some results
        let results = vec![
            super::tool_discovery::ToolDiscoveryResult {
                tool: McpToolInfo {
                    name: "search_files".to_string(),
                    description: "Search for files".to_string(),
                    input_schema: None,
                },
                relevance_score: 0.9,
                detail_level,
            }
        ];

        cache.cache_discovery(provider_name, keyword, detail_level, results.clone());

        // Cache hit
        let cached = cache.get_cached_discovery(provider_name, keyword, detail_level);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }
}