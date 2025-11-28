//! Smart result caching with deduplication
//!
//! Caches tool results with fuzzy matching to prevent redundant tool calls.

use crate::utils::current_timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::tools::result_metadata::EnhancedToolResult;

/// Signature for caching and deduplication
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ResultSignature {
    pub tool_name: String,
    pub canonical_args: String,
    pub args_hash: u64,
}

impl ResultSignature {
    /// Create signature from tool call
    #[inline]
    pub fn from_tool_call(tool: &str, args: &Value) -> Self {
        let canonical = Self::canonicalize_args(args);
        let hash = Self::hash_args(&canonical);

        Self {
            tool_name: tool.to_string(),
            canonical_args: canonical,
            args_hash: hash,
        }
    }

    /// Normalize JSON for comparison
    fn canonicalize_args(args: &Value) -> String {
        match args {
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();

                let parts: Vec<String> = keys
                    .iter()
                    .map(|k| format!("{}:{:?}", k, map[*k]))
                    .collect();

                format!("{{{}}}", parts.join(","))
            }
            Value::Array(arr) => {
                let parts: Vec<String> = arr.iter().map(|v| format!("{:?}", v)).collect();
                format!("[{}]", parts.join(","))
            }
            v => format!("{:?}", v),
        }
    }

    /// Hash args for fast lookup
    #[inline]
    fn hash_args(canonical: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        hasher.finish()
    }

    /// Calculate similarity with another signature
    pub fn similarity(&self, other: &ResultSignature) -> f32 {
        if self.tool_name != other.tool_name {
            return 0.0;
        }

        // Simple similarity: count matching characters up to length
        let min_len = self.canonical_args.len().min(other.canonical_args.len());
        if min_len == 0 {
            return 0.0;
        }

        let matches = self
            .canonical_args
            .chars()
            .zip(other.canonical_args.chars())
            .filter(|(a, b)| a == b)
            .count();

        matches as f32 / min_len as f32
    }
}

/// Cached result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResult {
    pub signature: String,
    pub result: Arc<EnhancedToolResult>,
    pub cached_at: u64,
    pub hits: usize,
}

impl CachedResult {
    pub fn new(signature: String, result: EnhancedToolResult) -> Self {
        Self {
            signature,
            result: Arc::new(result),
            cached_at: current_timestamp(),
            hits: 0,
        }
    }

    /// Create from an existing Arc result without cloning the value
    pub fn from_arc(signature: String, result: Arc<EnhancedToolResult>) -> Self {
        Self {
            signature,
            result,
            cached_at: current_timestamp(),
            hits: 0,
        }
    }

    /// Check if cache is still fresh (within 5 minutes)
    #[inline]
    pub fn is_fresh(&self) -> bool {
        current_timestamp().saturating_sub(self.cached_at) < 300 // 5 minutes
    }
}

/// Smart result cache with fuzzy matching
pub struct SmartResultCache {
    cache: HashMap<String, CachedResult>,
    reuse_stats: HashMap<String, usize>,
    fuzzy_threshold: f32,
    max_entries: usize,
}

impl SmartResultCache {
    pub fn new(fuzzy_threshold: f32, max_entries: usize) -> Self {
        Self {
            cache: HashMap::new(),
            reuse_stats: HashMap::new(),
            fuzzy_threshold: fuzzy_threshold.clamp(0.0, 1.0),
            max_entries,
        }
    }

    /// Get cached result with exact or fuzzy matching
    pub fn get(&mut self, tool: &str, args: &Value) -> Option<(EnhancedToolResult, bool)> {
        let sig = ResultSignature::from_tool_call(tool, args);

        // Try exact match first
        if let Some(cached) = self.cache.get(&sig.canonical_args).cloned() {
            if cached.is_fresh() {
                self.record_hit(tool);
                let mut result = (*cached.result).clone();
                result.from_cache = true;

                return Some((result, true));
            }
        }

        // Try fuzzy match
        if let Some((_, cached)) = self.find_similar(&sig).map(|(s, c)| (s, c.clone())) {
            if cached.is_fresh() {
                self.record_hit(tool);
                let mut result = (*cached.result).clone();
                result.from_cache = true;

                return Some((result, true));
            }
        }

        None
    }

