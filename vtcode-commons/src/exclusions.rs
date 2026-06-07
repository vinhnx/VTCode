//! Centralized exclusion constants and helpers for file traversal.
//!
//! All directory walkers, grep invocations, and file-operation tools should
//! reference these constants instead of maintaining their own skip lists.

/// Directories skipped by default during workspace traversal.
///
/// This covers build artifacts, dependency stores, VCS metadata, and IDE
/// configuration directories that are almost never relevant to code search
/// or analysis.
pub const DEFAULT_EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".next",
    "vendor",
    ".cursor",
    ".vtcode",
    ".vscode",
    ".idea",
];

/// Sensitive files that must never be exposed in listings, search results,
/// or the TUI file palette.  These contain secrets, credentials, or
/// environment-specific configuration.
pub const SENSITIVE_FILES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.development",
    ".env.test",
    ".DS_Store",
];

/// Glob patterns passed to ripgrep (or other search back-ends) to exclude
/// noisy vendor/build directories from results.
pub const DEFAULT_IGNORE_GLOBS: &[&str] = &[
    "**/.git/**",
    "**/node_modules/**",
    "**/target/**",
    "**/.cursor/**",
];

/// Returns `true` if `name` matches any entry in [`SENSITIVE_FILES`] or
/// starts with `.env.` (catches all dotenv variants).
pub fn is_sensitive_file(name: &str) -> bool {
    SENSITIVE_FILES.contains(&name) || name.starts_with(".env.")
}
