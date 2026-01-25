//! Optimization-related accessors for ToolRegistry.

use std::sync::Arc;

use crate::core::memory_pool::MemoryPool;

use super::ToolRegistry;

impl ToolRegistry {
    /// Check if optimizations are enabled.
    pub fn has_optimizations_enabled(&self) -> bool {
        self.optimization_config
            .tool_registry
            .use_optimized_registry
            || self.optimization_config.memory_pool.enabled
    }

    /// Get memory pool for optimized allocations.
    pub fn memory_pool(&self) -> &Arc<MemoryPool> {
        &self.memory_pool
    }

    /// Clear the hot tool cache (useful for testing or memory management).
    pub fn clear_hot_cache(&self) {
        if self
            .optimization_config
            .tool_registry
            .use_optimized_registry
        {
            self.hot_tool_cache.write().clear();
        }
    }

    /// Get hot cache statistics.
    pub fn hot_cache_stats(&self) -> (usize, usize) {
        let cache = self.hot_tool_cache.read();
        (cache.len(), cache.cap().get())
    }

    /// Configure performance optimizations for this registry.
    pub fn configure_optimizations(&mut self, config: vtcode_config::OptimizationConfig) {
        self.optimization_config = config;

        // Resize hot cache if needed
        if self
            .optimization_config
            .tool_registry
            .use_optimized_registry
        {
            let new_size = self.optimization_config.tool_registry.hot_cache_size;
            if let Some(new_cache_size) = std::num::NonZeroUsize::new(new_size) {
                *self.hot_tool_cache.write() = lru::LruCache::new(new_cache_size);
            }
        }
    }

    /// Get the current optimization configuration.
    pub fn optimization_config(&self) -> &vtcode_config::OptimizationConfig {
        &self.optimization_config
    }
}
