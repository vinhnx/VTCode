use super::ToolRegistry;
use crate::tools::edited_file_monitor::{MutationLease, conflict_override_snapshot};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::fs;

enum PlannedPatchWrite {
    Text {
        path: PathBuf,
        before: Option<String>,
        content: String,
    },
    Removal {
        path: PathBuf,
        before: Option<String>,
    },
}

impl ToolRegistry {
    pub(super) async fn execute_apply_patch_internal(&self, args: Value) -> Result<Value> {
        let patch_input = crate::tools::apply_patch::decode_apply_patch_input(&args)?
            .ok_or_else(|| anyhow!("Missing patch input {}", crate::tools::error_helpers::PATCH_PARAMETER_HINT))?;
        let override_snapshot = conflict_override_snapshot(&args);

        let patch = crate::tools::editing::Patch::parse(&patch_input.text)?;
        let mutation_paths = self.patch_mutation_paths(&patch).await?;
        let _mutation_leases = self.acquire_patch_mutations(&mutation_paths).await;
        for operation in patch.operations() {
            if let Some(conflict) = self
                .detect_patch_operation_conflict(operation, override_snapshot.clone())
                .await?
            {
                return Ok(conflict.to_tool_output(self.workspace_root()));
            }
        }

        let inspection = self.edited_file_monitor_ref().source_inspection();
        let inspection_plan = inspection.prepare_patch(&patch, self.workspace_root()).await?;
        let planned_writes = self.planned_patch_writes(&patch).await?;

        let diff = self.patch_diff_previews(&planned_writes);
        let results = patch.apply(&self.workspace_root_owned()).await?;
        inspection.commit_patch(
            inspection_plan,
            crate::tools::source_inspection::call_key(crate::config::constants::tools::APPLY_PATCH, &args),
        );
        for write in planned_writes {
            let (path, result) = match write {
                PlannedPatchWrite::Text { path, content, .. } => {
                    let result = self.edited_file_monitor_ref().record_agent_write_text(&path, &content);
                    (path, result)
                }
                PlannedPatchWrite::Removal { path, .. } => {
                    let result = self.edited_file_monitor_ref().record_agent_removal(&path);
                    (path, result)
                }
            };

            if let Err(err) = result {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to refresh edited-file snapshot after apply_patch"
                );
            }
        }

