//! Centralized exclusion constants and helpers for file traversal.
//!
//! All directory walkers, grep invocations, and file-operation tools should
//! reference these constants instead of maintaining their own skip lists.

/// Name of the workspace-local ignore file that mirrors `.gitignore` semantics
/// but only affects VT Code's own file operations.
///
/// Registered as a custom ignore filename on every traversal walker so the
/// same patterns prune the file picker, grep/list tools, and the indexer.
pub const VTCODE_IGNORE_FILE: &str = ".vtcodegitignore";

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
    ".git-credentials",
    ".netrc",
    ".npmrc",
    ".pypirc",
    "credentials",
    "credentials.json",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "id_rsa",
];

/// Glob patterns passed to ripgrep (or other search back-ends) to exclude
/// noisy vendor/build directories from results.
pub const DEFAULT_IGNORE_GLOBS: &[&str] = &[
    "**/.git/**",
    "**/node_modules/**",
    "**/target/**",
    "**/.cursor/**",
    "**/dist/**",
    "**/.next/**",
    "**/vendor/**",
    "**/.vtcode/**",
    "**/.vscode/**",
    "**/.idea/**",
];

/// Returns `true` if `name` matches any entry in [`SENSITIVE_FILES`] or
/// starts with `.env.` (catches all dotenv variants). Matching is
/// case-insensitive because macOS and Windows commonly use case-insensitive
/// filesystems.
pub fn is_sensitive_file(name: &str) -> bool {
    SENSITIVE_FILES.iter().any(|sensitive| name.eq_ignore_ascii_case(sensitive))
        || name.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case(".env."))
}

#[cfg(test)]
mod tests {
    use super::is_sensitive_file;

    #[test]
    fn sensitive_file_matching_is_case_insensitive() {
        assert!(is_sensitive_file(".ENV"));
        assert!(is_sensitive_file(".Env.Local"));
        assert!(is_sensitive_file(".NPMRC"));
        assert!(!is_sensitive_file(".environment"));
    }

    #[test]
    fn ssh_private_key_basenames_are_sensitive() {
        assert!(is_sensitive_file("id_dsa"));
        assert!(is_sensitive_file("id_ecdsa"));
        assert!(is_sensitive_file("ID_ECDSA"));
        assert!(!is_sensitive_file("id_ecdsa.pub"));
    }
}
