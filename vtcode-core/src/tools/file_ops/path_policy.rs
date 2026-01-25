use super::FileOpsTool;
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

impl FileOpsTool {
    pub(super) fn canonical_workspace_root(&self) -> &PathBuf {
        &self.canonical_workspace_root
    }

    pub(super) fn workspace_relative_display(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.workspace_root) {
            relative.to_string_lossy().into_owned()
        } else if let Ok(relative) = path.strip_prefix(self.canonical_workspace_root()) {
            relative.to_string_lossy().into_owned()
        } else {
            path.to_string_lossy().into_owned()
        }
    }

    pub(super) fn absolute_candidate(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        }
    }

    pub(super) async fn normalize_and_validate_user_path(&self, path: &str) -> Result<PathBuf> {
        self.normalize_and_validate_candidate(Path::new(path), path)
            .await
    }

    pub(super) async fn normalize_and_validate_candidate(
        &self,
        path: &Path,
        original_display: &str,
    ) -> Result<PathBuf> {
        use crate::utils::path::normalize_path;
        let absolute = self.absolute_candidate(path);
        let normalized = normalize_path(&absolute);
        let normalized_root = normalize_path(&self.workspace_root);

        if !normalized.starts_with(&normalized_root) {
            return Err(anyhow!(
                "Error: Path '{}' resolves outside the workspace.",
                original_display
            ));
        }

        let canonical = self.canonicalize_allow_missing(&normalized).await?;
        if !canonical.starts_with(self.canonical_workspace_root()) {
            return Err(anyhow!(
                "Error: Path '{}' resolves outside the workspace.",
                original_display
            ));
        }

        Ok(canonical)
    }

    pub(super) async fn canonicalize_allow_missing(&self, normalized: &Path) -> Result<PathBuf> {
        if tokio::fs::try_exists(normalized).await? {
            return tokio::fs::canonicalize(normalized).await.with_context(|| {
                format!(
                    "Failed to resolve canonical path for '{}'.",
                    normalized.display()
                )
            });
        }

        let mut current = normalized.to_path_buf();
        while let Some(parent) = current.parent() {
            if tokio::fs::try_exists(parent).await? {
                let canonical_parent =
                    tokio::fs::canonicalize(parent).await.with_context(|| {
                        format!(
                            "Failed to resolve canonical path for '{}'.",
                            parent.display()
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

    pub(super) fn resolve_file_path(&self, path: &str) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();

        // Try exact path first
        paths.push(self.workspace_root.join(path));

        // If it's just a filename, try common directories that exist in most projects
        if !path.contains('/') && !path.contains('\\') {
            // Generic source directories found in most projects
            paths.push(self.workspace_root.join("src").join(path));
            paths.push(self.workspace_root.join("lib").join(path));
            paths.push(self.workspace_root.join("bin").join(path));
            paths.push(self.workspace_root.join("app").join(path));
            paths.push(self.workspace_root.join("source").join(path));
            paths.push(self.workspace_root.join("sources").join(path));
            paths.push(self.workspace_root.join("include").join(path));
            paths.push(self.workspace_root.join("docs").join(path));
            paths.push(self.workspace_root.join("doc").join(path));
            paths.push(self.workspace_root.join("examples").join(path));
            paths.push(self.workspace_root.join("example").join(path));
            paths.push(self.workspace_root.join("tests").join(path));
            paths.push(self.workspace_root.join("test").join(path));
        }

        // Try case-insensitive variants for filenames
        if !path.contains('/')
            && !path.contains('\\')
            && let Ok(entries) = std::fs::read_dir(&self.workspace_root)
        {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string()
                    && name.to_lowercase() == path.to_lowercase()
                {
                    paths.push(entry.path());
                }
            }
        }

        Ok(paths)
    }

    /// Public helper to normalize and validate a user-provided path against the workspace root.
    pub async fn normalize_user_path(&self, path: &str) -> Result<PathBuf> {
        self.normalize_and_validate_user_path(path).await
    }
}
