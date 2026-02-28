use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{LazyLock, Mutex};

/// Maximum cache size (increased from 32 to 128 for multi-project workflows)
const MAX_CACHE_SIZE: usize = 128;

/// Task categories for prompt generation
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TaskType {
    System,
    Lightweight,
    Specialized,
}

/// Providers can expose a cache key describing the prompt variant.
pub trait PromptProvider {
    fn cache_key(&self) -> String;
    fn task_type(&self) -> TaskType;
}

/// Simple in-memory prompt cache keyed by provider + task type.
/// Uses LRU eviction to manage memory in multi-project workflows.
pub struct SystemPromptCache {
    entries: Mutex<LruCache<String, String>>,
}

impl Default for SystemPromptCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemPromptCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(LruCache::new(NonZeroUsize::new(MAX_CACHE_SIZE).unwrap())),
        }
    }

    /// Get cached prompt or build it if missing.
    /// Uses LRU eviction when cache is full (no more clear-on-overflow).
    pub fn get_or_insert_with<F>(&self, key: &str, builder: F) -> String
    where
        F: FnOnce() -> String,
    {
        // Check if already cached (LruCache.get() requires mutable access for LRU update)
        {
            let mut store = self.entries.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(value) = store.get(key) {
                return value.clone();
            }
        }

        // Not cached - build value
        let value = builder();

        // Insert into cache
        {
            let mut store = self.entries.lock().unwrap_or_else(|p| p.into_inner());
            store.put(key.to_string(), value.clone());
            // LRU cache automatically evicts least recently used entry when full
        }

        value
    }

    /// Get cached value, returning None on miss (for async callers that want to build outside the lock)
    pub fn get(&self, key: &str) -> Option<String> {
        let mut store = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        store.get(key).cloned()
    }

    /// Insert a value into the cache
    pub fn insert(&self, key: String, value: String) {
        let mut store = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        store.put(key, value);
        // LRU cache automatically evicts least recently used entry when full
    }

    pub fn clear(&self) {
        if let Ok(mut store) = self.entries.lock() {
            store.clear();
        }
    }
}

/// Global prompt cache shared across runs.
pub static PROMPT_CACHE: LazyLock<SystemPromptCache> = LazyLock::new(SystemPromptCache::new);
