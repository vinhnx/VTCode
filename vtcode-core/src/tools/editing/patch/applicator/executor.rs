use std::path::Path;

use super::lifecycle::OperationEffect;
use super::operations::Operation;
use super::{PatchError, PreparedOperation};

pub(super) struct OperationExecutor<'a> {
    root: &'a Path,
}

impl<'a> OperationExecutor<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub(super) async fn execute(
        &self,
        operation: PreparedOperation<'_>,
    ) -> Result<OperationEffect, PatchError> {
        Operation::from_prepared(operation).apply(self.root).await
    }
}
