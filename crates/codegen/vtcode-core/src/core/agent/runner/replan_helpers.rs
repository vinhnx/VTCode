//! Replan and augment helpers for the plan-build-evaluate harness.
//!
//! Contains methods for replanning after evaluator rejection and augmenting
//! generator tasks with contract information.

use super::AgentRunner;
use super::orchestration::{EvaluationArtifacts, PlannerArtifacts};
use super::planner_types::ReplanResponse;
use crate::core::agent::harness_artifacts;
use crate::core::agent::task::Task;
use crate::tools::handlers::TaskTrackerTool;
use crate::tools::traits::Tool;
use serde_json::json;
use tracing::warn;

impl AgentRunner {
    /// Build a `TaskTrackerTool` from the runner's workspace and planning
    /// workflow state. Extracted to avoid repeating construction in
    /// `apply_required_tracker_updates` and `replan_from_failure`.
    fn tracker_tool(&self) -> TaskTrackerTool {
        TaskTrackerTool::new(self._workspace.clone(), self.tool_registry.planning_workflow_state())
    }

    /// Re-plan from the current state after an evaluator rejection.
    ///
    /// Following the long-running harness pattern: "the evaluator takes on part
    /// of the local planner role for feedback-driven replanning." This method:
    ///
    /// 1. Attempts an LLM-based structured replan (`request_replan_response`)
    ///    that produces a revised feature list, contract addendum, and new
    ///    tracker items.
    /// 2. Applies the evaluator's `required_tracker_updates` to the tracker.
    /// 3. Falls back to annotation-only (appending evaluator feedback to
    ///    spec/contract/feature-list) if the LLM replan fails.
    pub(super) async fn replan_from_failure(
        &mut self,
        task: &Task,
        evaluation: &EvaluationArtifacts,
        revision_round: usize,
    ) -> Option<PlannerArtifacts> {
        let spec_path = harness_artifacts::current_spec_path(&self._workspace);
        let contract_path = harness_artifacts::current_contract_path(&self._workspace);
        let tracker_path = harness_artifacts::current_task_path(&self._workspace);
        let feature_list_path = harness_artifacts::current_feature_list_path(&self._workspace);

        // Apply evaluator's required tracker updates to the tracker tool.
        if !evaluation.required_tracker_updates.is_empty() {
            self.apply_required_tracker_updates(&evaluation.required_tracker_updates).await;
        }

        // Attempt LLM-based structured replan.
        let replan = self.request_replan_response(task, evaluation, revision_round).await;

        if let Some(ref replan) = replan {
            self.apply_replan_response(replan).await;
        } else {
            // Fallback: annotate artifacts with evaluator feedback.
            for (label, path) in [
                ("spec", &spec_path),
                ("contract", &contract_path),
                ("feature_list", &feature_list_path),
            ] {
                annotate_artifact(&self._workspace, path, label, &evaluation.summary, revision_round).await;
            }
        }

        Some(PlannerArtifacts {
            spec_path,
            contract_path,
            tracker_path,
            feature_list_path,
        })
    }

    /// Apply the evaluator's `required_tracker_updates` by adding each as a
    /// new tracker item.
    async fn apply_required_tracker_updates(&self, updates: &[String]) {
        let tracker_tool = self.tracker_tool();
        for update in updates {
            let trimmed = update.trim();
            if trimmed.is_empty() {
                continue;
            }
            let result = tracker_tool
                .execute(json!({
                    "action": "add",
                    "item": trimmed,
                }))
                .await;
            if let Err(e) = result {
                warn!(error = %e, item = trimmed, "failed to add required tracker update");
            }
        }
    }

    /// Apply a structured replan response: overwrite the feature list, append
    /// the contract addendum, and add new tracker items.
    async fn apply_replan_response(&self, replan: &ReplanResponse) {
        if !replan.rationale.is_empty() {
            tracing::info!(
                rationale = %replan.rationale,
                "Applying structured replan from evaluator feedback"
            );
        }
        if let Some(ref feature_list) = replan.revised_feature_list {
            let trimmed = feature_list.trim();
            if !trimmed.is_empty() {
                if let Err(e) = harness_artifacts::write_feature_list(&self._workspace, trimmed).await {
                    warn!(error = %e, "failed to write revised feature list");
                }
            }
        }

        if let Some(ref addendum) = replan.contract_addendum {
            let trimmed = addendum.trim();
            if !trimmed.is_empty() {
                let contract_path = harness_artifacts::current_contract_path(&self._workspace);
                let existing = tokio::fs::read_to_string(&contract_path).await.unwrap_or_default();
                let updated = format!("{existing}\n\n--- Replan Addendum ---\n{trimmed}\n");
                if let Err(e) = harness_artifacts::write_contract(&self._workspace, &updated).await {
                    warn!(error = %e, "failed to write contract addendum");
                }
            }
        }

        if !replan.new_tracker_items.is_empty() {
            let tracker_tool = self.tracker_tool();
            let items: Vec<serde_json::Value> = replan
                .new_tracker_items
                .iter()
                .map(|item| {
                    json!({
                        "description": item.description,
                        "outcome": item.outcome,
                        "verify": item.verify,
                        "files": item.files,
                    })
                })
                .collect();
            if let Err(e) = tracker_tool
                .execute(json!({
                    "action": "add_items",
                    "items": items,
                }))
                .await
            {
                warn!(error = %e, "failed to add new tracker items from replan");
            }
        }
    }

    /// Augment a task with generator contract instructions.
    pub(super) fn augment_generator_task(&self, task: &Task, artifacts: &PlannerArtifacts) -> Task {
        let mut effective_task = task.clone();
        let addendum = format!(
            "Generator contract:\n- Treat `{}`, `{}`, `{}`, and `{}` as the source of truth.\n- The execution contract defines what done must look like in observable terms.\n- The feature list enumerates the project's features with acceptance criteria.\n- Work one tracker step at a time.\n- Do not mark a step done until the implementation and verification evidence both support it.\n- Keep the tracker current.\n- Leave resumable state before yielding.",
            artifacts.spec_path.display(),
            artifacts.contract_path.display(),
            artifacts.feature_list_path.display(),
            artifacts.tracker_path.display()
        );
        effective_task.instructions = Some(match task.instructions.as_deref() {
            Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{addendum}"),
            _ => addendum,
        });
        effective_task
    }
}

async fn annotate_artifact(
    workspace: &std::path::Path,
    path: &std::path::Path,
    label: &str,
    evaluation_summary: &str,
    revision_round: usize,
) {
    let existing = tokio::fs::read_to_string(path).await.unwrap_or_default();
    let annotated = format!(
        "{existing}\n\n\
         --- Revision Round {revision_round} ---\n\
         Evaluator feedback:\n{evaluation_summary}\n",
    );
    let write_fn = match label {
        "spec" => harness_artifacts::write_spec(workspace, &annotated).await,
        "feature_list" => harness_artifacts::write_feature_list(workspace, &annotated).await,
        _ => harness_artifacts::write_contract(workspace, &annotated).await,
    };
    let _ = write_fn.inspect_err(|e| warn!(error = %e, "annotate_artifact: failed to annotate {label}"));
}