    /// Get a shared Arc to the cached result to avoid cloning the entire result.
    pub fn get_arc(&mut self, tool: &str, args: &Value) -> Option<(Arc<EnhancedToolResult>, bool)> {
        let sig = ResultSignature::from_tool_call(tool, args);

        // Try exact match first
        if let Some(cached) = self.cache.get(&sig.canonical_args).cloned() {
            if cached.is_fresh() {
                self.record_hit(tool);
                let arc_res = Arc::clone(&cached.result);
                return Some((arc_res, true));
            }
        }

        // Try fuzzy match
        if let Some((_, cached)) = self.find_similar(&sig).map(|(s, c)| (s, c.clone())) {
            if cached.is_fresh() {
                self.record_hit(tool);
                let arc_res = Arc::clone(&cached.result);
                return Some((arc_res, true));
            }
        }

        None
    }

    /// Put result in cache
    pub fn put(&mut self, tool: &str, args: &Value, result: EnhancedToolResult) {
        let sig = ResultSignature::from_tool_call(tool, args);
        let cached = CachedResult::new(sig.canonical_args.clone(), result);

        // Evict if needed
        if self.cache.len() >= self.max_entries {
            self.evict_lru();
        }

        self.cache.insert(sig.canonical_args, cached);
    }

    /// Put an Arc-wrapped result into the cache to avoid cloning where callers already
    /// have an Arc<EnhancedToolResult> reference.
    pub fn put_arc(&mut self, tool: &str, args: &Value, result: Arc<EnhancedToolResult>) {
        let sig = ResultSignature::from_tool_call(tool, args);
        let cached = CachedResult::from_arc(sig.canonical_args.clone(), result);

        // Evict if needed
        if self.cache.len() >= self.max_entries {
            self.evict_lru();
        }

        self.cache.insert(sig.canonical_args, cached);
    }

    /// Find similar cached result
    fn find_similar(&self, sig: &ResultSignature) -> Option<(f32, &CachedResult)> {
        let mut best: Option<(f32, &CachedResult)> = None;

        for cached in self.cache.values() {
            let other_sig =
                ResultSignature::from_tool_call(&cached.result.tool_name, &cached.result.value);

            let similarity = sig.similarity(&other_sig);
            if similarity >= self.fuzzy_threshold {
                if let Some((best_sim, _)) = &best {
                    if similarity > *best_sim {
                        best = Some((similarity, cached));
                    }
                } else {
                    best = Some((similarity, cached));
                }
            }
        }

        best
    }

    /// Evict least recently used entry
    fn evict_lru(&mut self) {
        if let Some(sig) = self
            .cache
            .iter()
            .min_by_key(|(_, cached)| cached.hits)
            .map(|(sig, _)| sig.clone())
        {
            self.cache.remove(&sig);
        }
    }

    /// Record cache hit
    fn record_hit(&mut self, tool: &str) {
        self.reuse_stats
            .entry(tool.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let total_hits: usize = self.reuse_stats.values().sum();

        CacheStats {
            total_entries: self.cache.len(),
            total_hits,
            reuse_stats: self.reuse_stats.clone(),
            max_entries: self.max_entries,
        }
    }

    /// Clear cache
    pub fn clear(&mut self) {
        self.cache.clear();
        self.reuse_stats.clear();
    }

    /// Get cache size in entries
    pub fn size(&self) -> usize {
        self.cache.len()
    }
}

impl Default for SmartResultCache {
    fn default() -> Self {
        Self::new(0.85, 1000)
    }
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_hits: usize,
    pub reuse_stats: HashMap<String, usize>,
    pub max_entries: usize,
}

impl CacheStats {
    /// Calculate cache hit rate
    pub fn hit_rate(&self) -> f32 {
        if self.total_entries == 0 {
            0.0
        } else {
            self.total_hits as f32 / (self.total_entries + self.total_hits) as f32
        }
    }

