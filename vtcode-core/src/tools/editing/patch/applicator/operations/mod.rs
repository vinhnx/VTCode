mod add;
mod delete;
mod update;

use std::path::Path;

use super::lifecycle::OperationEffect;
use super::{PatchError, PreparedOperation};

pub(crate) enum Operation<'a> {
    Add(add::AddOperation<'a>),
    Delete(delete::DeleteOperation<'a>),
    Update(update::UpdateOperation<'a>),
}

impl<'a> Operation<'a> {
    pub(crate) fn from_prepared(prepared: PreparedOperation<'a>) -> Self {
        match prepared {
            PreparedOperation::Add { path, content } => {
                Operation::Add(add::AddOperation::new(path, content))
            }
            PreparedOperation::Delete { path } => {
                Operation::Delete(delete::DeleteOperation::new(path))
            }
            PreparedOperation::Update {
                path,
                new_path,
                chunks,
                permissions,
            } => Operation::Update(update::UpdateOperation::new(
                path,
                new_path,
                chunks,
                permissions,
            )),
        }
    }

    pub(crate) async fn apply(self, root: &Path) -> Result<OperationEffect, PatchError> {
        match self {
            Operation::Add(operation) => operation.apply(root).await,
            Operation::Delete(operation) => operation.apply(root).await,
            Operation::Update(operation) => operation.apply(root).await,
        }
    }
}
