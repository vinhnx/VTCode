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
