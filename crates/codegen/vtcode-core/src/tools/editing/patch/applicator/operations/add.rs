use std::path::Path;

use super::super::PatchError;
use super::super::io::AtomicWriter;
use super::super::lifecycle::{OperationEffect, OperationState};

pub(crate) struct AddOperation<'a> {
    path: &'a str,
    content: &'a str,
}

impl<'a> AddOperation<'a> {
    pub(crate) fn new(path: &'a str, content: &'a str) -> Self {
        Self { path, content }
    }

    pub(crate) async fn apply(self, root: &Path) -> Result<OperationEffect, PatchError> {
        let full_path = root.join(self.path);
        let mut writer = AtomicWriter::create(&full_path, None).await?;
        writer.write_all(self.content.as_bytes()).await?;

        match writer.commit().await {
            Ok(()) => Ok(OperationEffect::applied(
                OperationState::AddedFile { path: full_path },
                format!("Added file: {} ({} bytes)", self.path, self.content.len()),
            )),
            Err(err) => Err(err),
        }
    }
}
