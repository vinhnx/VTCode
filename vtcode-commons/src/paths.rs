use anyhow::{Context, Result, anyhow, bail};
use std::path::{Component, Path, PathBuf};
use tracing::warn;

/// Normalize a path by resolving `.` and `..` components lexically.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

/// Canonicalize a path with fallback to the original path if canonicalization fails.
pub fn canonicalize_workspace(workspace_root: &Path) -> PathBuf {
    std::fs::canonicalize(workspace_root).unwrap_or_else(|error| {
        warn!(
            path = %workspace_root.display(),
            %error,
            "Failed to canonicalize workspace root; falling back to provided path"
        );
        workspace_root.to_path_buf()
    })
}

/// Resolve a path relative to a workspace root and ensure it stays within it.
pub fn resolve_workspace_path(workspace_root: &Path, user_path: &Path) -> Result<PathBuf> {
    let candidate = if user_path.is_absolute() {
        user_path.to_path_buf()
    } else {
        workspace_root.join(user_path)
    };

    let canonical = std::fs::canonicalize(&candidate)
        .with_context(|| format!("Failed to canonicalize path {}", candidate.display()))?;

    let workspace_canonical = std::fs::canonicalize(workspace_root).with_context(|| {
        format!(
            "Failed to canonicalize workspace root {}",
            workspace_root.display()
        )
    })?;

    if !canonical.starts_with(&workspace_canonical) {
        return Err(anyhow!(
            "Path {} escapes workspace root {}",
            canonical.display(),
            workspace_canonical.display()
        ));
    }

    Ok(canonical)
}

/// Return a canonicalised absolute path that is guaranteed to reside inside the
/// provided `workspace_root`.  If the path is outside the workspace an error is
/// returned.
pub fn secure_path(workspace_root: &Path, user_path: &Path) -> Result<PathBuf> {
    // Resolve relative paths against the workspace root.
    resolve_workspace_path(workspace_root, user_path)
}

/// Ensure a candidate path is inside the workspace root after lexical
/// normalization.
///
/// Returns the normalized candidate path on success.
pub fn ensure_path_within_workspace(candidate: &Path, workspace_root: &Path) -> Result<PathBuf> {
    let normalized_candidate = normalize_path(candidate);
    let normalized_workspace = normalize_path(workspace_root);

    if !normalized_candidate.starts_with(&normalized_workspace) {
        bail!(
            "Path '{}' escapes workspace '{}'",
            candidate.display(),
            workspace_root.display()
        );
    }

    Ok(normalized_candidate)
}

/// Normalize identifiers to ASCII alphanumerics with lowercase output.
pub fn normalize_ascii_identifier(value: &str) -> String {
    let mut normalized = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        }
    }
    normalized
}

/// Check if a path string is a safe relative path (no traversal, no absolute).
pub fn is_safe_relative_path(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return false;
    }

    // Check for path traversal attempts
    if path.contains("..") {
        return false;
    }

    // Block absolute paths for security
    if path.starts_with('/') || path.contains(':') {
        return false;
    }

    true
}

/// Validates that a path is safe to use.
/// Preventing traversal, absolute system paths, and dangerous characters.
///
/// Optimization: Uses early returns and byte-level checks for common patterns
pub fn validate_path_safety(path: &str) -> Result<()> {
    // Optimization: Fast path for empty or very short paths
    if path.is_empty() {
        return Ok(());
    }

    // Reject path traversal attempts
    // Optimization: Use contains on bytes for simple patterns
    if path.contains("..") {
        bail!("Path traversal attempt detected ('..')");
    }

    // Additional traversal patterns
    if path.contains("~/../") || path.contains("/.../") {
        bail!("Advanced path traversal detected");
    }

    // Optimization: Only check Unix critical paths if path starts with '/'
    if path.starts_with('/') {
        // Reject absolute paths outside workspace
        // Note: We can't strictly block all absolute paths as the agent might need to access
        // explicitly allowed directories, but we can block obvious system critical paths.
        static UNIX_CRITICAL: &[&str] = &[
            "/etc", "/usr", "/bin", "/sbin", "/var", "/boot", "/root", "/dev",
        ];
        for prefix in UNIX_CRITICAL {
            let is_var_temp_exception = *prefix == "/var"
                && (path.starts_with("/var/folders/")
                    || path == "/var/folders"
                    || path.starts_with("/var/tmp/")
                    || path == "/var/tmp");

            if !is_var_temp_exception && matches_critical_prefix(path, prefix) {
                bail!("Access to system directory denied: {}", prefix);
            }
        }
    }

    // Windows critical paths
    #[cfg(windows)]
    {
        let path_lower = path.to_lowercase();
        static WIN_CRITICAL: &[&str] = &["c:\\windows", "c:\\program files", "c:\\system32"];
        for prefix in WIN_CRITICAL {
            if path_lower.starts_with(prefix) {
                bail!("Access to Windows system directory denied");
            }
        }
    }

    // Reject dangerous shell characters in paths (including null byte)
    // Optimization: Check bytes directly for faster character detection
    static DANGEROUS_CHARS: &[u8] = b"$`|;&\n\r><\0";
    for &c in path.as_bytes() {
        if DANGEROUS_CHARS.contains(&c) {
            bail!("Path contains dangerous shell characters");
        }
    }

    Ok(())
}

