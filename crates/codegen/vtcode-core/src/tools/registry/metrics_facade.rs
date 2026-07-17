//! Metrics-related accessors for ToolRegistry.

use super::ToolRegistry;

impl ToolRegistry {
    /// Return the shared metrics collector for this registry instance.
    pub fn metrics_collector(&self) -> std::sync::Arc<crate::metrics::MetricsCollector> {
        self.metrics.clone()
    }

    /// Get total tool calls made in current session (for observability).
    pub fn tool_call_count(&self) -> u64 {
        self.tool_call_counter
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get total PTY poll iterations (for CPU monitoring).
    pub fn pty_poll_count(&self) -> u64 {
        self.pty_poll_counter
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
