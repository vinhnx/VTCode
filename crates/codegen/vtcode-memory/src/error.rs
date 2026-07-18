//! Error type for the session store.

use std::path::PathBuf;

/// Errors produced by the session store.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    /// IO error while reading or writing a session artifact.
    #[error("session store IO error at {path}: {source}")]
    Io {
        /// Path that triggered the error.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A JSON (de)serialization error.
    #[error("session store serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// The requested turn does not exist in the index.
    #[error("turn {turn} not found in session {session}")]
    TurnNotFound {
        /// Session id.
        session: String,
        /// Missing turn number.
        turn: u64,
    },

    /// The store directory could not be created.
    #[error("failed to create session directory {path}: {source}")]
    CreateDir {
        /// Path that could not be created.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
}

impl SessionStoreError {
    /// Convenience constructor for an IO error at a path.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path: path.into(), source }
    }
}
