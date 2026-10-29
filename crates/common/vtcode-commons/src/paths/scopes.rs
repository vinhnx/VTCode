use super::canonicalize::canonicalize_workspace;
use super::normalize::{expand_tilde, file_name_from_path, normalize_path};
use super::safety::{is_safe_relative_path, validate_path_safety};
use anyhow::Result;
use std::path::{Path, PathBuf};

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

// ============================================================================
// Extension Traits (Pattern 3: Extension Traits)
// ============================================================================

/// Extension trait that adds path normalization and safety methods to `Path`.
///
/// Delegates to the free functions in the `paths` module tree, providing a more
/// ergonomic call-site syntax:
///
/// ```rust
/// use vtcode_commons::paths::PathExt;
/// use std::path::Path;
///
/// let normalized = Path::new("/tmp/project/src/../src/lib.rs").normalize();
/// ```
pub trait PathExt {
    /// Normalize a path by resolving `.` and `..` components lexically.
    fn normalize(&self) -> PathBuf;

    /// Canonicalize with fallback to the original path if canonicalization fails.
    fn canonicalize_or_self(&self) -> PathBuf;

    /// Extract the filename from a path as a `String`, with fallback to the
    /// full path when no filename component exists.
    ///
    /// Unlike [`Path::file_name`] which returns `Option<&OsStr>`, this method
    /// always returns a `String` and falls back gracefully.
    fn file_name_str(&self) -> String;
}

impl PathExt for Path {
    fn normalize(&self) -> PathBuf {
        normalize_path(self)
    }

    fn canonicalize_or_self(&self) -> PathBuf {
        canonicalize_workspace(self)
    }

    fn file_name_str(&self) -> String {
        self.file_name()
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.to_string_lossy().into_owned())
    }
}

/// Extension trait that adds path-related methods to `str`.
///
/// Provides ergonomic access to tilde expansion and path safety checks:
///
/// ```rust
/// use vtcode_commons::paths::StrPathExt;
///
/// let expanded = "~/projects/vtcode".expand_tilde();
/// assert!(StrPathExt::is_safe_path("src/main.rs"));
/// ```
pub trait StrPathExt {
    /// Expand a leading `~` or `~/` to the user's home directory.
    fn expand_tilde(&self) -> PathBuf;

    /// Check if this path string is a safe relative path (no traversal, no absolute).
    fn is_safe_path(&self) -> bool;

    /// Validate that this path is safe to use (no traversal, no dangerous characters).
    fn validate_safety(&self) -> Result<()>;

    /// Extract the filename from this path string.
    fn file_name_str(&self) -> String;
}

impl StrPathExt for str {
    fn expand_tilde(&self) -> PathBuf {
        expand_tilde(self)
    }

    fn is_safe_path(&self) -> bool {
        is_safe_relative_path(self)
    }

    fn validate_safety(&self) -> Result<()> {
        validate_path_safety(self)
    }

    fn file_name_str(&self) -> String {
        file_name_from_path(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(PathResolver::resolve(&paths, "subdir/file.txt"), PathBuf::from("/tmp/project/subdir/file.txt"));
        assert_eq!(
            PathResolver::resolve_config(&paths, "settings.toml"),
            PathBuf::from("/tmp/project/config/settings.toml")
        );
        assert_eq!(paths.cache_dir(), Some(PathBuf::from("/tmp/project/cache")));
    }
}
