use anyhow::{Result, bail};
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
    let canonical = crate::utils::path::canonicalize_allow_missing(&normalized).await?;
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
