use std::path::Path;

use super::lifecycle::OperationEffect;
use super::operations::Operation;
use super::{PatchError, PreparedOperation};

pub(super) struct OperationExecutor<'a> {
    root: &'a Path,
}

use std::future::Future;

impl<'a> OperationExecutor<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub(super) fn execute(
        &self,
        operation: PreparedOperation<'a>,
    ) -> impl Future<Output = Result<OperationEffect, PatchError>> + 'a {
        Operation::from_prepared(operation).apply(self.root)
    }
}
