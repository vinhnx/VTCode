//! Metrics-related accessors for ToolRegistry.

use super::ToolRegistry;

impl ToolRegistry {
    /// Compatibility turn boundary for existing registry callers.
    /// Previews are bounded per result, so there is no aggregate allowance
    /// to reset and this boundary never changes output visibility.
    pub fn begin_turn_preview_window(&self) {}

    /// Return the shared metrics collector for this registry instance.
    pub fn metrics_collector(&self) -> std::sync::Arc<crate::metrics::MetricsCollector> {
        self.metrics.clone()
    }

    /// Get total tool calls made in current session (for observability).
    pub fn tool_call_count(&self) -> u64 {
        self.tool_call_counter.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get total PTY poll iterations (for CPU monitoring).
    pub fn pty_poll_count(&self) -> u64 {
        self.pty_poll_counter.load(std::sync::atomic::Ordering::Relaxed)
    }
}