        let mut response = json!({
            "success": true,
            "applied": results,
            "modified_files": mutation_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        });
        response["diff"] = Value::Array(diff);
        Ok(response)
    }

    async fn patch_mutation_paths(&self, patch: &crate::tools::editing::Patch) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for path in crate::tools::apply_patch::patch_mutation_target_paths(patch) {
            let path = path.to_str().ok_or_else(|| anyhow!("apply_patch path is not valid UTF-8"))?;
            paths.push(self.file_ops_tool().normalize_user_path(path).await?);
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    async fn planned_patch_writes(&self, patch: &crate::tools::editing::Patch) -> Result<Vec<PlannedPatchWrite>> {
        let mut writes = Vec::new();
        for operation in patch.operations() {
            writes.extend(self.planned_patch_writes_for_operation(operation).await?);
        }
        Ok(writes)
    }

    fn patch_diff_previews(&self, writes: &[PlannedPatchWrite]) -> Vec<Value> {
        writes
            .iter()
            .map(|write| {
                let (path, before, after) = match write {
                    PlannedPatchWrite::Text { path, before, content } => (path, before.as_deref(), content.as_str()),
                    PlannedPatchWrite::Removal { path, before } => (path, before.as_deref(), ""),
                };
                let operation = match write {
                    PlannedPatchWrite::Text { before: None, .. } => "created",
                    PlannedPatchWrite::Text { before: Some(_), .. } => "updated",
                    PlannedPatchWrite::Removal { .. } => "deleted",
                };
                let relative_path = path
                    .strip_prefix(self.workspace_root())
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                let mut preview = crate::tools::file_ops::build_diff_preview(&relative_path, before, after);
                if let Some(fields) = preview.as_object_mut() {
                    fields.insert("path".to_string(), Value::String(relative_path));
                    fields.insert("operation".to_string(), Value::String(operation.to_string()));
                }
                preview
            })
            .collect()
    }

    async fn acquire_patch_mutations(&self, mutation_paths: &[PathBuf]) -> Vec<MutationLease> {
        let mut leases = Vec::new();
        for path in mutation_paths {
            leases.push(self.edited_file_monitor_ref().acquire_mutation(path).await);
        }
        leases
    }

    async fn detect_patch_operation_conflict(
        &self,
        operation: &crate::tools::editing::PatchOperation,
        override_snapshot: Option<crate::tools::edited_file_monitor::FileSnapshot>,
    ) -> Result<Option<crate::tools::edited_file_monitor::FileConflict>> {
        let monitor = self.edited_file_monitor_ref();
        match operation {
            crate::tools::editing::PatchOperation::AddFile { path, content } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                monitor
                    .detect_conflict(&canonical_path, Some(content.clone()), override_snapshot)
                    .await
            }
            crate::tools::editing::PatchOperation::DeleteFile { path } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                monitor
                    .detect_conflict(&canonical_path, Some(String::new()), override_snapshot)
                    .await
            }
            crate::tools::editing::PatchOperation::UpdateFile { path, chunks, .. } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                let intended_content = if let Some(content) = monitor.tracked_read_text(&canonical_path).await {
                    match crate::tools::editing::patch::render_patch_update_content(
                        &canonical_path,
                        &content,
                        chunks,
                        path,
                    )
                    .await
                    {
                        Ok(rendered) => Some(rendered),
                        Err(err) => {
                            tracing::debug!(
                                path = %canonical_path.display(),
                                error = %err,
                                "Failed to render patch conflict preview content"
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                monitor
                    .detect_conflict(&canonical_path, intended_content, override_snapshot)
                    .await
            }
        }
    }

    async fn planned_patch_writes_for_operation(
        &self,
        operation: &crate::tools::editing::PatchOperation,
    ) -> Result<Vec<PlannedPatchWrite>> {
        match operation {
            crate::tools::editing::PatchOperation::AddFile { path, content } => Ok(vec![PlannedPatchWrite::Text {
                path: self.file_ops_tool().normalize_user_path(path).await?,
                before: None,
                content: content.clone(),
            }]),
            crate::tools::editing::PatchOperation::DeleteFile { path } => {
                let path = self.file_ops_tool().normalize_user_path(path).await?;
                let before = fs::read_to_string(&path).await.ok();
                Ok(vec![PlannedPatchWrite::Removal { path, before }])
            }
            crate::tools::editing::PatchOperation::UpdateFile { path, new_path, chunks } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                let source_content = fs::read_to_string(&canonical_path)
                    .await
                    .with_context(|| format!("Failed to read patch source {}", canonical_path.display()))?;

                let rendered = crate::tools::editing::patch::render_patch_update_content(
                    &canonical_path,
                    &source_content,
                    chunks,
                    path,
                )
                .await
                .with_context(|| format!("Failed to plan patch output for {}", canonical_path.display()))?;

                let mut writes = Vec::new();
                if let Some(destination) = new_path.as_ref().filter(|candidate| candidate.as_str() != path.as_str()) {
                    writes.push(PlannedPatchWrite::Removal {
                        path: canonical_path,
                        before: Some(source_content.clone()),
                    });
                    writes.push(PlannedPatchWrite::Text {
                        path: self.file_ops_tool().normalize_user_path(destination).await?,
                        before: None,
                        content: rendered,
                    });
                } else {
                    writes.push(PlannedPatchWrite::Text {
                        path: canonical_path,
                        before: Some(source_content),
                        content: rendered,
                    });
                }

                Ok(writes)
            }
        }
    }
}
