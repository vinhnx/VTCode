//! Reconcile task-scoped replan additions through the existing tracker API.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AgentRunner, PlannerItem};
use crate::tools::handlers::task_tracking::{
    TaskStepMetadata, TaskTrackingStatus, metadata_from_input, parse_status_prefix,
};
use crate::tools::traits::Tool;

#[derive(Deserialize)]
struct TrackerItem {
    index: Option<usize>,
    index_path: Option<String>,
    description: String,
    status: TaskTrackingStatus,
    #[serde(flatten)]
    metadata: TaskStepMetadata,
}

impl TrackerItem {
    fn covers(&self, requested: &TaskStepMetadata) -> bool {
        (requested.files.is_empty() || requested.files == self.metadata.files)
            && (requested.outcome.is_none() || requested.outcome == self.metadata.outcome)
            && (requested.verify.is_empty() || requested.verify == self.metadata.verify)
    }
}

fn tracker_items(response: &Value) -> Result<Vec<TrackerItem>> {
    let items = response
        .pointer("/checklist/items")
        .context("tracker response omitted checklist items")?;
    serde_json::from_value(items.clone()).context("decode tracker items for replan reconciliation")
}

impl AgentRunner {
    /// Repeated observations and exact structured requirements do not append
    /// duplicate work or reset progress. Enrich only an uncompleted, uniquely
    /// identified placeholder; changed criteria remain a separate pending item.
    pub(super) async fn apply_tracker_items(&self, updates: &[PlannerItem]) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }
        let tool = self.tracker_tool();
        let mut current = tracker_items(&tool.execute(json!({"action":"list"})).await?)?;
        for update in updates {
            let (_, description) = parse_status_prefix(update.description.trim());
            let description = description.trim();
            if description.is_empty() {
                continue;
            }
            let metadata = metadata_from_input(Some(&update.files), Some(&update.outcome), Some(&update.verify));
            let matches: Vec<_> = current.iter().filter(|item| item.description.trim() == description).collect();
            if matches.iter().any(|item| item.covers(&metadata)) {
                continue;
            }
            let mut args = json!({
                "action":"add", "description":description,
                "files":metadata.files, "outcome":metadata.outcome, "verify":metadata.verify,
            });
            if let [item] = matches.as_slice()
                && item.metadata == TaskStepMetadata::default()
                && item.status != TaskTrackingStatus::Completed
            {
                args["action"] = json!("update");
                args["status"] = json!(item.status);
                // Standard updates require a status, and neither tracker accepts
                // both address shapes. Retain a planning step's hierarchical path.
                if let Some(path) = &item.index_path {
                    args["index_path"] = json!(path);
                } else {
                    args["index"] = json!(item.index.context("tracker item omitted its index")?);
                }
                args.as_object_mut()
                    .context("tracker update must be an object")?
                    .remove("description");
            }
            current = tracker_items(&tool.execute(args).await.context("apply reconciled tracker item")?)?;
        }
        Ok(())
    }
}
