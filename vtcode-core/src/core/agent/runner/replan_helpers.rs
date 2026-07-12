//! Replan and augment helpers for the plan-build-evaluate harness.
//!
//! Contains methods for replanning after evaluator rejection and augmenting
//! generator tasks with contract information.

use super::AgentRunner;
use super::orchestration::{EvaluationArtifacts, PlannerArtifacts};
use crate::core::agent::harness_artifacts;
use crate::core::agent::task::Task;
use tracing::warn;

impl AgentRunner {
    /// Re-plan from the current state after an evaluator rejection.
    ///
    /// Appends evaluator feedback to the existing spec and contract files so the
    /// generator can see what went wrong. This is intentionally LLM-free —
    /// the artifacts are updated in-place and the next continuation loop picks
    /// them up transparently.
    pub(super) async fn replan_from_failure(
        &mut self,
        _task: &Task,
        evaluation: &EvaluationArtifacts,
        revision_round: usize,
    ) -> Option<PlannerArtifacts> {
        let spec_path = harness_artifacts::current_spec_path(&self._workspace);
        let contract_path = harness_artifacts::current_contract_path(&self._workspace);
        let tracker_path = harness_artifacts::current_task_path(&self._workspace);

        // Annotate both artifacts with evaluator feedback using shared logic.
        for (label, path) in [("spec", &spec_path), ("contract", &contract_path)] {
            annotate_artifact(
                &self._workspace,
                path,
                label,
                &evaluation.summary,
                revision_round,
            )
            .await;
        }

        Some(PlannerArtifacts {
            spec_path,
            contract_path,
            tracker_path,
        })
    }

    /// Augment a task with generator contract instructions.
    pub(super) fn augment_generator_task(&self, task: &Task, artifacts: &PlannerArtifacts) -> Task {
        let mut effective_task = task.clone();
        let addendum = format!(
            "Generator contract:\n- Treat `{}`, `{}`, and `{}` as the source of truth.\n- The execution contract defines what done must look like in observable terms.\n- Work one tracker step at a time.\n- Do not mark a step done until the implementation and verification evidence both support it.\n- Keep the tracker current.\n- Leave resumable state before yielding.",
            artifacts.spec_path.display(),
            artifacts.contract_path.display(),
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
        _ => harness_artifacts::write_contract(workspace, &annotated).await,
    };
    let _ = write_fn
        .inspect_err(|e| warn!(error = %e, "annotate_artifact: failed to annotate {label}"));
}
