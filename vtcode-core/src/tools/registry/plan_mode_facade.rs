//! Plan mode state accessors for ToolRegistry.

use crate::tools::handlers::PlanModeState;

use super::ToolRegistry;

impl ToolRegistry {
    /// Enable plan mode (read-only enforcement).
    ///
    /// When enabled, mutating tools (`unified_file` writes/edits, `apply_patch`,
    /// `unified_exec` runs, etc.)
    /// are blocked and the agent can only read/analyze the codebase.
    ///
    /// `PlanModeState` is the single source of truth; this method delegates to it
    /// so that `is_plan_mode()` and `plan_mode_state().is_active()` are always in
    /// agreement.
    pub fn enable_plan_mode(&self) {
        let was_active = self.plan_mode_state.is_active();
        self.plan_mode_state.enable();
        if !was_active {
            // Invalidate the tool catalog cache so the next snapshot reflects the
            // plan-mode-filtered tool set rather than serving a stale pre-transition entry.
            self.tool_catalog_state
                .note_explicit_refresh("plan_mode_enabled");
        }
    }

    /// Disable plan mode (allow mutating tools again).
    pub fn disable_plan_mode(&self) {
        let was_active = self.plan_mode_state.is_active();
        self.plan_mode_state.disable();
        if was_active {
            // Invalidate the catalog cache so mutating tools reappear immediately.
            self.tool_catalog_state
                .note_explicit_refresh("plan_mode_disabled");
        }
    }

    /// Check if plan mode is currently enabled.
    ///
    /// Reads directly from `PlanModeState` — the single authoritative flag.
    #[inline]
    pub fn is_plan_mode(&self) -> bool {
        self.plan_mode_state.is_active()
    }

    /// Get the shared Plan Mode state (used by plan mode tools and pipeline transitions).
    #[inline]
    pub fn plan_mode_state(&self) -> PlanModeState {
        self.plan_mode_state.clone()
    }
}
