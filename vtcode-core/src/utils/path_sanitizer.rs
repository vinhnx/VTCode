//! Utility for safely handling file system paths.
//!
//! The VT Code agent must never escape the workspace directory when performing
//! file operations.  This module provides a single helper `secure_path` that
//! canonicalises a user‑provided path and verifies that it is a descendant of
//! the current working directory.  All callers should use this function before
//! performing any read/write/delete operation.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// Return a canonicalised absolute path that is guaranteed to reside inside the
/// provided `workspace_root`.  If the path is outside the workspace an error is
/// returned.
///
/// # Arguments
/// * `workspace_root` – Absolute path to the VT Code workspace (usually the
///   current working directory).
/// * `user_path` – Path supplied by the user or a tool argument. May be
///   relative or absolute.
///
/// The function performs three steps:
/// 1. Resolve `user_path` against `workspace_root` if it is relative.
/// 2. Canonicalise the resulting path to resolve symlinks (`std::fs::canonicalize`).
/// 3. Ensure the canonical path starts with the workspace root.
///
/// This mirrors the validation performed elsewhere in the codebase but centralises
/// it for consistency and easier future changes (e.g., sandboxing).
pub fn secure_path(workspace_root: &Path, user_path: &Path) -> Result<PathBuf> {
    // Resolve relative paths against the workspace root.
    let joined = if user_path.is_absolute() {
        user_path.to_path_buf()
    } else {
        workspace_root.join(user_path)
    };

    // Canonicalise to eliminate `..` components and resolve symlinks.
    let canonical = std::fs::canonicalize(&joined)
        .with_context(|| format!("Failed to canonicalize path {}", joined.display()))?;

    // Ensure the canonical path is within the workspace.
    let workspace_canonical = std::fs::canonicalize(workspace_root)
        .with_context(|| format!("Failed to canonicalize workspace root {}", workspace_root.display()))?;
    if !canonical.starts_with(&workspace_canonical) {
        return Err(anyhow!(
            "Path {} escapes workspace root {}",
            canonical.display(),
            workspace_canonical.display()
        ));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_secure_path_inside_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let file_path = workspace.join("sub/file.txt");
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        File::create(&file_path).unwrap();
        let result = secure_path(workspace, Path::new("sub/file.txt")).unwrap();
        assert_eq!(result, fs::canonicalize(&file_path).unwrap());
    }

    #[test]
    fn test_secure_path_outside_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let outside = env::temp_dir().join("outside.txt");
        // Ensure the file exists so canonicalize succeeds.
        File::create(&outside).unwrap();
        let err = secure_path(workspace, &outside).unwrap_err();
        assert!(err.to_string().contains("escapes workspace"));
    }
}

