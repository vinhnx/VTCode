//! Plan mode state accessors for ToolRegistry.

use crate::tools::handlers::PlanModeState;

use super::ToolRegistry;

impl ToolRegistry {
    /// Enable plan mode (read-only enforcement).
    ///
    /// When enabled, mutating tools (write_file, apply_patch, run_pty_cmd, etc.)
    /// are blocked and the agent can only read/analyze the codebase.
    pub fn enable_plan_mode(&self) {
        self.plan_read_only_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Disable plan mode (allow mutating tools again).
    pub fn disable_plan_mode(&self) {
        self.plan_read_only_mode
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Check if plan mode is currently enabled.
    pub fn is_plan_mode(&self) -> bool {
        self.plan_read_only_mode
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get the shared Plan Mode state (used by plan mode tools and pipeline transitions).
    pub fn plan_mode_state(&self) -> PlanModeState {
        self.plan_mode_state.clone()
    }
}
