//! Shared constants for tool operations to eliminate duplication
//!
//! This module provides common constants used across multiple tool implementations
//! to reduce code duplication and ensure consistency.
//!
//! Core constants are defined in `vtcode-commons::tool_types` and re-exported here
//! for backward compatibility. Additional vtcode-specific constants are defined below.

// Re-export core constants from vtcode-commons::tool_types for backward compatibility
pub use vtcode_commons::tool_types::{
    DEFAULT_HASHMAP_CAPACITY, DEFAULT_STRING_CAPACITY, DEFAULT_VEC_CAPACITY,
    ERROR_DETECTION_PATTERNS, MAX_CONTEXT_LINES, MAX_FILE_SIZE_FOR_PROCESSING,
    MAX_LIST_ITEMS_SUMMARY, MAX_OUTPUT_TOKENS, MAX_SEARCH_RESULTS, NETWORK_ERROR_PATTERNS,
    OVERFLOW_INDICATOR_PREFIX, OVERFLOW_INDICATOR_SUFFIX, empty_object_schema,
};

/// Memory and resource error patterns
pub const RESOURCE_ERROR_PATTERNS: &[&str] = &[
    "memory",
    "oom",
    "out of",
    "resource",
    "too large",
    "disk full",
    "quota exceeded",
];

/// Git-specific error patterns
pub const GIT_ERROR_PATTERNS: &[&str] = &[
    "git error",
    "git fatal",
    "merge conflict",
    "rebase conflict",
    "detached HEAD",
];

/// Command execution error patterns
pub const COMMAND_ERROR_PATTERNS: &[&str] = &[
    "command not found",
    "command failed",
    "exit code",
    "permission denied",
    "no such file or directory",
];

/// File system error patterns
pub const FILESYSTEM_ERROR_PATTERNS: &[&str] = &[
    "file not found",
    "no such file",
    "directory not found",
    "permission denied",
    "read-only file system",
    "disk quota exceeded",
];
