//! Tool discovery caching system for MCP to avoid redundant tool searches
//!
//! This module provides multi-level caching for MCP tool discovery with
//! bloom filters for fast exact-name negative lookups and an LRU cache for
//! positive results. The bloom filter only screens exact tool names; fuzzy
//! discovery keywords are never screened against it because its definitive
//! "no" does not extend to fuzzy matching.

use lru::LruCache;
use rustc_hash::FxHashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::error;

use super::McpToolInfo;
use super::tool_discovery::DetailLevel;

/// Bloom filter for fast negative lookups (tool doesn't exist)
///
/// Probabilistic set membership: `contains` returning `false` is definitive
/// (the item was never inserted), while `true` means "maybe" (bounded
/// false-positive rate). The definitive-no guarantee only covers exact
/// membership of previously inserted items, so the filter must only screen
/// exact tool names — never fuzzy or transformed queries such as prefixes,
/// substrings, or keyword variants.
#[derive(Clone)]
pub struct BloomFilter {
    /// Packed bit array: bit `i` lives in `bits[i / WORD_BITS]`.
    bits: Vec<u64>,
    /// Number of hash functions
    num_hashes: usize,
    /// Size of the bit array in bits
    size: usize,
}

impl BloomFilter {
    /// Bits per storage word.
    const WORD_BITS: usize = 64;

    fn new(expected_items: usize, false_positive_rate: f64) -> Self {
        let expected_items = expected_items.max(1);
        let size = Self::optimal_size(expected_items, false_positive_rate).max(1);
        let num_hashes = Self::optimal_num_hashes(size, expected_items).max(1);

        Self {
            bits: vec![0; size.div_ceil(Self::WORD_BITS)],
            num_hashes,
            size,
        }
    }

    /// Add an item to the bloom filter
    fn insert(&mut self, item: &str) {
        for index in Self::indices(item, self.num_hashes, self.size) {
            if let Some(word) = self.bits.get_mut(index / Self::WORD_BITS) {
                *word |= 1u64 << (index % Self::WORD_BITS);
            }
        }
    }

    /// Check if an item might be in the set
    fn contains(&self, item: &str) -> bool {
        Self::indices(item, self.num_hashes, self.size).all(|index| {
            self.bits
                .get(index / Self::WORD_BITS)
                .is_some_and(|word| word & (1u64 << (index % Self::WORD_BITS)) != 0)
        })
    }

    /// Derive the bit indices for `item` from a single hash pass
    /// (Kirsch–Mitzenmacher): `index_i = (base + i * step) % size`, with
    /// `step` forced odd so consecutive hash functions never alias onto one
    /// bit sequence. Indices are deterministic in the item, which preserves
    /// the no-false-negatives guarantee; truncating the hash halves to
    /// `usize` is harmless for hash-derived values, and the modulo bounds
    /// every index to the filter size.
    fn indices(item: &str, num_hashes: usize, size: usize) -> impl Iterator<Item = usize> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        item.hash(&mut hasher);
        let hash = hasher.finish();
        let base = (hash >> 32) as usize;
        let step = ((hash & 0xffff_ffff) as usize) | 1;

