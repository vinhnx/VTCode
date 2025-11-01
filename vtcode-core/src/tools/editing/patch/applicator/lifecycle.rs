use std::path::PathBuf;

use super::PatchError;
use super::io::{BackupEntry, remove_existing_entry};

pub(crate) enum OperationEffect {
    Applied {
        state: OperationState,
        detail: String,
    },
    Skipped {
        detail: String,
    },
}

impl OperationEffect {
    pub(crate) fn applied(state: OperationState, detail: impl Into<String>) -> Self {
        Self::Applied {
            state,
            detail: detail.into(),
        }
    }

    pub(crate) fn skipped(detail: impl Into<String>) -> Self {
        Self::Skipped {
            detail: detail.into(),
        }
    }
}

pub(crate) enum OperationState {
    AddedFile {
        path: PathBuf,
    },
    DeletedFile {
        original_path: PathBuf,
        backup: BackupEntry,
    },
    UpdatedFile {
        original_path: PathBuf,
        written_path: PathBuf,
        backup: BackupEntry,
    },
}

impl OperationState {
    pub(crate) async fn commit(self) -> Result<(), PatchError> {
        match self {
            OperationState::AddedFile { .. } => Ok(()),
            OperationState::DeletedFile { backup, .. }
            | OperationState::UpdatedFile { backup, .. } => backup.remove().await,
        }
    }

    pub(crate) async fn rollback(self) -> Result<(), PatchError> {
        match self {
            OperationState::AddedFile { path } => remove_existing_entry(&path).await,
            OperationState::DeletedFile {
                original_path,
                backup,
            } => backup.restore(&original_path).await,
            OperationState::UpdatedFile {
                original_path,
                written_path,
                backup,
            } => {
                remove_existing_entry(&written_path).await?;
                backup.restore(&original_path).await
            }
        }
    }
}
