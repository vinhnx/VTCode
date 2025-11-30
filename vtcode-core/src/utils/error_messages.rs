//! Consolidated error messages to eliminate duplication across codebase
//!
//! This module contains constants for common error messages used throughout vtcode.
//! Using constants instead of string literals reduces binary size and memory overhead.

// File operation errors
pub const ERR_READ_FILE: &str = "failed to read file";
pub const ERR_WRITE_FILE: &str = "failed to write file";
pub const ERR_READ_DIR: &str = "failed to read directory";
pub const ERR_CREATE_DIR: &str = "failed to create directory";
pub const ERR_REMOVE_FILE: &str = "failed to remove file";
pub const ERR_REMOVE_DIR: &str = "failed to remove directory";
pub const ERR_READ_DIR_ENTRY: &str = "failed to read directory entry";
pub const ERR_GET_FILE_TYPE: &str = "failed to read file type";
pub const ERR_GET_METADATA: &str = "failed to read file metadata";
pub const ERR_CANONICALIZE_PATH: &str = "failed to canonicalize path";
pub const ERR_READ_SYMLINK: &str = "failed to read symlink";

// Session/State errors
pub const ERR_CREATE_SESSION_DIR: &str = "failed to create session directory";
pub const ERR_READ_SESSION: &str = "failed to read session";
pub const ERR_WRITE_SESSION: &str = "failed to write session";
pub const ERR_DELETE_SESSION: &str = "failed to delete session";
pub const ERR_ARCHIVE_SESSION: &str = "failed to archive session";

// Skill/Tool errors
pub const ERR_CREATE_SKILLS_DIR: &str = "failed to create skills directory";
pub const ERR_CREATE_SKILL_DIR: &str = "failed to create skill directory";
pub const ERR_READ_SKILL_CODE: &str = "failed to read skill code";
pub const ERR_WRITE_SKILL_CODE: &str = "failed to write skill code";
pub const ERR_READ_SKILL_METADATA: &str = "failed to read skill metadata";
pub const ERR_WRITE_SKILL_METADATA: &str = "failed to write skill metadata";
pub const ERR_PARSE_SKILL_METADATA: &str = "failed to parse skill metadata";
pub const ERR_WRITE_SKILL_DOCS: &str = "failed to write skill documentation";
pub const ERR_DELETE_SKILL: &str = "failed to delete skill";
pub const ERR_READ_SKILLS_DIR: &str = "failed to read skills directory";

// Audit/Logging errors
pub const ERR_CREATE_AUDIT_DIR: &str = "Failed to create audit directory";
pub const ERR_WRITE_AUDIT_LOG: &str = "failed to write audit log";

// Checkpoint/Snapshot errors
pub const ERR_CREATE_CHECKPOINT_DIR: &str = "failed to create checkpoint directory";
pub const ERR_WRITE_CHECKPOINT: &str = "failed to write checkpoint";
pub const ERR_READ_CHECKPOINT: &str = "failed to read checkpoint";

// Policy errors
pub const ERR_CREATE_POLICY_DIR: &str = "Failed to create directory for tool policy config";
pub const ERR_CREATE_WORKSPACE_POLICY_DIR: &str = "Failed to create workspace policy directory";

// Serialization errors
pub const ERR_SERIALIZE_METADATA: &str = "failed to serialize skill metadata";
pub const ERR_SERIALIZE_STATE: &str = "failed to serialize state";
pub const ERR_DESERIALIZE: &str = "failed to deserialize data";

// IPC/SDK errors
pub const ERR_CREATE_IPC_DIR: &str = "failed to create IPC directory";
pub const ERR_READ_REQUEST_FILE: &str = "failed to read request file";
pub const ERR_READ_REQUEST_JSON: &str = "failed to read request JSON";
pub const ERR_PARSE_REQUEST_JSON: &str = "failed to parse request JSON";
pub const ERR_PARSE_ARGS: &str = "failed to parse tokenized args";
pub const ERR_PARSE_RESULT: &str = "failed to parse de-tokenized result";

/// Helper macro for file operation errors with context
/// Usage: file_err!("path", "read") -> "failed to read path"
#[macro_export]
macro_rules! file_err {
    ($path:expr, read) => {
        format!("failed to read {}", $path)
    };
    ($path:expr, write) => {
        format!("failed to write {}", $path)
    };
    ($path:expr, delete) => {
        format!("failed to delete {}", $path)
    };
    ($path:expr, create) => {
        format!("failed to create {}", $path)
    };
}

/// Helper macro for context errors
/// Usage: ctx_err!(operation, context) -> "operation context"
#[macro_export]
macro_rules! ctx_err {
    ($op:expr, $ctx:expr) => {
        format!("{}: {}", $op, $ctx)
    };
}
