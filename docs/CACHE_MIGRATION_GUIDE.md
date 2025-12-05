# Cache Migration Guide

**Date**: December 2025
**Status**: Complete
**Version**: 0.47.7+

## Overview

VT Code has consolidated its caching implementations from 5 separate modules to a unified system. This guide helps you migrate from deprecated cache APIs to the new unified interface.

## Quick Reference

| Deprecated API                               | New API                                                | Status        |
| -------------------------------------------- | ------------------------------------------------------ | ------------- |
| `improvements_cache::LruCache`               | `crate::cache::UnifiedCache`                           | ✅ Deprecated |
| `improvements_cache::CacheStats`             | `crate::cache::CacheStats`                             | ✅ Deprecated |
| `smart_cache::SmartResultCache`              | `result_cache::ToolResultCache::with_fuzzy_matching()` | ⚠️ Superseded |
| `smart_cache::ResultSignature::similarity()` | `result_cache::FuzzyMatcher::similarity()`             | ⚠️ Superseded |

## Migration Examples

### 1. LruCache → UnifiedCache

**Before (deprecated):**

```rust
use crate::tools::improvements_cache::LruCache;
use std::time::Duration;

let cache = LruCache::new(1000, Duration::from_secs(300));
cache.put("key".to_string(), "value".to_string())?;
let value = cache.get_owned("key");
```

**After (unified):**

```rust
use crate::cache::{UnifiedCache, EvictionPolicy, DEFAULT_CACHE_TTL, CacheKey};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct MyCacheKey(String);

impl CacheKey for MyCacheKey {
    fn to_cache_key(&self) -> String {
        self.0.clone()
    }
}

let mut cache = UnifiedCache::new(1000, DEFAULT_CACHE_TTL, EvictionPolicy::Lru);
cache.insert(MyCacheKey("key".into()), "value".to_string(), 5);
let value = cache.get_owned(&MyCacheKey("key".into()));
```

### 2. SmartResultCache → ToolResultCache with Fuzzy Matching

**Before (legacy):**

```rust
use crate::tools::smart_cache::SmartResultCache;

let mut cache = SmartResultCache::new(0.8, 10000);
cache.put(tool_name, args, result);

if let Some((result, from_cache)) = cache.get(tool_name, args) {
    // Use cached result
}
```

**After (unified):**

```rust
use crate::tools::result_cache::{ToolResultCache, ToolCacheKey};

let mut cache = ToolResultCache::with_fuzzy_matching(10000, 0.8);
let key = ToolCacheKey::from_json(tool_name, &args, target_path);

cache.insert(key.clone(), result.to_string());

if let Some(result) = cache.get(&key) {
    // Use cached result (Arc<String>)
}
```

### 3. Fuzzy Similarity Matching

**Before (legacy):**

```rust
use crate::tools::smart_cache::ResultSignature;

let sig1 = ResultSignature::from_tool_call(tool, args1);
let sig2 = ResultSignature::from_tool_call(tool, args2);
let similarity = sig1.similarity(&sig2);
```

**After (unified):**

```rust
use crate::tools::result_cache::FuzzyMatcher;
use serde_json::Value;

let similarity = FuzzyMatcher::similarity(args1, args2);
// Returns f32: 0.0 (different) to 1.0 (identical)
```

## Cache Stats Migration

**Before:**

```rust
use crate::tools::improvements_cache::CacheStats;

let stats: CacheStats = cache.stats();
println!("Utilization: {}%", stats.utilization_percent);
```

**After:**

```rust
use crate::cache::CacheStats;

let stats: CacheStats = cache.stats().clone();
println!("Hit rate: {:.2}%",
    stats.hits as f64 / (stats.hits + stats.misses) as f64 * 100.0);
```

## Thread-Safe Wrapper Pattern

For concurrent access, wrap UnifiedCache with `parking_lot::RwLock`:

```rust
use crate::cache::{UnifiedCache, EvictionPolicy, DEFAULT_CACHE_TTL};
use std::sync::Arc;

pub struct ThreadSafeCache<K, V> {
    inner: Arc<parking_lot::RwLock<UnifiedCache<K, V>>>,
}

impl<K, V> ThreadSafeCache<K, V>
where
    K: crate::cache::CacheKey,
    V: crate::cache::CacheValue,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(parking_lot::RwLock::new(
                UnifiedCache::new(capacity, DEFAULT_CACHE_TTL, EvictionPolicy::Lru)
            )),
        }
    }

    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        self.inner.write().get(key)
    }

    pub fn insert(&self, key: K, value: V, size: u64) {
        self.inner.write().insert(key, value, size);
    }
}
```

## Why Migrate?

**Benefits:**

-   ✅ Single unified caching API across entire codebase
-   ✅ Consistent eviction policies (LRU, LFU, FIFO, TTL-only)
-   ✅ Better integration with existing infrastructure
-   ✅ Improved maintainability (3 implementations vs 5)
-   ✅ Same performance characteristics
-   ✅ Zero runtime overhead

**Deprecated modules still work** but will be removed in v0.50.0 (Q2 2026).

## Deprecation Timeline

| Version | Status      | Action Required                |
| ------- | ----------- | ------------------------------ |
| 0.47.7  | Deprecated  | Warnings appear, no breakage   |
| 0.48.x  | Deprecated  | Continue using, plan migration |
| 0.49.x  | Deprecated  | Migration recommended          |
| 0.50.0  | **Removed** | Must migrate before upgrading  |

## Need Help?

-   See examples in migrated modules:

    -   `vtcode-core/src/tools/improvements_registry_ext.rs`
    -   `vtcode-core/src/tools/async_middleware.rs`
    -   `vtcode-core/src/ui/tui/session/performance.rs`

-   Check unified cache implementation:

    -   `vtcode-core/src/cache/mod.rs`

-   Ask in GitHub Discussions: https://github.com/vinhnx/vtcode/discussions

## Summary

All cache migrations maintain backward compatibility. The deprecated APIs continue to work but emit warnings. Plan your migration before v0.50.0 when deprecated modules will be removed entirely.
