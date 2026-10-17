use super::ToolRegistry;
use anyhow::{Context, Result, ensure};
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::atomic::Ordering;

impl ToolRegistry {
    pub(crate) fn matrix_worker_outcome(&self) -> Option<crate::exec::events::matrix::MatrixOutcome> {
        self.matrix_worker.read().as_ref().and_then(|worker| worker.reported_outcome())
    }
    pub(crate) fn has_matrix_access(&self) -> bool {
        self.matrix_coordinator.load(Ordering::Acquire) || self.matrix_worker.read().is_some()
    }
    pub(crate) fn set_matrix_worker(&self, context: crate::subagents::matrix::MatrixWorkerContext) {
        *self.matrix_worker.write() = Some(context);
    }
    pub fn set_matrix_coordinator(&self, active: bool) {
        self.matrix_coordinator.store(active, Ordering::Release);
    }
    pub(crate) fn matrix_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let worker = self.matrix_worker.read().clone();
            if let Some(worker) = worker {
                if let Some(evidence) = args.get("evidence_ids") {
                    let ids = evidence
                        .as_array()
                        .context("evidence_ids must be an array")?
                        .iter()
                        .map(|id| {
                            id.as_str()
                                .filter(|id| !id.is_empty())
                                .map(str::to_owned)
                                .context("evidence IDs must be nonempty strings")
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if !ids.is_empty() {
                        let validator = self
                            .harness_context
                            .decision_validator
                            .read()
                            .clone()
                            .context("canonical worker evidence validator unavailable")?;
                        let task = self
                            .harness_context_snapshot()
                            .task_id
                            .context("worker task identity unavailable")?;
                        validator(task, ids).await?;
                    }
                }
                return worker.report(args);
            }
            ensure!(
                self.matrix_coordinator.load(Ordering::Acquire),
                "matrix control requires selected coordinator role"
            );
            let controller = self.subagent_controller().context("matrix requires a subagent controller")?;
            controller.matrix_control(args).await
        })
    }
    pub(crate) fn enforce_matrix_role(&self, canonical: &str) -> Result<()> {
        if self.matrix_worker.read().is_some() {
            ensure!(!crate::subagents::is_subagent_tool(canonical), "nested delegation is disabled for matrix workers");
        } else if self.matrix_coordinator.load(Ordering::Acquire) {
            ensure!(
                [
                    "matrix",
                    "agent",
                    "spawn_agent",
                    "wait_agent",
                    "close_agent",
                    "request_user_input",
                    "record_decision",
                    "task_tracker"
                ]
                .contains(&canonical),
                "coordinator delegates discovery, execution, edits and verification"
            );
        }
        Ok(())
    }
}
