use std::path::PathBuf;

use thiserror::Error;

/// Errors produced while parsing or applying agent patches.
#[derive(Debug, Error)]
pub enum PatchError {
    #[error("cannot parse empty patch input")]
    EmptyInput,

    #[error("patch does not contain any operations")]
    NoOperations,

    #[error("invalid patch format: {0}")]
    InvalidFormat(String),

    #[error("invalid patch hunk on line {line}: {message}")]
    InvalidHunk { line: usize, message: String },

    #[error("invalid patch operation for '{path}': {reason}")]
    InvalidOperation { path: String, reason: String },

    #[error("invalid path for {operation}: {path} ({reason})")]
    InvalidPath {
        operation: &'static str,
        path: String,
        reason: String,
    },

    #[error("file '{path}' not found for update")]
    MissingFile { path: String },

    #[error("failed to locate context '{context}' in '{path}'")]
    ContextNotFound { path: String, context: String },

    #[error("failed to locate expected lines in '{path}':\n{snippet}")]
    SegmentNotFound { path: String, snippet: String },

    #[error("I/O error while {action} '{path}': {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to generate temporary path for '{path}': {source}")]
    TempPath {
        path: PathBuf,
        #[source]
        source: std::time::SystemTimeError,
    },

    #[error("failed to rollback patch after error ({original}): {rollback}")]
    Recovery {
        original: Box<PatchError>,
        #[source]
        rollback: Box<PatchError>,
    },
}