        (0..num_hashes).map(move |i| base.wrapping_add(i.wrapping_mul(step)) % size)
    }

    /// Clear the bloom filter
    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    /// Calculate optimal size for bloom filter
    #[allow(
        clippy::cast_sign_loss,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "The calculated Bloom filter size is bounded by the target usize range before conversion."
    )]
    fn optimal_size(expected_items: usize, false_positive_rate: f64) -> usize {
        let size = -(expected_items as f64 * false_positive_rate.ln() / (2.0_f64.ln().powi(2)));
        size.max(0.0).ceil() as usize
    }

    /// Calculate optimal number of hash functions
    #[allow(
        clippy::cast_sign_loss,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "The calculated Bloom filter hash count is bounded by the target usize range before conversion."
    )]
    fn optimal_num_hashes(size: usize, expected_items: usize) -> usize {
        let num_hashes = (size as f64 / expected_items as f64) * 2.0_f64.ln();
        num_hashes.max(0.0).ceil() as usize
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

struct DiscoveryCacheInner {
    bloom_filter: BloomFilter,
    detailed_cache: LruCache<ToolDiscoveryCacheKey, CachedToolDiscoveryEntry>,
    all_tools_cache: FxHashMap<String, Vec<McpToolInfo>>,
    last_refresh: FxHashMap<String, Instant>,
}

/// Cached tool discovery result (matches actual API)
#[derive(Debug, Clone)]
pub struct ToolDiscoveryResult {
    tool: McpToolInfo,
    relevance_score: f64,
    detail_level: DetailLevel,
}

/// Multi-level caching system for tool discovery
pub(crate) struct ToolDiscoveryCache {
    inner: Arc<RwLock<DiscoveryCacheInner>>,
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
    pub(crate) fn new(capacity: usize) -> Self {
        let config = CacheConfig {
            max_age: Duration::from_secs(300),                  // 5 minutes
            provider_refresh_interval: Duration::from_secs(60), // 1 minute
            expected_tool_count: 1000,
            false_positive_rate: 0.01, // 1% false positive rate
        };

        let bloom_filter = BloomFilter::new(config.expected_tool_count, config.false_positive_rate);
        let cache_size = NonZeroUsize::new(capacity).or(NonZeroUsize::new(100));

        Self {
            inner: Arc::new(RwLock::new(DiscoveryCacheInner {
                bloom_filter,
                detailed_cache: LruCache::new(cache_size.unwrap_or(NonZeroUsize::MIN)),
                all_tools_cache: FxHashMap::default(),
                last_refresh: FxHashMap::default(),
            })),
            config,
        }
    }

    /// Check if a tool with this exact name might exist (fast negative lookup)
    ///
    /// `false` is definitive: no tool with this exact name was ever inserted,
    /// so the caller can skip the real lookup. `true` may be a false positive
    /// and must fall through to the authoritative check. Only exact tool
    /// names may be screened here — the filter cannot answer fuzzy queries
    /// (prefixes, substrings, keyword variants), and screening those would
    /// produce false negatives.
    pub fn might_have_tool(&self, tool_name: &str) -> bool {
        match self.inner.read() {
            Ok(inner) => inner.bloom_filter.contains(tool_name),
            Err(_) => {
                tracing::warn!("Bloom filter lock poisoned, assuming tool might exist");
                true
            }
        }
    }

    /// Get cached tool discovery results
    fn get_cached_discovery(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
    ) -> Option<Arc<Vec<ToolDiscoveryResult>>> {
        // OPTIMIZATION: Use to_owned() for explicit String allocation
        let key = ToolDiscoveryCacheKey {
            provider_name: provider_name.to_owned(),
            keyword: keyword.to_owned(),
            detail_level,
        };

        let mut inner = match self.inner.write() {
            Ok(inner) => inner,
            Err(e) => {
                tracing::error!("Detailed cache lock poisoned: {}", e);
                return None;
            }
        };

        if let Some(cached) = inner.detailed_cache.get(&key) {
            // Check if the cached entry is still fresh
            if cached.timestamp.elapsed() < self.config.max_age {
                return Some(Arc::clone(&cached.results));
            } else {
                // Entry is stale, remove it
                drop(inner.detailed_cache.pop(&key));
            }
        }

        None
    }

    /// Cache tool discovery results
    fn cache_discovery(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
        results: Vec<ToolDiscoveryResult>,
    ) {
        self.cache_discovery_shared(provider_name, keyword, detail_level, Arc::new(results));
    }

    fn cache_discovery_shared(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
        results: Arc<Vec<ToolDiscoveryResult>>,
    ) {
        // OPTIMIZATION: Use to_owned() for explicit String allocation
        let key = ToolDiscoveryCacheKey {
            provider_name: provider_name.to_owned(),
            keyword: keyword.to_owned(),
            detail_level,
        };

        let cached = CachedToolDiscoveryEntry {
            // OPTIMIZATION: Wrap in Arc once, share across cache hits
            results: Arc::clone(&results),
            timestamp: Instant::now(),
        };

        let Ok(mut inner) = self.inner.write() else {
            tracing::error!("Failed to acquire discovery cache lock for writing");
            return;
        };

        drop(inner.detailed_cache.put(key, cached));

        for result in results.iter() {
            inner.bloom_filter.insert(&result.tool.name);
        }
    }

    /// Get all cached tools for a provider (with refresh checking)
    pub fn get_all_tools(&self, provider_name: &str, refresh_if_stale: bool) -> Option<Vec<McpToolInfo>> {
        let inner = match self.inner.read() {
            Ok(inner) => inner,
            Err(e) => {
                error!("Discovery cache lock poisoned: {}", e);
                return None;
            }
        };

        let should_refresh = if let Some(last) = inner.last_refresh.get(provider_name) {
            last.elapsed() > self.config.provider_refresh_interval
        } else {
            true
        };

        if should_refresh && refresh_if_stale {
            return None; // Signal that refresh is needed
        }

        inner.all_tools_cache.get(provider_name).cloned()
    }

    /// Cache all tools for a provider
    pub fn cache_all_tools(&self, provider_name: &str, tools: Vec<McpToolInfo>) {
        let mut inner = match self.inner.write() {
            Ok(inner) => inner,
            Err(e) => {
                tracing::error!("Discovery cache lock poisoned: {}", e);
                return;
            }
        };

        drop(inner.all_tools_cache.insert(provider_name.to_owned(), tools.clone()));
        let _previous = inner.last_refresh.insert(provider_name.to_owned(), Instant::now());

        // Update bloom filter with all tool names
        inner.bloom_filter.clear(); // Clear and rebuild for accuracy

        let all_tool_names: Vec<String> = inner
            .all_tools_cache
            .values()
            .flat_map(|provider_tools| provider_tools.iter().map(|tool| tool.name.clone()))
            .collect();

        for tool_name in all_tool_names {
            inner.bloom_filter.insert(&tool_name);
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
        if let Ok(mut inner) = self.inner.write() {
            inner.bloom_filter.clear();
            inner.detailed_cache.clear();
            inner.all_tools_cache.clear();
            inner.last_refresh.clear();
        }
    }

    /// Get cache statistics
    pub(crate) fn stats(&self) -> ToolCacheStats {
        let (detailed_entries, detailed_capacity, all_tools_entries, bf_size, bf_hashes) = self
            .inner
            .read()
            .map(|inner| {
                (
                    inner.detailed_cache.len(),
                    inner.detailed_cache.cap().get(),
                    inner.all_tools_cache.len(),
                    inner.bloom_filter.size,
                    inner.bloom_filter.num_hashes,
                )
            })
            .unwrap_or((0, 0, 0, 0, 0));

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
    detailed_cache_entries: usize,
    detailed_cache_capacity: usize,
    all_tools_cache_entries: usize,
    bloom_filter_size: usize,
    bloom_filter_hashes: usize,
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
    ///
    /// The keyword is matched fuzzily (prefix, substring, description, and
    /// similarity scoring in `perform_search`), so the bloom filter
    /// must not screen it: its definitive "no" only holds for exact
    /// membership of inserted tool names, and filtering non-name keywords
    /// here would discard results the fuzzy search would find. Repeated
    /// queries are instead short-circuited by the LRU detailed cache.
    pub fn search_tools(
        &self,
        provider_name: &str,
        keyword: &str,
        detail_level: DetailLevel,
        all_tools: Vec<McpToolInfo>,
    ) -> Arc<Vec<ToolDiscoveryResult>> {
        // Check detailed cache
        if let Some(cached) = self.cache.get_cached_discovery(provider_name, keyword, detail_level) {
            return cached;
        }

        // Perform the search
        let results = Arc::new(self.perform_search(&all_tools, keyword, detail_level));

        // Cache the results
        self.cache
            .cache_discovery_shared(provider_name, keyword, detail_level, Arc::clone(&results));

        results
    }

    /// Get all tools for a provider with caching
    pub fn get_all_tools_cached(&self, provider_name: &str, all_tools: Vec<McpToolInfo>) -> Vec<McpToolInfo> {
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
                let result = ToolDiscoveryResult { tool: tool.clone(), relevance_score, detail_level };
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
        let schema_str = serde_json::to_string(&tool.input_schema).unwrap_or_default().to_lowercase();
        if schema_str.contains(keyword) {
            score += 0.2;
        }

        // Fuzzy fallback using Sørensen-Dice for partial keyword matches
        if score == 0.0 {
            let sd_name = strsim::sorensen_dice(&name_lower, keyword);
            let sd_desc = strsim::sorensen_dice(&description_lower, keyword);
            let max_sd = sd_name.max(sd_desc);
            if max_sd > 0.3 {
                score = max_sd * 0.5;
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
    fn bloom_filter_has_no_false_negatives() {
        let mut filter = BloomFilter::new(1_000, 0.01);
        let inserted: Vec<String> = (0..1_000).map(|i| format!("tool-{i}")).collect();

        for name in &inserted {
            filter.insert(name);
        }

        for name in &inserted {
            assert!(filter.contains(name), "inserted item {name} must never be reported absent");
        }
    }

    #[test]
    fn bloom_filter_clear_resets_membership() {
        let mut filter = BloomFilter::new(1_000, 0.01);

        filter.insert("tool-a");
        filter.clear();

        // Every bit is zero after clear, so even false positives are impossible.
        assert!(!filter.contains("tool-a"));
        assert!(!filter.contains("tool-b"));
    }

    #[test]
    fn bloom_filter_false_positive_rate_stays_near_target() {
        let mut filter = BloomFilter::new(10_000, 0.01);
        for i in 0..10_000 {
            filter.insert(&format!("tool-{i}"));
        }

        let probes = 100_000u64;
        let false_positives = (0..probes).filter(|i| filter.contains(&format!("absent-{i}"))).count();
        let observed = false_positives as f64 / probes as f64;
        assert!(
            observed <= 0.02,
            "observed false-positive rate {observed} exceeded 2x the 1% target ({false_positives}/{probes})"
        );
    }

    #[test]
    fn bloom_filter_sizing_matches_reference_formulas() {
        // Worked example from the bloom-filter reference (samwho.dev/bloom-filters):
        // 1,000,000 items at a 1-in-a-million false-positive rate fit in ~3.59 MB.
        assert_eq!(BloomFilter::optimal_size(1_000_000, 1e-6), 28_755_176);
        assert_eq!(BloomFilter::optimal_num_hashes(28_755_176, 1_000_000), 20);
    }

    #[test]
    fn bloom_filter_packs_bits_into_words() {
        let filter = BloomFilter::new(1_000, 0.01);

        // m = ceil(-n * ln(p) / ln(2)^2) = 9,586 bits, packed into 150 u64
        // words instead of 9,586 one-byte bools.
        assert_eq!(filter.size, 9_586);
        assert_eq!(filter.bits.len(), 150);
        assert_eq!(filter.num_hashes, 7);
    }

    #[test]
    fn search_tools_returns_fuzzy_matches_for_non_name_keywords() {
        let discovery = CachedToolDiscovery::new(10);
        let tools = vec![McpToolInfo {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            provider: "test".to_string(),
            input_schema: serde_json::json!({}),
            output_schema: None,
            ui: None,
            visibility: crate::types::McpToolVisibility::default(),
        }];

        // "read" is not an exact tool name, so a name-membership bloom screen
        // would definitively reject it and discard the prefix match below.
        let results = discovery.search_tools("test_provider", "read", DetailLevel::Full, tools);
        assert_eq!(results.len(), 1);
        assert_eq!(results.first().map(|result| result.tool.name.as_str()), Some("read_file"));
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
        assert!(cache.get_cached_discovery(provider_name, keyword, detail_level).is_none());

        // Cache some results
        let results = vec![ToolDiscoveryResult {
            tool: McpToolInfo {
                name: "search_files".to_string(),
                description: "Search for files".to_string(),
                provider: "test".to_string(),
                input_schema: serde_json::json!({}),
                output_schema: None,
                ui: None,
                visibility: crate::types::McpToolVisibility::default(),
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
