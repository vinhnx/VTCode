//! Shared constants for tool operations to eliminate duplication
//!
//! This module provides common constants used across multiple tool implementations
//! to reduce code duplication and ensure consistency.

/// Standard error patterns used for error detection across tools
/// These patterns are used in get_errors, grep operations, and error analysis
pub const ERROR_DETECTION_PATTERNS: &[&str] = &[
    "error",
    "failed",
    "exception",
    "permission denied",
    "not found",
    "no such file",
    "cannot",
    "could not",
    "panic",
    "crash",
    "unhandled",
    "fatal",
    "timeout",
    "connection refused",
    "access denied",
    "stack trace",
    "traceback",
    "abort",
    "terminate",
];

/// Network-related error patterns for more specific error detection
pub const NETWORK_ERROR_PATTERNS: &[&str] = &[
    "connection",
    "timeout",
    "network",
    "http",
    "ssl",
    "tls",
    "dns",
    "proxy",
];

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

/// Default capacity hints for common collections
pub const DEFAULT_VEC_CAPACITY: usize = 32;
pub const DEFAULT_HASHMAP_CAPACITY: usize = 16;
pub const DEFAULT_STRING_CAPACITY: usize = 256;

/// Context optimization constants following AGENTS.md guidelines
pub const MAX_SEARCH_RESULTS: usize = 5;
pub const MAX_LIST_ITEMS_SUMMARY: usize = 5;
pub const OVERFLOW_INDICATOR_PREFIX: &str = "[+]";
pub const OVERFLOW_INDICATOR_SUFFIX: &str = "more items]";

/// Common tool operation limits
pub const MAX_FILE_SIZE_FOR_PROCESSING: usize = 100 * 1024 * 1024; // 100MB
pub const MAX_CONTEXT_LINES: usize = 20;
pub const MAX_OUTPUT_TOKENS: usize = 4000;
