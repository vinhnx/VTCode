use std::future::Future;
use std::path::Path;

pub(super) use super::error::PatchError;
pub(super) use super::{PatchChunk, PatchOperation};

mod executor;
mod io;
mod journal;
mod lifecycle;
mod operations;
mod planner;
mod progress;
mod runner;
mod text;
pub(crate) use text::inspection_replacements;

pub(crate) use planner::PreparedOperation;

pub(crate) async fn apply(root: &Path, operations: &[PatchOperation]) -> Result<Vec<String>, PatchError> {
    ensure_operation_targets_within_workspace(root, operations).await?;
    let plan = planner::plan_operations(root, operations).await?;
    runner::execute_plan(root, plan).await
}

async fn ensure_operation_targets_within_workspace(
    root: &Path,
    operations: &[PatchOperation],
) -> Result<(), PatchError> {
    for operation in operations {
        match operation {
            PatchOperation::AddFile { path, .. } | PatchOperation::DeleteFile { path } => {
                ensure_target_within_workspace(root, path).await?;
            }
            PatchOperation::UpdateFile { path, new_path, .. } => {
                ensure_target_within_workspace(root, path).await?;
                if let Some(new_path) = new_path {
                    ensure_target_within_workspace(root, new_path).await?;
                }
            }
        }
    }

    Ok(())
}

async fn ensure_target_within_workspace(root: &Path, path: &str) -> Result<(), PatchError> {
    let candidate = root.join(path);
    vtcode_commons::paths::ensure_path_within_workspace_resolved(&candidate, root)
        .await
        .map(|_| ())
        .map_err(|error| PatchError::InvalidPath {
            operation: "apply",
            path: path.to_string(),
            reason: format!("workspace containment check failed: {error}"),
        })
}

pub(crate) fn render_updated_content<'a>(
    source_path: &'a Path,
    content: &'a str,
    chunks: &'a [PatchChunk],
    path: &'a str,
) -> impl Future<Output = Result<String, PatchError>> + 'a {
    text::render_patched_text_from_content(source_path, content, chunks, path)
}
