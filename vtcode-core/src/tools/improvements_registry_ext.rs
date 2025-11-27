//! Real integration with ToolRegistry
//!
//! Extends ToolRegistry with improvement capabilities:
//! - Tool effectiveness tracking
//! - Intelligent tool selection based on patterns
//! - Result caching and optimization
//! - Observability integration

use crate::tools::{
    improvements_cache::LruCache,
    improvements_errors::ObservabilityContext,
    pattern_engine::{ExecutionEvent, PatternEngine},
};
use std::sync::Arc;
use std::time::Duration;

/// Tool effectiveness metrics
#[derive(Clone, Debug)]
pub struct ToolMetrics {
    pub name: String,
    pub total_calls: usize,
    pub successful_calls: usize,
    pub total_duration_ms: u64,
    pub avg_quality: f32,
}

impl ToolMetrics {
    pub fn success_rate(&self) -> f32 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.successful_calls as f32 / self.total_calls as f32
        }
    }

    pub fn avg_duration_ms(&self) -> u64 {
        if self.total_calls == 0 {
            0
        } else {
            self.total_duration_ms / self.total_calls as u64
        }
    }
}

/// ToolRegistry improvement extension
pub struct ToolRegistryImprovement {
    pattern_engine: Arc<PatternEngine>,
    tool_metrics: Arc<parking_lot::RwLock<std::collections::HashMap<String, ToolMetrics>>>,
    result_cache: Arc<LruCache<String, String>>,
    obs_context: Arc<ObservabilityContext>,
}

impl ToolRegistryImprovement {
    /// Create new registry extension
    pub fn new(obs_context: Arc<ObservabilityContext>) -> Self {
        Self {
            pattern_engine: Arc::new(PatternEngine::new(1000, 20)),
            tool_metrics: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            result_cache: Arc::new(
                LruCache::new(10000, Duration::from_secs(3600))
                    .with_observability(obs_context.clone()),
            ),
            obs_context,
        }
    }

    /// Record tool execution
    pub fn record_execution(
        &self,
        tool_name: String,
        arguments: String,
        success: bool,
        quality_score: f32,
        duration_ms: u64,
    ) {
        // Update metrics
        {
            let mut metrics = self.tool_metrics.write();
            let entry = metrics
                .entry(tool_name.clone())
                .or_insert_with(|| ToolMetrics {
                    name: tool_name.clone(),
                    total_calls: 0,
                    successful_calls: 0,
                    total_duration_ms: 0,
                    avg_quality: 0.0,
                });

            entry.total_calls += 1;
            if success {
                entry.successful_calls += 1;
            }
            entry.total_duration_ms += duration_ms;
            entry.avg_quality = (entry.avg_quality * (entry.total_calls as f32 - 1.0)
                + quality_score)
                / entry.total_calls as f32;
        }

        // Record pattern event
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.pattern_engine.record(ExecutionEvent {
            tool_name: tool_name.clone(),
            arguments,
            success,
            quality_score,
            duration_ms,
            timestamp: now,
        });

        // Log metric
        self.obs_context.metric(
            "tool_effectiveness",
            &format!("{}_success_rate", tool_name),
            {
                let metrics = self.tool_metrics.read();
                metrics
                    .get(&tool_name)
                    .map(|m| m.success_rate())
                    .unwrap_or(0.0)
            },
        );
    }

    /// Get tool metrics
    pub fn get_tool_metrics(&self, tool_name: &str) -> Option<ToolMetrics> {
        self.tool_metrics.read().get(tool_name).cloned()
    }

    /// Get all tool metrics
    pub fn get_all_metrics(&self) -> Vec<ToolMetrics> {
        self.tool_metrics.read().values().cloned().collect()
    }

    /// Get execution summary
    pub fn get_summary(&self) -> crate::tools::pattern_engine::ExecutionSummary {
        self.pattern_engine.summary()
    }

    /// Predict next tool based on patterns
    pub fn predict_next_tool(&self) -> Option<String> {
        self.pattern_engine.predict_next_tool()
    }

    /// Cache result for tool execution
    pub fn cache_result(&self, tool: &str, args: &str, result: &str) {
        let key = format!("{}::{}", tool, args);
        let _ = self.result_cache.put(key, result.to_owned());
    }

    /// Try to get cached result
    pub fn get_cached_result(&self, tool: &str, args: &str) -> Option<String> {
        let key = format!("{}::{}", tool, args);
        self.result_cache.get_owned(&key)
    }

    /// Clear cache
    pub fn clear_cache(&self) {
        self.result_cache.clear();
    }

    /// Get cache stats
    pub fn cache_stats(&self) -> crate::tools::improvements_cache::CacheStats {
        self.result_cache.stats()
    }

    /// Rank tools by effectiveness
    pub fn rank_tools(&self) -> Vec<(String, f32)> {
        let metrics = self.tool_metrics.read();
        let mut tools: Vec<_> = metrics
            .values()
            .map(|m| {
                let score = (m.success_rate() * 0.6)
                    + ((1.0 - (m.avg_duration_ms() as f32 / 5000.0).min(1.0)) * 0.4);
                (m.name.clone(), score)
            })
            .collect();

        tools.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_execution() {
        let obs = Arc::new(ObservabilityContext::noop());
        let ext = ToolRegistryImprovement::new(obs);

        ext.record_execution("grep_file".to_owned(), "pattern".to_owned(), true, 0.8, 100);

        let metrics = ext.get_tool_metrics("grep_file");
        assert!(metrics.is_some());
        assert_eq!(metrics.unwrap().success_rate(), 1.0);
    }

    #[test]
    fn test_cache_result() {
        let obs = Arc::new(ObservabilityContext::noop());
        let ext = ToolRegistryImprovement::new(obs);

        ext.cache_result("grep_file", "pattern", "result");
        assert_eq!(
            ext.get_cached_result("grep_file", "pattern"),
            Some("result".to_owned())
        );
    }

    #[test]
    fn test_rank_tools() {
        let obs = Arc::new(ObservabilityContext::noop());
        let ext = ToolRegistryImprovement::new(obs);

        ext.record_execution("tool1".to_owned(), "arg".to_owned(), true, 0.9, 100);
        ext.record_execution("tool2".to_owned(), "arg".to_owned(), false, 0.3, 50);

        let ranked = ext.rank_tools();
        assert_eq!(ranked[0].0, "tool1"); // Higher success rate
    }
}
