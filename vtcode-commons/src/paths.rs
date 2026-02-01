use anyhow::{anyhow, Context, Result};
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

/// Return a canonicalised absolute path that is guaranteed to reside inside the
/// provided `workspace_root`.  If the path is outside the workspace an error is
/// returned.
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
        .with_context(|| {
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

        if let Some(cache_dir) = self.cache_dir() {
            if path.starts_with(&cache_dir) {
                return PathScope::Cache;
            }
        }

        if let Some(telemetry_dir) = self.telemetry_dir() {
            if path.starts_with(&telemetry_dir) {
                return PathScope::Telemetry;
            }
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
    use std::path::PathBuf;

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
}
