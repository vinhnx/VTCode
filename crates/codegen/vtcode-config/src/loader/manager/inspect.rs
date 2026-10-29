use super::*;

impl ConfigManager {
    /// Get the loaded configuration
    pub fn config(&self) -> &VTCodeConfig {
        &self.config
    }

    /// Get the timing metrics recorded during loading, if available.
    pub fn phase_timing(&self) -> Option<ConfigPhaseTiming> {
        self.phase_timing
    }

    /// Get the configuration file path (if loaded from file)
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Return whether a path belongs to a repository-controlled config layer.
    ///
    /// This keeps callers that persist a full config document from having to
    /// duplicate layer-origin and explicit-session checks.
    #[must_use]
    pub fn is_repository_controlled_path(&self, path: &Path) -> bool {
        if self.explicit_config_is_trusted {
            return false;
        }

        if self
            .layer_stack
            .layers()
            .iter()
            .any(|layer| layer.is_enabled() && Self::repository_source_matches_path(&layer.source, path))
        {
            return true;
        }

        self.config_path.is_none()
            && self.workspace_root.as_deref().is_some_and(|workspace| {
                Self::repository_source_matches_path(
                    &ConfigLayerSource::Workspace { file: workspace.join(&self.config_file_name) },
                    path,
                )
            })
    }

    /// Get the active workspace root for this manager.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Resolve the workspace-level config file this manager reads from, i.e.
    /// the highest enabled `Workspace` layer.
    ///
    /// With a session-explicit config file (`--config` / `VTCODE_CONFIG_PATH`)
    /// that layer is the override file itself, so config writes land where the
    /// session actually reads from. Falls back to `<workspace>/<config_file_name>`
    /// when no workspace layer is present.
    pub fn preferred_workspace_config_path(&self, workspace: &Path) -> PathBuf {
        self.layer_stack
            .layers()
            .iter()
            .rev()
            .find_map(|layer| match &layer.source {
                ConfigLayerSource::Workspace { file } if layer.is_enabled() => Some(file.clone()),
                _ => None,
            })
            .unwrap_or_else(|| workspace.join(&self.config_file_name))
    }

    /// Get the config filename used by this manager (usually `vtcode.toml`).
    pub fn config_file_name(&self) -> &str {
        &self.config_file_name
    }

    /// Get the configuration layer stack
    pub fn layer_stack(&self) -> &ConfigLayerStack {
        &self.layer_stack
    }

    /// Resolve the canonical user-level config file VT Code should write to.
    pub fn preferred_user_config_path(&self) -> Option<PathBuf> {
        self.canonical_user_config_path.clone()
    }

    /// Return every supported user-level config path for the loaded config
    /// filename, including paths that do not exist yet.
    ///
    /// Callers that monitor configuration must retain the nonexistent paths:
    /// a later `None -> Some(mtime)` transition is a real configuration change.
    pub fn user_config_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.tracked_user_config_paths.clone();

        for path in self
            .layer_stack
            .layers()
            .iter()
            .filter_map(|layer| match (&layer.source, layer.is_enabled()) {
                (ConfigLayerSource::User { file }, true) => Some(file),
                _ => None,
            })
        {
            if !paths.iter().any(|existing| existing == path) {
                paths.push(path.clone());
            }
        }

        paths
    }

    /// Return every configuration file location that can affect a workspace.
    ///
    /// The list intentionally includes files that do not exist yet. Polling
    /// callers can therefore observe file creation as well as modification or
    /// deletion. Paths retain their configured spelling so consumers that
    /// write a target can still apply the no-follow file policy at the final
    /// path component.
    pub fn watched_config_paths(workspace: &Path) -> Vec<PathBuf> {
        let provider = defaults::current_config_defaults();
        let config_file_name = provider.config_file_name().to_string();
        let workspace_paths = provider.workspace_paths_for(workspace);
        let workspace_root = workspace_paths.workspace_root().to_path_buf();
        let mut paths = Vec::new();

        let mut push_unique = |path: PathBuf| {
            if !paths.iter().any(|existing| existing == &path) {
                paths.push(path);
            }
        };

        if let Some(explicit_path) = session_override::explicit_config_path() {
            push_unique(explicit_path);
        }
        if let Ok(system_paths) = provider.system_config_paths(&config_file_name) {
            for path in system_paths {
                push_unique(path);
            }
        }
        for path in provider.home_config_paths(&config_file_name) {
            push_unique(path);
        }
        if let Ok(Some(canonical_path)) = provider.canonical_user_config_path(&config_file_name) {
            push_unique(canonical_path);
        }

        if let Some(project_name) = Self::current_project_name(&workspace_root) {
            push_unique(
                workspace_paths
                    .config_dir()
                    .join("projects")
                    .join(project_name)
                    .join("config")
                    .join(&config_file_name),
            );
        }

        push_unique(workspace_paths.config_dir().join(&config_file_name));
        push_unique(workspace_root.join(&config_file_name));
        push_unique(workspace_root.join(".vtcode").join("theme.toml"));
        paths
    }

    pub(super) fn repository_config_paths(workspace: &Path) -> Vec<PathBuf> {
        let provider = defaults::current_config_defaults();
        let config_file_name = provider.config_file_name().to_string();
        let workspace_paths = provider.workspace_paths_for(workspace);
        let workspace_root = canonicalize_workspace_root(workspace_paths.workspace_root());
        let config_dir = workspace_paths.config_dir();
        let mut paths = Vec::with_capacity(3);

        let mut push_unique = |path: PathBuf| {
            if !paths.iter().any(|existing| existing == &path) {
                paths.push(path);
            }
        };

        if let Some(project_path) = Self::project_config_path(&config_dir, &workspace_root, &config_file_name) {
            push_unique(project_path);
        }
        push_unique(config_dir.join(&config_file_name));
        push_unique(workspace_root.join(&config_file_name));
        paths
    }

    pub(super) fn repository_source_matches_path(source: &ConfigLayerSource, path: &Path) -> bool {
        let Some(source_file) = (match source {
            ConfigLayerSource::Project { file } | ConfigLayerSource::Workspace { file } => Some(file),
            ConfigLayerSource::System { .. } | ConfigLayerSource::User { .. } | ConfigLayerSource::Runtime => None,
        }) else {
            return false;
        };

        canonicalize_workspace_root(source_file) == canonicalize_workspace_root(path)
    }

    /// Get the effective TOML configuration
    pub fn effective_config(&self) -> toml::Value {
        self.layer_stack.effective_config()
    }

    /// Return whether any enabled layer explicitly sets a top-level key.
    ///
    /// Borrow-only scan that avoids a full TOML merge for single-key presence
    /// checks on the startup hot path (e.g. `default_primary_agent`).
    pub fn has_explicit_top_level_key(&self, key: &str) -> bool {
        self.layer_stack
            .layers()
            .iter()
            .filter(|layer| layer.is_enabled())
            .any(|layer| layer.config.as_table().is_some_and(|table| table.contains_key(key)))
    }

    /// Get session duration from agent config
    pub fn session_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60 * 60) // Default 1 hour
    }
}
