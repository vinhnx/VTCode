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

pub(crate) use planner::PreparedOperation;

pub(crate) async fn apply(
    root: &Path,
    operations: &[PatchOperation],
) -> Result<Vec<String>, PatchError> {
    let plan = planner::plan_operations(root, operations).await?;
    runner::execute_plan(root, plan).await
}

pub(crate) fn render_updated_content<'a>(
    source_path: &'a Path,
    content: &'a str,
    chunks: &'a [PatchChunk],
    path: &'a str,
) -> impl Future<Output = Result<String, PatchError>> + 'a {
    text::render_patched_text_from_content(source_path, content, chunks, path)
}
