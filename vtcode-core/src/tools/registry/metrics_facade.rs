//! Metrics-related accessors for ToolRegistry.

use super::ToolRegistry;

impl ToolRegistry {
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

    /// Increment tool call counter (should be called by tool executors).
    #[allow(dead_code)]
    pub(crate) fn increment_tool_calls(&self) {
        self.tool_call_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Increment PTY poll counter (called by PTY polling loop).
    #[allow(dead_code)]
    pub(crate) fn increment_pty_polls(&self) {
        self.pty_poll_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
