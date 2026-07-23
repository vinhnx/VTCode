//! Planning workflow state accessors for ToolRegistry.

use crate::config::constants::tools as tool_names;
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::PlanningWorkflowState;
use crate::tools::names::canonical_tool_name;
use indexmap::IndexMap;

use super::ToolRegistry;

const PLAN_MODE_EXPLORATION_TOOLS: &[&str] = &[
    tool_names::EXEC_COMMAND,
    tool_names::CODE_SEARCH,
    tool_names::READ_FILE,
    tool_names::LIST_FILES,
    tool_names::GREP_FILE,
    tool_names::TASK_TRACKER,
];

impl ToolRegistry {
    /// Enable planning workflow (read-only enforcement).
    ///
    /// When enabled, mutating tools (`file_operation` writes/edits, `apply_patch`,
    /// `command_session` runs, etc.)
    /// are blocked and the agent can only read/analyze the codebase.
    ///
    /// `PlanningWorkflowState` is the single source of truth; this method delegates to it
    /// so that `is_planning_active()` and `planning_workflow_state().is_active()` are always in
    /// agreement.
    pub fn enable_planning(&self) {
        let was_active = self.planning_workflow_state.is_active();
        self.planning_workflow_state.enable();
        if !was_active {
            *self.cached_available_tools.write() = None;
            // Invalidate the tool catalog cache so the next snapshot reflects the
            // planning-workflow-filtered tool set rather than serving a stale pre-transition entry.
            self.tool_catalog_state.note_explicit_refresh("planning_workflow_enabled");
        }
    }

    /// Disable planning workflow (allow mutating tools again).
    pub fn disable_planning(&self) {
        let was_active = self.planning_workflow_state.is_active();
        self.planning_workflow_state.disable();
        if was_active {
            *self.cached_available_tools.write() = None;
            // Invalidate the catalog cache so mutating tools reappear immediately.
            self.tool_catalog_state.note_explicit_refresh("planning_workflow_disabled");
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

    /// Apply policy overrides for plan-mode exploration.
    ///
    /// Saves the current policy for each exploration tool and sets it to `Allow`
    /// so plan mode can read the codebase without requiring manual config edits.
    /// Idempotent: if overrides are already active, does nothing.
    pub async fn apply_planning_mode_policy_overrides(&self) {
        // Idempotent guard: don't double-apply on re-entrant transitions.
        if self.planning_mode_policy_overrides.read().is_some() {
            return;
        }

        let mut saved: IndexMap<String, ToolPolicy> = IndexMap::new();
        for &tool in PLAN_MODE_EXPLORATION_TOOLS {
            let canonical = canonical_tool_name(tool);
            let current = self.get_tool_policy(canonical).await;
            if current != ToolPolicy::Allow {
                saved.insert(canonical.to_owned(), current);
                if let Err(err) = self.set_tool_policy(canonical, ToolPolicy::Allow).await {
                    tracing::warn!(tool = %canonical, error = %err, "failed to apply plan-mode policy override");
                }
            }
        }

        if !saved.is_empty() {
            *self.planning_mode_policy_overrides.write() = Some(saved);
        }
    }

    /// Restore policies saved by `apply_planning_mode_policy_overrides`.
    ///
    /// Only touches tools whose policies were actually overridden; all other
    /// user-configured policies are left untouched.
    pub async fn restore_post_planning_policies(&self) {
        let saved = self.planning_mode_policy_overrides.write().take();
        if let Some(saved) = saved {
            for (tool, policy) in saved {
                if let Err(err) = self.set_tool_policy(&tool, policy).await {
                    tracing::warn!(tool = %tool, error = %err, "failed to restore post-planning policy");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use tempfile::TempDir;

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

        assert_eq!(after_first, after_second, "enabling an already-active planning workflow must not bump the epoch");
        assert!(registry.is_planning_active());
    }

    #[tokio::test]
    async fn planning_transitions_invalidate_available_tools() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let inactive = registry.available_tools().await;
        assert!(!inactive.contains(&tools::CODE_SEARCH.to_string()));
        assert!(!inactive.contains(&tools::REQUEST_USER_INPUT.to_string()));

        registry.enable_planning();
        let active = registry.available_tools().await;
        assert!(active.contains(&tools::CODE_SEARCH.to_string()));
        assert!(active.contains(&tools::REQUEST_USER_INPUT.to_string()));

        registry.disable_planning();
        let inactive_again = registry.available_tools().await;
        assert!(!inactive_again.contains(&tools::CODE_SEARCH.to_string()));
        assert!(!inactive_again.contains(&tools::REQUEST_USER_INPUT.to_string()));
    }

    #[tokio::test]
    async fn apply_planning_mode_policy_overrides_allows_exploration_tools() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.apply_planning_mode_policy_overrides().await;

        for &tool in PLAN_MODE_EXPLORATION_TOOLS {
            let canonical = canonical_tool_name(tool);
            assert_eq!(
                registry.get_tool_policy(canonical).await,
                ToolPolicy::Allow,
                "{canonical} should be Allow during planning"
            );
        }
    }

    #[tokio::test]
    async fn apply_planning_mode_policy_overrides_is_idempotent() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.apply_planning_mode_policy_overrides().await;
        registry.apply_planning_mode_policy_overrides().await;

        for &tool in PLAN_MODE_EXPLORATION_TOOLS {
            let canonical = canonical_tool_name(tool);
            assert_eq!(
                registry.get_tool_policy(canonical).await,
                ToolPolicy::Allow,
                "{canonical} should remain Allow after double-apply"
            );
        }
    }

    #[tokio::test]
    async fn restore_post_planning_policies_restores_original_policies() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        // Set exec_command to Prompt explicitly so we can verify restoration
        registry.set_tool_policy(tools::EXEC_COMMAND, ToolPolicy::Prompt).await.ok();

        registry.apply_planning_mode_policy_overrides().await;
        assert_eq!(registry.get_tool_policy(tools::EXEC_COMMAND).await, ToolPolicy::Allow);

        registry.restore_post_planning_policies().await;
        assert_eq!(
            registry.get_tool_policy(tools::EXEC_COMMAND).await,
            ToolPolicy::Prompt,
            "exec_command policy should be restored to Prompt"
        );
    }

    #[tokio::test]
    async fn restore_post_planning_policies_preserves_unrelated_tools() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.set_tool_policy(tools::APPLY_PATCH, ToolPolicy::Deny).await.ok();

        registry.apply_planning_mode_policy_overrides().await;
        registry.restore_post_planning_policies().await;

        assert_eq!(
            registry.get_tool_policy(tools::APPLY_PATCH).await,
            ToolPolicy::Deny,
            "unrelated tool policy must not be touched"
        );
    }

    #[tokio::test]
    async fn restore_without_apply_is_noop() {
        let temp_dir = TempDir::new().expect("tempdir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.restore_post_planning_policies().await;

        for &tool in PLAN_MODE_EXPLORATION_TOOLS {
            let canonical = canonical_tool_name(tool);
            let policy = registry.get_tool_policy(canonical).await;
            assert!(
                matches!(policy, ToolPolicy::Allow | ToolPolicy::Prompt),
                "{canonical} policy should be unchanged: {policy:?}"
            );
        }
    }
}
