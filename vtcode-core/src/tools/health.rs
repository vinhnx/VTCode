use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Outcome of a single tool execution.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ExecutionResult {
    success: bool,
    latency_ms: f64,
}

/// Metrics for a single tool.
#[derive(Debug, Clone, Default)]
pub struct ToolStats {
    pub success_count: u64,
    pub failure_count: u64,
    pub total_count: u64,
    pub consecutive_failures: u64,
    pub avg_latency_ms: f64,
    /// Sliding window of recent executions (last N)
    recent_history: VecDeque<ExecutionResult>,
}

/// Tracks health and performance of tools with sliding window.
pub struct ToolHealthTracker {
    stats: Arc<RwLock<HashMap<String, ToolStats>>>,
    failure_threshold: u64,
    window_size: usize,
}

impl ToolHealthTracker {
    /// Create a new health tracker.
    /// failure_threshold: number of consecutive failures before marking as unhealthy.
    pub fn new(failure_threshold: u64) -> Self {
        Self {
            stats: Arc::new(RwLock::new(HashMap::new())),
            failure_threshold,
            window_size: 20, // Track last 20 executions for current health
        }
    }

    /// Set the tracking window size.
    pub fn set_window_size(&mut self, size: usize) {
        self.window_size = size;
    }

    /// Record a tool execution result.
    pub fn record_execution(&self, tool_name: &str, success: bool, latency: Duration) {
        if let Ok(mut stats_map) = self.stats.write() {
            let tool_stats = stats_map.entry(tool_name.to_string()).or_default();
            let latency_ms = latency.as_secs_f64() * 1000.0;

            // Update lifetime stats
            tool_stats.total_count += 1;

            // Weighted average for lifetime latency (simple decay)
            if tool_stats.total_count == 1 {
                tool_stats.avg_latency_ms = latency_ms;
            } else {
                let n = tool_stats.total_count as f64;
                tool_stats.avg_latency_ms =
                    tool_stats.avg_latency_ms * ((n - 1.0) / n) + latency_ms / n;
            }

            if success {
                tool_stats.success_count += 1;
                tool_stats.consecutive_failures = 0;
            } else {
                tool_stats.failure_count += 1;
                tool_stats.consecutive_failures += 1;
            }

            // Update sliding window
            tool_stats.recent_history.push_back(ExecutionResult {
                success,
                latency_ms,
            });
            if tool_stats.recent_history.len() > self.window_size {
                tool_stats.recent_history.pop_front();
            }
        }
    }

    /// Check if a tool is considered healthy.
    pub fn is_healthy(&self, tool_name: &str) -> bool {
        self.check_health(tool_name).0
    }

    /// Check health and returns (is_healthy, reason)
    pub fn check_health(&self, tool_name: &str) -> (bool, Option<String>) {
        if let Ok(stats_map) = self.stats.read() {
            if let Some(stats) = stats_map.get(tool_name) {
                // Criterion 1: Consecutive failures (Circuit Breaker)
                if stats.consecutive_failures >= self.failure_threshold {
                    return (
                        false,
                        Some(format!(
                            "{} consecutive failures",
                            stats.consecutive_failures
                        )),
                    );
                }

                // Criterion 2: Recent error rate (Degredation)
                // Only enforce if we have enough data (at least half window)
                if stats.recent_history.len() >= 5 {
                    let failures = stats.recent_history.iter().filter(|r| !r.success).count();
                    let failure_rate = failures as f64 / stats.recent_history.len() as f64;
                    if failure_rate > 0.6 {
                        return (
                            false,
                            Some(format!(
                                "High recent failure rate: {:.1}%",
                                failure_rate * 100.0
                            )),
                        );
                    }
                }
            }
        }
        (true, None)
    }

    /// Determine if execution should be delegated to this tool based on health metrics.
    /// Returns true if the tool is healthy enough to attempt execution.
    pub fn should_delegate(&self, tool: &str) -> bool {
        self.is_healthy(tool)
    }

    /// Get latency stats (avg, p95 estimate)
    pub fn get_latency_stats(&self, tool: &str) -> Option<(f64, f64)> {
        let map = self.stats.read().ok()?;
        let stats = map.get(tool)?;

        // Simple average
        let avg = stats.avg_latency_ms;

        // Use recent window for "current" latency if available, else lifetime
        if stats.recent_history.is_empty() {
            return Some((avg, avg));
        }

        let mut sorted: Vec<f64> = stats.recent_history.iter().map(|r| r.latency_ms).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let p95_idx = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
        let p95 = sorted.get(p95_idx).copied().unwrap_or(avg);

        Some((avg, p95))
    }
    /// Get snapshot of all tool stats
    pub fn get_all_tool_stats(&self) -> HashMap<String, ToolStats> {
        self.stats
            .read()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

impl Default for ToolHealthTracker {
    fn default() -> Self {
        Self::new(50)
    }
}
