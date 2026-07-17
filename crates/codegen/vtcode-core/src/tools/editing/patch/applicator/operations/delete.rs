use std::path::Path;

use super::super::PatchError;
use super::super::io::snapshot_for;
use super::super::lifecycle::{OperationEffect, OperationState};

pub(crate) struct DeleteOperation<'a> {
    path: &'a str,
}

impl<'a> DeleteOperation<'a> {
    pub(crate) fn new(path: &'a str) -> Self {
        Self { path }
    }

    pub(crate) async fn apply(self, root: &Path) -> Result<OperationEffect, PatchError> {
        let full_path = root.join(self.path);
        match snapshot_for(&full_path).await? {
            Some(backup) => Ok(OperationEffect::applied(
                OperationState::DeletedFile {
                    original_path: full_path,
                    backup,
                },
                format!("Deleted file: {}", self.path),
            )),
            None => Ok(OperationEffect::skipped(format!(
                "File not found, skipped deletion: {}",
                self.path
            ))),
        }
    }
}