    /// Get reuse stats by tool
    pub fn by_tool(&self, tool: &str) -> usize {
        self.reuse_stats.get(tool).copied().unwrap_or(0)
    }

    /// Format as human-readable string
    pub fn to_summary(&self) -> String {
        let hit_rate = (self.hit_rate() * 100.0) as u32;
        format!(
            "Cache: {} entries | {} hits ({}% reuse) | Top: {}",
            self.total_entries,
            self.total_hits,
            hit_rate,
            self.reuse_stats
                .iter()
                .max_by_key(|(_, count)| *count)
                .map(|(tool, count)| format!("{}: {}", tool, count))
                .unwrap_or_default()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ResultMetadata;

    #[test]
    fn test_result_signature_creation() {
        let args = Value::Number(42.into());
        let sig = ResultSignature::from_tool_call("grep", &args);

        assert_eq!(sig.tool_name, "grep");
        assert!(!sig.canonical_args.is_empty());
    }

    #[test]
    fn test_result_signature_canonicalize() {
        let obj = serde_json::json!({ "b": 2, "a": 1 });
        let canonical = ResultSignature::canonicalize_args(&obj);

        // Should be sorted by keys
        assert!(canonical.contains("a:") && canonical.contains("b:"));
    }

    #[test]
    fn test_smart_cache_put_get() {
        let mut cache = SmartResultCache::new(0.85, 100);
        let args = Value::String("test".to_string());
        let result = EnhancedToolResult::new(
            Value::String("result".to_string()),
            ResultMetadata::success(0.8, 0.8),
            "grep".to_string(),
        );

        cache.put_arc("grep", &args, Arc::new(result.clone()));

        let cached = cache.get("grep", &args);
        assert!(cached.is_some());
        assert!(cached.unwrap().1); // Was cached
    }

    #[test]
    fn test_cache_stats() {
        let mut cache = SmartResultCache::new(0.85, 100);
        let args = Value::String("test".to_string());
        let result = EnhancedToolResult::new(
            Value::Null,
            ResultMetadata::success(0.8, 0.8),
            "grep".to_string(),
        );

        cache.put_arc("grep", &args, Arc::new(result));
        cache.get("grep", &args);

        let stats = cache.stats();
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.total_hits, 1);
    }

    #[test]
    fn test_cache_lru_eviction() {
        let mut cache = SmartResultCache::new(0.85, 2);

        let result1 = EnhancedToolResult::new(
            Value::Null,
            ResultMetadata::success(0.8, 0.8),
            "grep".to_string(),
        );

        let result2 = EnhancedToolResult::new(
            Value::Null,
            ResultMetadata::success(0.8, 0.8),
            "find".to_string(),
        );

        let result3 = EnhancedToolResult::new(
            Value::Null,
            ResultMetadata::success(0.8, 0.8),
            "shell".to_string(),
        );

        cache.put_arc("grep", &Value::String("a".to_string()), Arc::new(result1));
        cache.put_arc("find", &Value::String("b".to_string()), Arc::new(result2));
        cache.put_arc("shell", &Value::String("c".to_string()), Arc::new(result3));

        // Should have evicted one
        assert!(cache.size() <= 2);
    }

    #[test]
    fn test_put_arc_get_arc() {
        let mut cache = SmartResultCache::new(0.85, 100);
        let args = Value::String("test".to_string());
        let result = EnhancedToolResult::new(
            Value::String("result".to_string()),
            crate::tools::ResultMetadata::success(0.8, 0.8),
            "grep".to_string(),
        );

        let arc_res = Arc::new(result.clone());
        cache.put_arc("grep", &args, Arc::clone(&arc_res));

        let cached = cache.get_arc("grep", &args);
        assert!(cached.is_some());
        assert_eq!(Arc::ptr_eq(&cached.unwrap().0, &arc_res), true);
    }
}
