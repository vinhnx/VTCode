use super::PatchError;
use super::lifecycle::OperationState;

pub(super) struct OperationJournal {
    applied: Vec<OperationState>,
}

impl OperationJournal {
    pub(super) fn new() -> Self {
        Self {
            applied: Vec::new(),
        }
    }

    pub(super) fn record(&mut self, state: OperationState) {
        self.applied.push(state);
    }

    pub(super) async fn rollback_all(&mut self) -> Result<(), PatchError> {
        let mut rollback_error = None;

        while let Some(state) = self.applied.pop() {
            if let Err(err) = state.rollback().await {
                rollback_error.get_or_insert(err);
            }
        }

        if let Some(err) = rollback_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    pub(super) async fn commit_all(mut self) -> Result<(), PatchError> {
        while let Some(state) = self.applied.pop() {
            state.commit().await?;
        }
        Ok(())
    }
}
