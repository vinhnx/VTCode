use std::path::Path;

use super::executor::OperationExecutor;
use super::journal::OperationJournal;
use super::lifecycle::OperationEffect;
use super::progress::ProgressMarker;
use super::{PatchError, PreparedOperation};

pub(super) async fn execute_plan(
    root: &Path,
    plan: Vec<PreparedOperation<'_>>,
) -> Result<Vec<String>, PatchError> {
    let total = plan.len();
    let executor = OperationExecutor::new(root);
    let mut journal = OperationJournal::new();
    let mut results = Vec::with_capacity(total);
    let progress_total = total.max(1);

    for (index, prepared) in plan.into_iter().enumerate() {
        let marker = ProgressMarker::new(index + 1, progress_total);
        match executor.execute(prepared).await {
            Ok(OperationEffect::Applied { state, detail }) => {
                journal.record(state);
                results.push(marker.annotate(&detail));
            }
            Ok(OperationEffect::Skipped { detail }) => {
                results.push(marker.annotate(&detail));
            }
            Err(err) => {
                let rollback_result = journal.rollback_all().await;
                return Err(match rollback_result {
                    Ok(()) => err,
                    Err(rollback_err) => PatchError::Recovery {
                        original: Box::new(err),
                        rollback: Box::new(rollback_err),
                    },
                });
            }
        }
    }

    journal.commit_all().await?;
    Ok(results)
}
