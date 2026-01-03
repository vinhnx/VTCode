//! Memory pool for reducing allocations in hot paths

use parking_lot::Mutex;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use vtcode_config::MemoryPoolConfig;

/// Memory pool statistics for auto-tuning
#[derive(Debug, Clone, Default)]
pub struct MemoryPoolStats {
    pub string_hits: usize,
    pub string_misses: usize,
    pub value_hits: usize,
    pub value_misses: usize,
    pub vec_hits: usize,
    pub vec_misses: usize,
    pub allocations_avoided: usize,
}

/// Pre-allocated memory pools for common data structures
pub struct MemoryPool {
    string_pool: Mutex<VecDeque<String>>,
    value_pool: Mutex<VecDeque<Value>>,
    vec_pool: Mutex<VecDeque<Vec<String>>>,
    stats: Mutex<MemoryPoolStats>,
}

impl MemoryPool {
    pub fn new() -> Self {
        Self {
            string_pool: Mutex::new(VecDeque::with_capacity(64)),
            value_pool: Mutex::new(VecDeque::with_capacity(32)),
            vec_pool: Mutex::new(VecDeque::with_capacity(16)),
            stats: Mutex::new(MemoryPoolStats::default()),
        }
    }

    /// Create a new memory pool with custom sizes
    pub fn with_capacities(string_capacity: usize, value_capacity: usize, vec_capacity: usize) -> Self {
        Self {
            string_pool: Mutex::new(VecDeque::with_capacity(string_capacity)),
            value_pool: Mutex::new(VecDeque::with_capacity(value_capacity)),
            vec_pool: Mutex::new(VecDeque::with_capacity(vec_capacity)),
            stats: Mutex::new(MemoryPoolStats::default()),
        }
    }

    /// Create a memory pool from configuration
    pub fn from_config(config: &MemoryPoolConfig) -> Self {
        Self {
            string_pool: Mutex::new(VecDeque::with_capacity(
                config.max_string_pool_size
            )),
            value_pool: Mutex::new(VecDeque::with_capacity(
                config.max_value_pool_size
            )),
            vec_pool: Mutex::new(VecDeque::with_capacity(
                config.max_vec_pool_size
            )),
            stats: Mutex::new(MemoryPoolStats::default()),
        }
    }

    /// Get a reusable string, clearing it first
    pub fn get_string(&self) -> String {
        let mut stats = self.stats.lock();
        let result = self.string_pool.lock().pop_front();
        if let Some(mut s) = result {
            stats.string_hits += 1;
            stats.allocations_avoided += 1;
            s.clear();
            s
        } else {
            stats.string_misses += 1;
            String::new()
        }
    }

    /// Return a string to the pool after clearing it
    pub fn return_string(&self, mut s: String) {
        s.clear();
        let mut pool = self.string_pool.lock();
        // Use capacity as the limit to respect configuration
        if pool.len() < pool.capacity() {
            pool.push_back(s);
        }
    }

    /// Get a reusable Value
    pub fn get_value(&self) -> Value {
        let mut stats = self.stats.lock();
        let result = self.value_pool.lock().pop_front();
        if let Some(v) = result {
            stats.value_hits += 1;
            stats.allocations_avoided += 1;
            v
        } else {
            stats.value_misses += 1;
            Value::Null
        }
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
        let mut stats = self.stats.lock();
        let result = self.vec_pool.lock().pop_front();
        if let Some(mut v) = result {
            stats.vec_hits += 1;
            stats.allocations_avoided += 1;
            v.clear();
            v
        } else {
            stats.vec_misses += 1;
            Vec::new()
        }
    }

    /// Return a Vec<String> to the pool after clearing it
    pub fn return_vec(&self, mut v: Vec<String>) {
        v.clear();
        let mut pool = self.vec_pool.lock();
        // Use capacity as the limit to respect configuration
        if pool.len() < pool.capacity() {
            pool.push_back(v);
        }
    }

    /// Get memory pool statistics
    pub fn get_stats(&self) -> MemoryPoolStats {
        self.stats.lock().clone()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        *self.stats.lock() = MemoryPoolStats::default();
    }

