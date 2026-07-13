//! Guard modules for tool call validation.
//!
//! Each guard is responsible for a specific type of tool call validation:
//! - `blocked_tool_guard`: Tracks blocked tool call streaks
//! - `shell_run_guard`: Tracks repeated shell command runs
//! - `read_guard`: Tracks repeated file reads (family cap + per-file-path cap)
//! - `spool_guard`: Tracks spool chunk reads
//! - `task_tracker_guard`: Tracks duplicate task tracker creates
//! - `common`: Shared utilities for all guards

pub(crate) mod blocked_tool_guard;
pub(crate) mod common;
pub(crate) mod read_guard;
pub(crate) mod shell_run_guard;
pub(crate) mod spool_guard;
pub(crate) mod task_tracker_guard;

// Re-export public items for backward compatibility
pub(crate) use blocked_tool_guard::{
    enforce_blocked_tool_call_guard, max_consecutive_blocked_tool_calls_per_turn,
};
pub(crate) use read_guard::{
    enforce_read_after_write_guard, enforce_repeated_read_only_call_guard,
};
pub(crate) use shell_run_guard::enforce_repeated_shell_run_guard;
pub(crate) use spool_guard::enforce_spool_chunk_read_guard;
pub(crate) use task_tracker_guard::enforce_duplicate_task_tracker_create_guard;
