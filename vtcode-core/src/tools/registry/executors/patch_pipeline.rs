use super::ToolRegistry;
use crate::tools::edited_file_monitor::{MutationLease, conflict_override_snapshot};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::fs;

enum PlannedPatchWrite {
    Text { path: PathBuf, content: String },
    Removal { path: PathBuf },
}

impl ToolRegistry {
    pub(super) async fn execute_apply_patch_internal(&self, args: Value) -> Result<Value> {
        let patch_input = crate::tools::apply_patch::decode_apply_patch_input(&args)?
            .ok_or_else(|| anyhow!("Missing patch input (use 'input' or 'patch' parameter)"))?;
        let override_snapshot = conflict_override_snapshot(&args);

        let patch = crate::tools::editing::Patch::parse(&patch_input.text)?;
        let _mutation_leases = self.acquire_patch_mutations(&patch).await?;
        let planned_writes = self.planned_patch_writes(&patch).await?;
        for operation in patch.operations() {
            if let Some(conflict) = self
                .detect_patch_operation_conflict(operation, override_snapshot.clone())
                .await?
            {
                return Ok(conflict.to_tool_output(self.workspace_root()));
            }
        }

        let results = patch.apply(&self.workspace_root_owned()).await?;
        for write in planned_writes {
            let (path, result) = match write {
                PlannedPatchWrite::Text { path, content } => {
                    let result = self
                        .edited_file_monitor_ref()
                        .record_agent_write_text(&path, &content);
                    (path, result)
                }
                PlannedPatchWrite::Removal { path } => {
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

        Ok(json!({
            "success": true,
            "applied": results,
        }))
    }

    async fn patch_mutation_paths(
        &self,
        patch: &crate::tools::editing::Patch,
    ) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for operation in patch.operations() {
            match operation {
                crate::tools::editing::PatchOperation::AddFile { path, .. }
                | crate::tools::editing::PatchOperation::DeleteFile { path } => {
                    paths.push(self.file_ops_tool().normalize_user_path(path).await?);
                }
                crate::tools::editing::PatchOperation::UpdateFile { path, new_path, .. } => {
                    paths.push(self.file_ops_tool().normalize_user_path(path).await?);
                    if let Some(destination) = new_path
                        .as_ref()
                        .filter(|candidate| candidate.as_str() != path.as_str())
                    {
                        paths.push(
                            self.file_ops_tool()
                                .normalize_user_path(destination)
                                .await?,
                        );
                    }
                }
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    async fn planned_patch_writes(
        &self,
        patch: &crate::tools::editing::Patch,
    ) -> Result<Vec<PlannedPatchWrite>> {
        let mut writes = Vec::new();
        for operation in patch.operations() {
            writes.extend(self.planned_patch_writes_for_operation(operation).await?);
        }
        Ok(writes)
    }

    async fn acquire_patch_mutations(
        &self,
        patch: &crate::tools::editing::Patch,
    ) -> Result<Vec<MutationLease>> {
        let mut leases = Vec::new();
        for path in self.patch_mutation_paths(patch).await? {
            leases.push(self.edited_file_monitor_ref().acquire_mutation(&path).await);
        }
        Ok(leases)
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
                let intended_content =
                    if let Some(content) = monitor.tracked_read_text(&canonical_path).await {
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
            crate::tools::editing::PatchOperation::AddFile { path, content } => {
                Ok(vec![PlannedPatchWrite::Text {
                    path: self.file_ops_tool().normalize_user_path(path).await?,
                    content: content.clone(),
                }])
            }
            crate::tools::editing::PatchOperation::DeleteFile { path } => {
                Ok(vec![PlannedPatchWrite::Removal {
                    path: self.file_ops_tool().normalize_user_path(path).await?,
                }])
            }
            crate::tools::editing::PatchOperation::UpdateFile {
                path,
                new_path,
                chunks,
            } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                let source_content = if let Some(content) = self
                    .edited_file_monitor_ref()
                    .tracked_read_text(&canonical_path)
                    .await
                {
                    content
                } else {
                    match fs::read_to_string(&canonical_path).await {
                        Ok(content) => content,
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                            return Err(anyhow!(crate::tools::editing::PatchError::MissingFile {
                                path: canonical_path.display().to_string(),
                            }));
                        }
                        Err(err) => {
                            return Err(err).with_context(|| {
                                format!(
                                    "Failed to read patch source content for {}",
                                    canonical_path.display()
                                )
                            });
                        }
                    }
                };

                let rendered = crate::tools::editing::patch::render_patch_update_content(
                    &canonical_path,
                    &source_content,
                    chunks,
                    path,
                )
                .await
                .map_err(|err| {
                    anyhow!(
                        "Failed to plan patch output for {}: {err}",
                        canonical_path.display()
                    )
                })?;

                let mut writes = Vec::new();
                if let Some(destination) = new_path
                    .as_ref()
                    .filter(|candidate| candidate.as_str() != path.as_str())
                {
                    writes.push(PlannedPatchWrite::Removal {
                        path: canonical_path,
                    });
                    writes.push(PlannedPatchWrite::Text {
                        path: self
                            .file_ops_tool()
                            .normalize_user_path(destination)
                            .await?,
                        content: rendered,
                    });
                } else {
                    writes.push(PlannedPatchWrite::Text {
                        path: canonical_path,
                        content: rendered,
                    });
                }

                Ok(writes)
            }
        }
    }
}
