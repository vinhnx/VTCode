//! Planner helper methods for the plan-build-evaluate harness.
//!
//! Contains helper functions for generating fallback specs, building tracker
//! items, and normalizing planner output.

use super::AgentRunner;
use super::planner_types::PlannerItem;
use crate::core::agent::task::Task;
use serde_json::json;

impl AgentRunner {
    /// Generate a fallback execution spec when the planner doesn't provide one.
    pub(super) fn fallback_spec_markdown(&self, task: &Task) -> String {
        format!(
            "# Execution Spec\n\n## Goal\n{}\n\n## Acceptance Criteria\n- Complete the requested work.\n- Keep the tracker concrete and verifiable.\n\n## Assumptions\n- Scope remains limited to the user request.\n- Verification should use the lightest project-appropriate command available.\n",
            task.description.trim()
        )
    }

    /// Generate fallback planner items when the planner doesn't provide any.
    pub(super) fn fallback_planner_items(&self, task: &Task) -> Vec<serde_json::Value> {
        let verify = self.fallback_verify_commands();
        vec![json!({
            "description": task.description,
            "outcome": "Requested work is implemented and the tracker reflects the final state.",
            "verify": verify,
        })]
    }

    /// Build tracker items from planner output, falling back to defaults if empty.
    pub(super) fn build_planner_tracker_items(
        &self,
        task: &Task,
        items: Vec<PlannerItem>,
    ) -> Vec<serde_json::Value> {
        let fallback_verify = self.fallback_verify_commands();
        let tracker_items = items
            .into_iter()
            .filter_map(|item| self.normalize_planner_item(task, item, &fallback_verify))
            .collect::<Vec<_>>();
        if tracker_items.is_empty() {
            self.fallback_planner_items(task)
        } else {
            tracker_items
        }
    }

    /// Normalize a planner item into a tracker item JSON value.
    pub(super) fn normalize_planner_item(
        &self,
        task: &Task,
        item: PlannerItem,
        fallback_verify: &[String],
    ) -> Option<serde_json::Value> {
        let description = item.description.trim();
        let description = if description.is_empty() {
            task.description.trim()
        } else {
            description
        };
        if description.is_empty() {
            return None;
        }

        let outcome = item.outcome.trim();
        let outcome = if outcome.is_empty() {
            "Requested work is implemented and the tracker reflects the final state."
        } else {
            outcome
        };
        let files = item
            .files
            .into_iter()
            .map(|file| file.trim().to_string())
            .filter(|file| !file.is_empty())
            .collect::<Vec<_>>();
        let verify = item
            .verify
            .into_iter()
            .map(|command| command.trim().to_string())
            .filter(|command| !command.is_empty())
            .collect::<Vec<_>>();
        let verify = if verify.is_empty() {
            fallback_verify.to_vec()
        } else {
            verify
        };

        Some(json!({
            "description": description,
            "files": files,
            "outcome": outcome,
            "verify": verify,
        }))
    }

    /// Get the default verify commands for the workspace.
    pub(super) fn fallback_verify_commands(&self) -> Vec<String> {
        super::workspace_detection::infer_default_verify_commands(self._workspace.as_path())
    }
}