    /// Auto-tune pool sizes based on usage patterns
    /// Returns recommendations for configuration adjustments
    pub fn auto_tune(&self, config: &MemoryPoolConfig) -> MemoryPoolTuningRecommendation {
        let stats = self.get_stats();
        
        // Calculate hit rates
        let string_hit_rate = if stats.string_hits + stats.string_misses > 0 {
            stats.string_hits as f64 / (stats.string_hits + stats.string_misses) as f64
        } else {
            0.0
        };

        let value_hit_rate = if stats.value_hits + stats.value_misses > 0 {
            stats.value_hits as f64 / (stats.value_hits + stats.value_misses) as f64
        } else {
            0.0
        };

        let vec_hit_rate = if stats.vec_hits + stats.vec_misses > 0 {
            stats.vec_hits as f64 / (stats.vec_hits + stats.vec_misses) as f64
        } else {
            0.0
        };

        // Calculate current pool utilization
        let string_utilization = self.string_pool.lock().len() as f64 / config.max_string_pool_size as f64;
        let value_utilization = self.value_pool.lock().len() as f64 / config.max_value_pool_size as f64;
        let vec_utilization = self.vec_pool.lock().len() as f64 / config.max_vec_pool_size as f64;

        // Generate tuning recommendations
        MemoryPoolTuningRecommendation {
            string_hit_rate,
            value_hit_rate,
            vec_hit_rate,
            string_utilization,
            value_utilization,
            vec_utilization,
            total_allocations_avoided: stats.allocations_avoided,
            
            // Recommendations based on usage patterns
            string_size_recommendation: calculate_size_recommendation(
                string_hit_rate,
                string_utilization,
                config.max_string_pool_size
            ),
            value_size_recommendation: calculate_size_recommendation(
                value_hit_rate,
                value_utilization,
                config.max_value_pool_size
            ),
            vec_size_recommendation: calculate_size_recommendation(
                vec_hit_rate,
                vec_utilization,
                config.max_vec_pool_size
            ),
        }
    }
}

/// Calculate size recommendation based on hit rate and utilization
fn calculate_size_recommendation(hit_rate: f64, utilization: f64, current_size: usize) -> SizeRecommendation {
    if hit_rate < 0.3 {
        // Low hit rate - pool might be too small or not used effectively
        if utilization > 0.8 {
            SizeRecommendation::Increase(current_size.saturating_mul(2))
        } else {
            SizeRecommendation::Maintain
        }
    } else if hit_rate > 0.7 {
        // High hit rate - pool is working well
        if utilization > 0.9 {
            SizeRecommendation::Increase(current_size.saturating_add(16))
        } else if utilization < 0.5 {
            SizeRecommendation::Decrease(current_size.saturating_sub(8).max(16))
        } else {
            SizeRecommendation::Maintain
        }
    } else {
        // Medium hit rate
        if utilization > 0.85 {
            SizeRecommendation::Increase(current_size.saturating_add(8))
        } else {
            SizeRecommendation::Maintain
        }
    }
}

/// Memory pool tuning recommendation
#[derive(Debug, Clone)]
pub struct MemoryPoolTuningRecommendation {
    pub string_hit_rate: f64,
    pub value_hit_rate: f64,
    pub vec_hit_rate: f64,
    pub string_utilization: f64,
    pub value_utilization: f64,
    pub vec_utilization: f64,
    pub total_allocations_avoided: usize,
    pub string_size_recommendation: SizeRecommendation,
    pub value_size_recommendation: SizeRecommendation,
    pub vec_size_recommendation: SizeRecommendation,
}

/// Size recommendation enum
#[derive(Debug, Clone, Copy)]
pub enum SizeRecommendation {
    Maintain,
    Increase(usize),
    Decrease(usize),
}

impl From<MemoryPoolStats> for crate::telemetry::MemoryPoolTelemetry {
    fn from(stats: MemoryPoolStats) -> Self {
        Self {
            string_hit_rate: if stats.string_hits + stats.string_misses > 0 {
                stats.string_hits as f64 / (stats.string_hits + stats.string_misses) as f64
            } else {
                0.0
            },
            value_hit_rate: if stats.value_hits + stats.value_misses > 0 {
                stats.value_hits as f64 / (stats.value_hits + stats.value_misses) as f64
            } else {
                0.0
            },
            vec_hit_rate: if stats.vec_hits + stats.vec_misses > 0 {
                stats.vec_hits as f64 / (stats.vec_hits + stats.vec_misses) as f64
            } else {
                0.0
            },
            total_allocations_avoided: stats.allocations_avoided,
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
