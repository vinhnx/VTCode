use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Task categories for prompt generation
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TaskType {
    System,
    Lightweight,
    Specialized,
    Custom,
}

/// Providers can expose a cache key describing the prompt variant.
pub trait PromptProvider {
    fn cache_key(&self) -> String;
    fn task_type(&self) -> TaskType;
}

/// Simple in-memory prompt cache keyed by provider + task type.
pub struct SystemPromptCache {
    entries: RwLock<HashMap<String, String>>,
}

impl Default for SystemPromptCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemPromptCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Get cached prompt or build it if missing.
    pub fn get_or_insert_with<F>(&self, key: &str, builder: F) -> String
    where
        F: FnOnce() -> String,
    {
        {
            let store = self.entries.read().unwrap_or_else(|p| p.into_inner());
            if let Some(value) = store.get(key) {
                return value.clone();
            }
        }

        let mut store = self.entries.write().unwrap_or_else(|p| p.into_inner());
        let value = store.entry(key.to_string()).or_insert_with(builder).clone();
        if store.len() > 32 {
            store.clear();
            store.insert(key.to_string(), value.clone());
        }
        value
    }

    /// Get cached value, returning None on miss (for async callers that want to build outside the lock)
    pub fn get(&self, key: &str) -> Option<String> {
        let store = self.entries.read().unwrap_or_else(|p| p.into_inner());
        store.get(key).cloned()
    }

    /// Insert a value into the cache
    pub fn insert(&self, key: String, value: String) {
        let mut store = self.entries.write().unwrap_or_else(|p| p.into_inner());
        store.insert(key.clone(), value.clone());
        if store.len() > 32 {
            store.clear();
            store.insert(key, value);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut store) = self.entries.write() {
            store.clear();
        }
    }
}

/// Global prompt cache shared across runs.
pub static PROMPT_CACHE: LazyLock<SystemPromptCache> = LazyLock::new(SystemPromptCache::new);
