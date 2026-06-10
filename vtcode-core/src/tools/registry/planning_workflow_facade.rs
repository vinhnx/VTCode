//! Planning workflow state accessors for ToolRegistry.

use crate::tools::handlers::PlanningWorkflowState;

use super::ToolRegistry;

impl ToolRegistry {
    /// Enable planning workflow (read-only enforcement).
    ///
    /// When enabled, mutating tools (`unified_file` writes/edits, `apply_patch`,
    /// `unified_exec` runs, etc.)
    /// are blocked and the agent can only read/analyze the codebase.
    ///
    /// `PlanningWorkflowState` is the single source of truth; this method delegates to it
    /// so that `is_planning_active()` and `planning_workflow_state().is_active()` are always in
    /// agreement.
    pub fn enable_planning(&self) {
        let was_active = self.planning_workflow_state.is_active();
        self.planning_workflow_state.enable();
        if !was_active {
            // Invalidate the tool catalog cache so the next snapshot reflects the
            // planning-workflow-filtered tool set rather than serving a stale pre-transition entry.
            self.tool_catalog_state
                .note_explicit_refresh("planning_workflow_enabled");
        }
    }

    /// Disable planning workflow (allow mutating tools again).
    pub fn disable_planning(&self) {
        let was_active = self.planning_workflow_state.is_active();
        self.planning_workflow_state.disable();
        if was_active {
            // Invalidate the catalog cache so mutating tools reappear immediately.
            self.tool_catalog_state
                .note_explicit_refresh("planning_workflow_disabled");
        }
    }

    /// Check if planning workflow is currently enabled.
    ///
    /// Reads directly from `PlanningWorkflowState` — the single authoritative flag.
    #[inline]
    pub fn is_planning_active(&self) -> bool {
        self.planning_workflow_state.is_active()
    }

    /// Get the shared Planning workflow state (used by planning workflow tools and pipeline transitions).
    #[inline]
    pub fn planning_workflow_state(&self) -> PlanningWorkflowState {
        self.planning_workflow_state.clone()
    }
}
