use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Metrics for a single tool.
#[derive(Debug, Clone, Default)]
pub struct ToolStats {
    pub success_count: u64,
    pub failure_count: u64,
    pub total_count: u64,
    pub consecutive_failures: u64,
    pub avg_latency_ms: f64,
}

/// Tracks health and performance of tools.
pub struct ToolHealthTracker {
    stats: Arc<RwLock<HashMap<String, ToolStats>>>,
    failure_threshold: u64,
}

impl ToolHealthTracker {
    /// Create a new health tracker.
    /// failure_threshold: number of consecutive failures before marking as unhealthy.
    pub fn new(failure_threshold: u64) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            failure_threshold,
        }
    }

    /// Record a tool execution result.
    pub fn record_execution(&self, tool_name: &str, success: bool, latency: Duration) {
        if let Ok(mut stats_map) = self.stats.write() {
            let tool_stats = stats_map.entry(tool_name.to_string()).or_default();
            
            tool_stats.total_count += 1;
            let latency_ms = latency.as_secs_f64() * 1000.0;
            
            // Update rolling average latency (simple cumulative average for now)
            if tool_stats.total_count == 1 {
                tool_stats.avg_latency_ms = latency_ms;
            } else {
                let n = tool_stats.total_count as f64;
                tool_stats.avg_latency_ms = tool_stats.avg_latency_ms * ((n - 1.0) / n) + latency_ms / n;
            }

            if success {
                tool_stats.success_count += 1;
                tool_stats.consecutive_failures = 0;
            } else {
                tool_stats.failure_count += 1;
                tool_stats.consecutive_failures += 1;
            }
        }
    }

    /// Check if a tool is considered healthy.
    pub fn is_healthy(&self, tool_name: &str) -> bool {
        if let Ok(stats_map) = self.stats.read() {
            if let Some(stats) = stats_map.get(tool_name) {
                return stats.consecutive_failures < self.failure_threshold;
            }
        }
        true // Assume healthy if unknown
    }
}

impl Default for ToolHealthTracker {
    fn default() -> Self {
        Self::new(50)
    }
}