fn matches_critical_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Extract the filename from a path, with fallback to the full path.
pub fn file_name_from_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Canonicalize a path, walking up to find the nearest existing ancestor for new files.
///
/// This function handles paths to files that may not yet exist by finding the
/// nearest existing parent directory, canonicalizing that, and then appending
/// the remaining path components.
///
/// # Safety
/// This function is critical for security. It prevents symlink escapes by:
/// 1. Finding the nearest existing ancestor directory
/// 2. Canonicalizing that directory (resolves symlinks)
/// 3. Appending the remaining path components
///
/// # Arguments
/// * `normalized` - A normalized path (output from `normalize_path`)
///
/// # Returns
/// The canonical path, or the normalized path if no parent exists
pub async fn canonicalize_allow_missing(normalized: &Path) -> Result<PathBuf> {
    // If the path exists, canonicalize it directly
    if tokio::fs::try_exists(normalized).await.unwrap_or(false) {
        return tokio::fs::canonicalize(normalized).await.map_err(|e| {
            anyhow!(
                "Failed to resolve canonical path for '{}': {}",
                normalized.display(),
                e
            )
        });
    }

    // Walk up the directory tree to find the nearest existing ancestor
    let mut current = normalized.to_path_buf();
    while let Some(parent) = current.parent() {
        if tokio::fs::try_exists(parent).await.unwrap_or(false) {
            // Canonicalize the existing parent
            let canonical_parent = tokio::fs::canonicalize(parent).await.map_err(|e| {
                anyhow!(
                    "Failed to resolve canonical path for '{}': {}",
                    parent.display(),
                    e
                )
            })?;

            // Get the remaining path components
            let remainder = normalized
                .strip_prefix(parent)
                .unwrap_or_else(|_| Path::new(""));

            // Return the canonical parent + remaining components
            return if remainder.as_os_str().is_empty() {
                Ok(canonical_parent)
            } else {
                Ok(canonical_parent.join(remainder))
            };
        }
        current = parent.to_path_buf();
    }

    // No existing parent found, return normalized path as-is
    Ok(normalized.to_path_buf())
}

/// Provides the root directories an application uses to store data.
pub trait WorkspacePaths: Send + Sync {
    /// Absolute path to the application's workspace root.
    fn workspace_root(&self) -> &Path;

    /// Returns the directory where configuration files should be stored.
    fn config_dir(&self) -> PathBuf;

    /// Returns an optional cache directory for transient data.
    fn cache_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Returns an optional directory for telemetry or log artifacts.
    fn telemetry_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Determine the [`PathScope`] for a given path based on workspace directories.
    ///
    /// Returns the most specific scope matching the path:
    /// - `Workspace` if under `workspace_root()`
    /// - `Config` if under `config_dir()`
    /// - `Cache` if under `cache_dir()`
    /// - `Telemetry` if under `telemetry_dir()`
    /// - Falls back to `Cache` if no match
    fn scope_for_path(&self, path: &Path) -> PathScope {
        if path.starts_with(self.workspace_root()) {
            return PathScope::Workspace;
        }

        let config_dir = self.config_dir();
        if path.starts_with(&config_dir) {
            return PathScope::Config;
        }

        if let Some(cache_dir) = self.cache_dir()
            && path.starts_with(&cache_dir)
        {
            return PathScope::Cache;
        }

        if let Some(telemetry_dir) = self.telemetry_dir()
            && path.starts_with(&telemetry_dir)
        {
            return PathScope::Telemetry;
        }

        PathScope::Cache
    }
}

/// Helper trait that adds path resolution helpers on top of [`WorkspacePaths`].
pub trait PathResolver: WorkspacePaths {
    /// Resolve a path relative to the workspace root.
    fn resolve<P>(&self, relative: P) -> PathBuf
    where
        P: AsRef<Path>,
    {
        self.workspace_root().join(relative)
    }

