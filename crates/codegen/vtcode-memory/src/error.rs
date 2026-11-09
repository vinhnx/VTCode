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

    /// An audit pack is malformed or unsafe (bad schema, path traversal).
    #[error("invalid audit pack: {0}")]
    InvalidPack(String),

    /// The session is already open for writing in another process.
    #[error(
        "session {session} is already open in another process; continue there or fork it with vtcode --fork-session {session}"
    )]
    SessionBusy {
        /// Session id.
        session: String,
    },

    /// A mailbox message failed validation.
    #[error("invalid mail: {0}")]
    InvalidMail(String),

    /// The target session does not exist or is not live.
    #[error("session {session} is not live; messages go to running sessions only")]
    SessionNotLive {
        /// Session id.
        session: String,
    },

    /// The target inbox is full.
    #[error("session {session} has too many unread messages; it must drain before accepting more")]
    MailboxFull {
        /// Session id.
        session: String,
    },
}

impl SessionStoreError {
    /// Convenience constructor for an IO error at a path.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path: path.into(), source }
    }

    /// Convenience constructor for a mailbox validation failure.
    pub(crate) fn invalid_mail(reason: impl Into<String>) -> Self {
        Self::InvalidMail(reason.into())
    }

    /// Convenience constructor for a message to a non-live session.
    pub(crate) fn session_not_live(session: impl Into<String>) -> Self {
        Self::SessionNotLive { session: session.into() }
    }

    /// Convenience constructor for a send to a full inbox.
    pub(crate) fn mailbox_full(session: impl Into<String>) -> Self {
        Self::MailboxFull { session: session.into() }
    }
}
