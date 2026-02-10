use anyhow::{Result, anyhow, bail};
use std::path::{Path, PathBuf};

/// Consolidated path validation combining safety checks, normalization, and workspace bounds.
///
/// Steps:
/// 1. Shell character + system path safety (from validation/paths.rs)
/// 2. Normalize + workspace bounds check (from traits.rs)
/// 3. Canonical resolve + re-check for symlink escapes (from path_policy.rs)
pub async fn validate_and_resolve_path(workspace_root: &Path, path_str: &str) -> Result<PathBuf> {
    // Step 1: Safety checks (dangerous chars, system paths, traversal)
    super::paths::validate_path_safety(path_str)?;

    // Step 2: Normalize and check workspace bounds
    let path = Path::new(path_str);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let normalized = crate::utils::path::normalize_path(&absolute);
    let normalized_root = crate::utils::path::normalize_path(workspace_root);

    if !normalized.starts_with(&normalized_root) {
        bail!(
            "Path '{}' resolves outside the workspace boundary",
            path_str
        );
    }

    // Step 3: Canonical resolve + re-check (catches symlink escapes)
    let canonical = canonicalize_allow_missing(&normalized).await?;
    let canonical_root = if tokio::fs::try_exists(workspace_root).await.unwrap_or(false) {
        tokio::fs::canonicalize(workspace_root)
            .await
            .unwrap_or_else(|_| normalized_root.clone())
    } else {
        normalized_root
    };

    if !canonical.starts_with(&canonical_root) {
        bail!(
            "Path '{}' resolves outside the workspace boundary (after symlink resolution)",
            path_str
        );
    }

    Ok(canonical)
}

/// Canonicalize a path, walking up to find the nearest existing ancestor for new files.
async fn canonicalize_allow_missing(normalized: &Path) -> Result<PathBuf> {
    if tokio::fs::try_exists(normalized).await.unwrap_or(false) {
        return tokio::fs::canonicalize(normalized).await.map_err(|e| {
            anyhow!(
                "Failed to resolve canonical path for '{}': {}",
                normalized.display(),
                e
            )
        });
    }

    let mut current = normalized.to_path_buf();
    while let Some(parent) = current.parent() {
        if tokio::fs::try_exists(parent).await.unwrap_or(false) {
            let canonical_parent = tokio::fs::canonicalize(parent).await.map_err(|e| {
                anyhow!(
                    "Failed to resolve canonical path for '{}': {}",
                    parent.display(),
                    e
                )
            })?;
            let remainder = normalized
                .strip_prefix(parent)
                .unwrap_or_else(|_| Path::new(""));
            return if remainder.as_os_str().is_empty() {
                Ok(canonical_parent)
            } else {
                Ok(canonical_parent.join(remainder))
            };
        }
        current = parent.to_path_buf();
    }

    Ok(normalized.to_path_buf())
}
