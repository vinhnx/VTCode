use std::fs::Permissions;
use std::path::Path;

use super::super::io::{AtomicWriter, snapshot_for};
use super::super::lifecycle::{OperationEffect, OperationState};
use super::super::text::{compute_replacements, load_file_lines, write_patched_content};
use super::super::{PatchChunk, PatchError};

pub(crate) struct UpdateOperation<'a> {
    path: &'a str,
    new_path: Option<&'a str>,
    chunks: &'a [PatchChunk],
    permissions: Permissions,
}

impl<'a> UpdateOperation<'a> {
    pub(crate) fn new(
        path: &'a str,
        new_path: Option<&'a str>,
        chunks: &'a [PatchChunk],
        permissions: Permissions,
    ) -> Self {
        Self {
            path,
            new_path,
            chunks,
            permissions,
        }
    }

    pub(crate) async fn apply(self, root: &Path) -> Result<OperationEffect, PatchError> {
        let source_path = root.join(self.path);
        let (original_lines, had_trailing_newline) = load_file_lines(&source_path).await?;
        let replacements = compute_replacements(&original_lines, self.chunks, self.path)?;
        let ensure_trailing_newline =
            had_trailing_newline || self.chunks.iter().any(|chunk| chunk.is_end_of_file());
        let Some(backup) = snapshot_for(&source_path).await? else {
            return Err(PatchError::MissingFile {
                path: self.path.to_string(),
            });
        };

        let destination_path = self
            .new_path
            .map(|rel| root.join(rel))
            .unwrap_or_else(|| source_path.clone());

        let mut writer = AtomicWriter::create(&destination_path, Some(self.permissions)).await?;

        if let Err(err) = write_patched_content(
            &mut writer,
            original_lines,
            replacements,
            ensure_trailing_newline,
        )
        .await
        {
            let _ = writer.rollback().await;
            backup.restore(&source_path).await?;
            return Err(err);
        }

        if let Err(err) = writer.commit().await {
            backup.restore(&source_path).await?;
            return Err(err);
        }

        let chunk_count = self.chunks.len();
        let chunk_label = if chunk_count == 1 { "chunk" } else { "chunks" };
        let detail = match self.new_path {
            Some(dest_rel) if dest_rel != self.path => {
                format!(
                    "Updated file: {} -> {} ({} {chunk_label})",
                    self.path, dest_rel, chunk_count
                )
            }
            _ => format!(
                "Updated file: {} ({} {chunk_label})",
                self.path, chunk_count
            ),
        };

        Ok(OperationEffect::applied(
            OperationState::UpdatedFile {
                original_path: source_path,
                written_path: destination_path,
                backup,
            },
            detail,
        ))
    }
}
