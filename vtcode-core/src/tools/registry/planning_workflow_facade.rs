//! Planning workflow state accessors for ToolRegistry.

use crate::tools::handlers::{PlanLifecyclePhase, PlanningWorkflowState};

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

    /// Set the planning lifecycle phase and invalidate the tool catalog cache.
    ///
    /// Phase transitions can change which tools are available to the agent, so
    /// the cached catalog snapshot must be refreshed. Callers that mutate the
    /// phase through `PlanningWorkflowState::set_phase` directly bypass this
    /// invalidation; route phase changes through this method whenever a
    /// `ToolRegistry` is available so a future phase-aware tool filter never
    /// serves a stale snapshot.
    pub fn set_planning_phase(&self, phase: PlanLifecyclePhase) {
        self.planning_workflow_state.set_phase(phase);
        self.tool_catalog_state
            .note_explicit_refresh("planning_workflow_phase");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::handlers::PlanLifecyclePhase;
    use tempfile::TempDir;

    #[tokio::test]
    async fn set_planning_phase_invalidates_tool_catalog() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let before = registry.tool_catalog_state().current_epoch();

        registry.set_planning_phase(PlanLifecyclePhase::ActiveDrafting);

        let after = registry.tool_catalog_state().current_epoch();
        assert!(
            after > before,
            "set_planning_phase must bump the catalog epoch"
        );
        assert_eq!(
            registry.planning_workflow_state().phase(),
            PlanLifecyclePhase::ActiveDrafting
        );
    }

    #[tokio::test]
    async fn enable_planning_short_circuits_when_already_active() {
        // The `was_active` short-circuit must not double-invalidate: enabling
        // planning a second time should leave the catalog epoch unchanged.
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.enable_planning();
        let after_first = registry.tool_catalog_state().current_epoch();

        registry.enable_planning();
        let after_second = registry.tool_catalog_state().current_epoch();

        assert_eq!(
            after_first, after_second,
            "enabling an already-active planning workflow must not bump the epoch"
        );
        assert!(registry.is_planning_active());
    }
}