    /// Resolve a path within the configuration directory.
    fn resolve_config<P>(&self, relative: P) -> PathBuf
    where
        P: AsRef<Path>,
    {
        self.config_dir().join(relative)
    }
}

impl<T> PathResolver for T where T: WorkspacePaths + ?Sized {}

/// Enumeration describing the conceptual scope of a file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathScope {
    Workspace,
    Config,
    Cache,
    Telemetry,
}

impl PathScope {
    /// Returns a human-readable description used in error messages.
    pub fn description(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Config => "configuration",
            Self::Cache => "cache",
            Self::Telemetry => "telemetry",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct StaticPaths {
        root: PathBuf,
        config: PathBuf,
    }

    impl WorkspacePaths for StaticPaths {
        fn workspace_root(&self) -> &Path {
            &self.root
        }

        fn config_dir(&self) -> PathBuf {
            self.config.clone()
        }

        fn cache_dir(&self) -> Option<PathBuf> {
            Some(self.root.join("cache"))
        }
    }

    #[test]
    fn resolves_relative_paths() {
        let paths = StaticPaths {
            root: PathBuf::from("/tmp/project"),
            config: PathBuf::from("/tmp/project/config"),
        };

        assert_eq!(
            PathResolver::resolve(&paths, "subdir/file.txt"),
            PathBuf::from("/tmp/project/subdir/file.txt")
        );
        assert_eq!(
            PathResolver::resolve_config(&paths, "settings.toml"),
            PathBuf::from("/tmp/project/config/settings.toml")
        );
        assert_eq!(paths.cache_dir(), Some(PathBuf::from("/tmp/project/cache")));
    }

    #[test]
    fn ensures_path_within_workspace_accepts_nested_path() {
        let workspace = Path::new("/tmp/project");
        let candidate = Path::new("/tmp/project/src/../src/lib.rs");
        let normalized = ensure_path_within_workspace(candidate, workspace).unwrap();
        assert_eq!(normalized, PathBuf::from("/tmp/project/src/lib.rs"));
    }

    #[test]
    fn ensures_path_within_workspace_rejects_escape() {
        let workspace = Path::new("/tmp/project");
        let candidate = Path::new("/tmp/project/../../etc/passwd");
        assert!(ensure_path_within_workspace(candidate, workspace).is_err());
    }

    #[tokio::test]
    async fn test_canonicalize_existing_file() {
        // Create a temporary directory and file
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("vtcode_test_existing.txt");
        tokio::fs::write(&test_file, b"test").await.unwrap();

        let canonical = canonicalize_allow_missing(&test_file).await.unwrap();

        // Should get the canonical path
        assert!(canonical.is_absolute());
        assert!(canonical.exists());

        // Cleanup
        tokio::fs::remove_file(&test_file).await.ok();
    }

    #[tokio::test]
    async fn test_canonicalize_missing_file() {
        // Use a path that doesn't exist but has an existing parent
        let temp_dir = std::env::temp_dir();
        let missing_file = temp_dir.join("vtcode_test_missing_dir/missing_file.txt");

        let canonical = canonicalize_allow_missing(&missing_file).await.unwrap();

        // Should get canonical parent + missing components
        assert!(canonical.is_absolute());
        assert!(canonical.to_string_lossy().contains("missing_file.txt"));
    }

    #[tokio::test]
    async fn test_canonicalize_deeply_missing_path() {
        // Use a path with multiple missing parent directories
        let temp_dir = std::env::temp_dir();
        let deep_missing = temp_dir.join("vtcode_test_a/b/c/d/file.txt");

        let canonical = canonicalize_allow_missing(&deep_missing).await.unwrap();

        // Should get canonical temp_dir + missing components
        assert!(canonical.is_absolute());
        assert!(canonical.to_string_lossy().contains("vtcode_test_a"));
    }

    #[tokio::test]
    async fn test_canonicalize_missing_file_with_existing_parent() {
        // Create a parent directory
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("vtcode_test_parent");
        tokio::fs::create_dir_all(&test_dir).await.unwrap();

        let missing_file = test_dir.join("missing.txt");
        let canonical = canonicalize_allow_missing(&missing_file).await.unwrap();

        // Should get canonical parent + missing filename
        assert!(canonical.is_absolute());
        assert!(canonical.to_string_lossy().ends_with("missing.txt"));

        // Cleanup
        tokio::fs::remove_dir(&test_dir).await.ok();
    }
}
