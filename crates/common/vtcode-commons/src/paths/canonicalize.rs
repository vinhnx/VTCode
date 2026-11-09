use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use tracing::warn;

/// Canonicalize a filesystem path without the Windows `\\?\` verbatim prefix.
///
/// `std::fs::canonicalize` returns verbatim (`\\?\`) paths on Windows, which
/// break path comparisons, round-tripping through user config, and some APIs
/// that reject verbatim paths. `dunce::canonicalize` strips the prefix when it
/// is safe to do so and is a perfect drop-in on non-Windows platforms.
///
/// This is the single canonical entry point for path canonicalization in the
/// workspace. All call sites should use this (or `canonicalize_workspace`) rather
/// than `std::fs::canonicalize` / `Path::canonicalize`, which are banned by
/// `clippy.toml`'s `disallowed-methods`.
pub fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Async canonicalize via `dunce`, using `spawn_blocking` to avoid blocking the
/// runtime. Equivalent to `tokio::fs::canonicalize` but without the Windows
/// `\\?\` verbatim prefix. Use this in async contexts instead of the sync
/// [`canonicalize`] when the path may require I/O.
pub async fn canonicalize_async(path: impl AsRef<Path> + Send) -> std::io::Result<PathBuf> {
    let path = path.as_ref().to_path_buf();
    match tokio::task::spawn_blocking(move || dunce::canonicalize(&path)).await {
        Ok(inner) => inner,
        Err(join_error) => Err(std::io::Error::other(join_error)),
    }
}

/// Canonicalize a path with fallback to the original path if canonicalization fails.
pub fn canonicalize_workspace(workspace_root: &Path) -> PathBuf {
    canonicalize(workspace_root).unwrap_or_else(|error| {
        warn!(
            path = %workspace_root.display(),
            %error,
            "Failed to canonicalize workspace root; falling back to provided path"
        );
        workspace_root.to_path_buf()
    })
}

/// Canonicalize a path, walking up to find the nearest existing ancestor for new files.
///
/// This function handles paths to files that may not yet exist by finding the
/// nearest existing parent directory, canonicalizing that, and then appending
/// the remaining path components.
///
/// # Security
/// This function is critical for security. It prevents symlink escapes by:
/// 1. Finding the nearest existing ancestor directory
/// 2. Canonicalizing that directory (resolves symlinks)
/// 3. Appending the remaining path components
///
/// # Arguments
/// * `normalized` - A normalized path (output from `normalize_path`)
///
/// # Returns
/// The canonical path, or the normalized path if no parent exists. Existing
/// symlinks must resolve, including symlinks whose targets do not yet exist.
pub async fn canonicalize_allow_missing(normalized: &Path) -> Result<PathBuf> {
    // Inspect the link itself: try_exists follows symlinks and reports a
    // dangling link as absent, which would leave it unresolved in the result.
    let mut current = normalized.to_path_buf();
    loop {
        match tokio::fs::symlink_metadata(&current).await {
            Ok(_) => {
                let canonical_parent = canonicalize_async(&current)
                    .await
                    .map_err(|e| anyhow!("Failed to resolve canonical path for '{}': {}", current.display(), e))?;

                let remainder = normalized.strip_prefix(&current).unwrap_or_else(|_| Path::new(""));

                // Return the canonical parent + remaining components
                return if remainder.as_os_str().is_empty() {
                    Ok(canonical_parent)
                } else {
                    Ok(canonical_parent.join(remainder))
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(anyhow!("Failed to inspect path '{}': {}", current.display(), error)),
        }
        let Some(parent) = current.parent() else { break };
        current = parent.to_path_buf();
    }

    // No existing parent found, return normalized path as-is
    Ok(normalized.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        drop(tokio::fs::remove_file(&test_file).await);
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
        drop(tokio::fs::remove_dir(&test_dir).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dangling_symlink_is_not_treated_as_a_missing_component() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = workspace.path().join("link");
        std::os::unix::fs::symlink(outside.path().join("missing"), &link).unwrap();
        assert!(canonicalize_allow_missing(&link).await.is_err());
        assert!(canonicalize_allow_missing(&link.join("child")).await.is_err());
        assert!(canonicalize_allow_missing(&workspace.path().join("new/child")).await.is_ok());
        let valid_link = workspace.path().join("valid");
        std::os::unix::fs::symlink(outside.path(), &valid_link).unwrap();
        let resolved = canonicalize_allow_missing(&valid_link.join("new/child")).await.unwrap();
        assert_eq!(resolved, canonicalize_async(outside.path()).await.unwrap().join("new/child"));
    }
}
