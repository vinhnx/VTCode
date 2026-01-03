//! Memory pool for reducing allocations in hot paths

use parking_lot::Mutex;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;

/// Pre-allocated memory pools for common data structures
pub struct MemoryPool {
    string_pool: Mutex<VecDeque<String>>,
    value_pool: Mutex<VecDeque<Value>>,
    vec_pool: Mutex<VecDeque<Vec<String>>>,
}

impl MemoryPool {
    pub fn new() -> Self {
        Self {
            string_pool: Mutex::new(VecDeque::with_capacity(64)),
            value_pool: Mutex::new(VecDeque::with_capacity(32)),
            vec_pool: Mutex::new(VecDeque::with_capacity(16)),
        }
    }

    /// Get a reusable string, clearing it first
    pub fn get_string(&self) -> String {
        self.string_pool
            .lock()
            .pop_front()
            .unwrap_or_else(String::new)
    }

    /// Return a string to the pool after clearing it
    pub fn return_string(&self, mut s: String) {
        s.clear();
        let mut pool = self.string_pool.lock();
        if pool.len() < 64 {
            pool.push_back(s);
        }
    }

    /// Get a reusable Value
    pub fn get_value(&self) -> Value {
        self.value_pool.lock().pop_front().unwrap_or(Value::Null)
    }

    /// Return a Value to the pool
    pub fn return_value(&self, v: Value) {
        let mut pool = self.value_pool.lock();
        if pool.len() < 32 {
            pool.push_back(v);
        }
    }

    /// Get a reusable Vec<String>
    pub fn get_vec(&self) -> Vec<String> {
        self.vec_pool.lock().pop_front().unwrap_or_else(Vec::new)
    }

    /// Return a Vec<String> to the pool after clearing it
    pub fn return_vec(&self, mut v: Vec<String>) {
        v.clear();
        let mut pool = self.vec_pool.lock();
        if pool.len() < 16 {
            pool.push_back(v);
        }
    }
}

impl Default for MemoryPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Global memory pool instance
static MEMORY_POOL: once_cell::sync::Lazy<Arc<MemoryPool>> =
    once_cell::sync::Lazy::new(|| Arc::new(MemoryPool::new()));

/// Get the global memory pool
pub fn global_pool() -> Arc<MemoryPool> {
    Arc::clone(&MEMORY_POOL)
}
