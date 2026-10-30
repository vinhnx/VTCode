mod cache;
mod inspect;
mod migrate;
mod persistence;
mod repository;

use cache::{
    RepositoryProviderSecurityViolation, canonicalize_workspace_root, push_unique_layer_source,
    should_skip_optional_global_error,
};
#[cfg(not(test))]
use cache::{cache_get, cache_insert, cache_remove, with_cache_mut};
use migrate::migrate_custom_api_keys_if_needed;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};

use crate::api_keys::{api_key_env_var, credential_metadata_key, store_credential_with_mode};
use crate::defaults::{self};
use crate::hooks::{HooksConfig, LifecycleHooksConfig, WorkspaceHookCommand, WorkspaceLifecycleHooks};
use crate::loader::config::VTCodeConfig;
use crate::loader::layers::{
    ConfigLayerEntry, ConfigLayerMetadata, ConfigLayerSource, ConfigLayerStack, LayerDisabledReason,
};
use crate::loader::session_override;
use vtcode_commons::VtCodePaths;
use vtcode_commons::canonicalize;

/// Timing metrics (in microseconds) for configuration loading phases
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigPhaseTiming {
    /// Duration of workspace path resolution (in microseconds)
    pub path_resolution_us: u64,
    /// Duration of layer probing and loading (in microseconds)
    pub layer_loading_us: u64,
    /// Duration of TOML merging and deserialization (in microseconds)
    pub merge_and_parse_us: u64,
    /// Duration of validation and API key migration (in microseconds)
    pub validation_us: u64,
}

/// Configuration manager for loading and validating configurations
#[derive(Clone)]
pub struct ConfigManager {
    pub(crate) config: VTCodeConfig,
    pub(super) config_path: Option<PathBuf>,
    pub(super) canonical_user_config_path: Option<PathBuf>,
    pub(super) tracked_user_config_paths: Vec<PathBuf>,
    write_global_config_to_canonical: bool,
    /// Whether the associated file was explicitly selected by the user.
    ///
    /// Explicit files are trusted even when they are represented as a
    /// workspace layer internally. Repository and project layers are not.
    pub(super) explicit_config_is_trusted: bool,
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) config_file_name: String,
    pub(crate) layer_stack: ConfigLayerStack,
    pub(super) phase_timing: Option<ConfigPhaseTiming>,
}

impl ConfigManager {
    /// Load configuration from the default locations rooted at the current directory.
    pub fn load() -> Result<Self> {
        if let Some(override_path) = session_override::explicit_config_path() {
            return Self::load_for_session(std::env::current_dir()?, &override_path).with_context(|| {
                format!("Failed to load configuration from explicit path {}", override_path.display())
            });
        }

        Self::load_from_workspace_with_repository_repair(std::env::current_dir()?)
    }

    /// Load configuration for an interactive session with an explicit config file.
    ///
    /// The explicit file takes the highest file-layer precedence (the same
    /// position a workspace root `vtcode.toml` would occupy), while the
    /// system/user global layers are still loaded underneath it. Unlike
    /// [`Self::load_from_file`], the manager's `workspace_root` remains the
    /// session workspace (not the explicit file's parent directory), so
    /// workspace-relative config writes and project-level paths keep
    /// resolving against the workspace.
    pub fn load_for_session(workspace: impl AsRef<Path>, explicit_path: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref();
        let explicit_path = explicit_path.as_ref();
        #[cfg(not(test))]
        let canonical_workspace = canonicalize_workspace_root(workspace);

        #[cfg(not(test))]
        if let Some(cached) = cache_get(&canonical_workspace) {
            return Ok(cached.as_ref().clone());
        }

        let mut manager = Self::load_from_file_impl(explicit_path, false).with_context(|| {
            format!(
                "Failed to load explicit session config file {} (from --config / VTCODE_CONFIG_PATH)",
                explicit_path.display()
            )
        })?;
        // The session workspace, not the explicit file's parent, anchors
        // workspace-relative config resolution for the rest of the session.
        manager.workspace_root = Some(canonicalize_workspace_root(workspace));

        #[cfg(not(test))]
        cache_insert(canonical_workspace, Arc::new(manager.clone()));

        Ok(manager)
    }

    /// Load only the system and user configuration layers.
    ///
    /// This is used by global configuration commands that must honor legacy
    /// and XDG search precedence without accidentally importing the current
    /// workspace's `vtcode.toml`.
    pub fn load_global() -> Result<Self> {
        let defaults_provider = defaults::current_config_defaults();
        let config_file_name = defaults_provider.config_file_name().to_string();
        let canonical_user_config_path = defaults_provider.canonical_user_config_path(&config_file_name)?;
        let mut tracked_user_config_paths = defaults_provider.home_config_paths(&config_file_name);
        if let Some(path) = &canonical_user_config_path
            && !tracked_user_config_paths.iter().any(|existing| existing == path)
        {
            tracked_user_config_paths.push(path.clone());
        }

        let mut layer_sources = Vec::new();
        for system_config_path in defaults_provider.system_config_paths(&config_file_name)? {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::System { file: system_config_path });
        }
        for home_config_path in &tracked_user_config_paths {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::User { file: home_config_path.clone() });
        }

        let mut layer_stack = ConfigLayerStack::default();
        for source in layer_sources {
            if let Some(layer) = Self::load_optional_layer(source) {
                layer_stack.push(layer);
            }
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!("Configuration layer '{}' failed to load: {}", layer.source.label(), error.message);
        }

        let (config, config_path) = if layer_stack.layers().is_empty() {
            let config = VTCodeConfig::default();
            config.validate().context("Default configuration failed validation")?;
            (config, None)
        } else {
            let (effective_toml, origins) = layer_stack.effective_config_with_origins();
            let mut config: VTCodeConfig = effective_toml
                .try_into()
                .context("Failed to deserialize effective global configuration")?;
            Self::validate_restricted_agent_fields(&layer_stack, &origins)?;
            Self::validate_provider_security_fields(&layer_stack, &origins, &config, false)?;
            config.validate().context("Global configuration failed validation")?;
            config.workspace_lifecycle_hooks =
                Some(Self::collect_workspace_lifecycle_hooks(&layer_stack, &config.hooks));
            let config_path = layer_stack
                .layers()
                .iter()
                .rev()
                .find(|layer| layer.is_enabled())
                .and_then(|layer| match &layer.source {
                    ConfigLayerSource::System { file } | ConfigLayerSource::User { file } => Some(file.clone()),
                    _ => None,
                });
            (config, config_path)
        };

        Ok(Self {
            config,
            config_path,
            canonical_user_config_path,
            tracked_user_config_paths,
            write_global_config_to_canonical: false,
            explicit_config_is_trusted: false,
            workspace_root: None,
            config_file_name,
            layer_stack,
            phase_timing: None,
        })
    }

    /// Invalidate the cached configuration for a specific workspace.
    ///
    /// Call this when config files may have changed on disk and the next
    /// `load_from_workspace` call should perform a fresh read instead of
    /// returning a previously cached result.
    pub fn invalidate_workspace_cache(workspace: impl AsRef<Path>) {
        let workspace = workspace.as_ref();
        #[cfg(not(test))]
        {
            let canonical_workspace = canonicalize_workspace_root(workspace);
            cache_remove(&canonical_workspace);
        }
        #[cfg(test)]
        let _ = workspace;
    }

    /// Invalidate every cached workspace configuration.
    ///
    /// Used when a session-scoped override changes or is cleared: the cache is
    /// keyed by canonical workspace only, so an override-loaded manager for any
    /// workspace must not leak into later default loads. This is a rare event
    /// (startup and tests), so a full sweep is cheap and safe.
    pub fn invalidate_all_workspace_cache() {
        #[cfg(not(test))]
        with_cache_mut(|map| map.clear());
    }

    /// Load configuration from a specific workspace
    ///
    /// When the session has an explicit config-file override (captured at
    /// startup via [`session_override::set_explicit_config_path`]), the
    /// override file takes precedence and is loaded as the highest file
    /// layer above the default global layers.
    pub fn load_from_workspace(workspace: impl AsRef<Path>) -> Result<Self> {
        if let Some(override_path) = session_override::explicit_config_path() {
            return Self::load_for_session(workspace, override_path);
        }

        let workspace = workspace.as_ref();
        #[cfg(not(test))]
        let canonical_workspace = canonicalize_workspace_root(workspace);

        #[cfg(not(test))]
        if let Some(cached) = cache_get(&canonical_workspace) {
            return Ok(cached.as_ref().clone());
        }

        let manager = Self::load_from_workspace_impl(workspace)?;

        #[cfg(not(test))]
        cache_insert(canonical_workspace, Arc::new(manager.clone()));

        Ok(manager)
    }

    /// Load a workspace configuration and repair provider settings left by
    /// older versions that persisted the merged configuration into a
    /// repository-controlled file.
    ///
    /// The strict loader still rejects repository-controlled provider
    /// definitions and endpoint/credential overrides. This entry point only
    /// handles that specific, already-validated violation by removing the
    /// prohibited fields from the exact repository layer that introduced it,
    /// then retrying the normal load. Explicit session files remain trusted
    /// and are never repaired.
    pub fn load_from_workspace_with_repository_repair(workspace: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref();
        let mut error = match Self::load_from_workspace(workspace) {
            Ok(manager) => return Ok(manager),
            Err(error) => error,
        };

        // More than one repository layer can contain a stale flattened copy.
        // Repair and retry until the strict loader succeeds or every known
        // repository layer has had a chance to be cleaned.
        let repository_paths = Self::repository_config_paths(workspace);
        for _ in 0..=repository_paths.len() {
            let Some(violation) = error.downcast_ref::<RepositoryProviderSecurityViolation>() else {
                return Err(error);
            };

            let source = violation.source.clone();
            let Some(path) = repository_paths
                .iter()
                .find(|candidate| Self::repository_source_matches_path(&source, candidate))
            else {
                return Err(error);
            };

            if !Self::repair_repository_config_file(path)? {
                return Err(error);
            }

            tracing::warn!(
                path = %path.display(),
                "Removed prohibited provider settings left by an older repository configuration write"
            );

            Self::invalidate_workspace_cache(workspace);
            match Self::load_from_workspace(workspace) {
                Ok(manager) => return Ok(manager),
                Err(next_error) => error = next_error,
            }
        }

        Err(error).context("Failed to load workspace configuration after repairing prohibited provider settings")
    }

    fn load_from_workspace_impl(workspace: impl AsRef<Path>) -> Result<Self> {
        let t0 = std::time::Instant::now();
        let workspace = workspace.as_ref();
        let defaults_provider = defaults::current_config_defaults();
        let workspace_paths = defaults_provider.workspace_paths_for(workspace);
        let workspace_root = canonicalize_workspace_root(workspace_paths.workspace_root());
        let config_dir = workspace_paths.config_dir();
        let config_file_name = defaults_provider.config_file_name().to_string();
        let canonical_user_config_path = defaults_provider.canonical_user_config_path(&config_file_name)?;
        let mut tracked_user_config_paths = defaults_provider.home_config_paths(&config_file_name);
        if let Some(path) = &canonical_user_config_path
            && !tracked_user_config_paths.iter().any(|existing| existing == path)
        {
            tracked_user_config_paths.push(path.clone());
        }
        let path_res_duration = t0.elapsed();

        let t1 = std::time::Instant::now();

        // Collect layer sources in precedence order so we can load them
        // concurrently and then push results while preserving order.
        let mut layer_sources: Vec<ConfigLayerSource> = Vec::with_capacity(8);

        // 1. System configuration: /etc compatibility followed by
        // XDG_CONFIG_DIRS candidates, all low-to-high precedence.
        for system_config_path in defaults_provider.system_config_paths(&config_file_name)? {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::System { file: system_config_path });
        }

        // 2. Legacy user config followed by the canonical XDG user config.
        for home_config_path in &tracked_user_config_paths {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::User { file: home_config_path.clone() });
        }

        // 3. Project-specific config (.vtcode/projects/<project>/config/vtcode.toml)
        if let Some(project_config_path) = Self::project_config_path(&config_dir, &workspace_root, &config_file_name) {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::Project { file: project_config_path });
        }

        // 4. Config directory fallback (.vtcode/vtcode.toml)
        let fallback_path = config_dir.join(&config_file_name);
        let workspace_config_path = workspace_root.join(&config_file_name);
        if fallback_path != workspace_config_path {
            push_unique_layer_source(&mut layer_sources, ConfigLayerSource::Workspace { file: fallback_path });
        }

        // 5. Workspace config (vtcode.toml in workspace root)
        push_unique_layer_source(
            &mut layer_sources,
            ConfigLayerSource::Workspace { file: workspace_config_path.clone() },
        );

        // Load all layers concurrently. Each load is independent I/O, so
        // spawning threads overlaps the disk reads and canonicalization
        // syscalls. The results are collected in source order to preserve
        // layer precedence.
        let raw_layers: Vec<Option<ConfigLayerEntry>> = std::thread::scope(|s| {
            let handles = layer_sources
                .into_iter()
                .map(|source| s.spawn(move || Self::load_optional_layer(source)))
                .collect::<Vec<_>>();
            // Join every handle first: `scope` re-panics if any panicked thread is left unjoined.
            let joined = handles.into_iter().map(|h| h.join()).collect::<Vec<_>>();
            joined
                .into_iter()
                .map(|r| r.map_err(|_payload| anyhow::anyhow!("config layer loader thread panicked")))
                .collect::<Result<Vec<_>>>()
        })?;

        let mut layer_stack = ConfigLayerStack::default();
        for layer in raw_layers.into_iter().flatten() {
            layer_stack.push(layer);
        }

        let layer_load_duration = t1.elapsed();

        // If no layers found, use default config
        if layer_stack.layers().is_empty() {
            let t_val = std::time::Instant::now();
            let config = VTCodeConfig::default();
            config.validate().context("Default configuration failed validation")?;
            let val_duration = t_val.elapsed();

            let phase_timing = ConfigPhaseTiming {
                path_resolution_us: path_res_duration.as_micros() as u64,
                layer_loading_us: layer_load_duration.as_micros() as u64,
                merge_and_parse_us: 0,
                validation_us: val_duration.as_micros() as u64,
            };
            tracing::debug!(target: "vtcode_config", ?phase_timing, "Default configuration loaded");

            return Ok(Self {
                config,
                config_path: None,
                canonical_user_config_path,
                tracked_user_config_paths,
                write_global_config_to_canonical: false,
                explicit_config_is_trusted: false,
                workspace_root: Some(workspace_root),
                config_file_name,
                layer_stack,
                phase_timing: Some(phase_timing),
            });
        }

        // If the workspace root vtcode.toml sets workspace.use_root_config = true,
        // discard all lower-precedence layers and re-merge with only the workspace
        // root and runtime layers.
        let use_root_config = layer_stack
            .layers()
            .iter()
            .find(|l| {
                matches!(
                    &l.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == workspace_config_path
                )
            })
            .map(|l| Self::workspace_root_wants_root_config_only(&l.config))
            .unwrap_or(false);
        if use_root_config {
            layer_stack.retain(|layer| {
                matches!(
                    &layer.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == workspace_config_path
                ) || matches!(&layer.source, ConfigLayerSource::Runtime)
            });
            if layer_stack.layers().is_empty() {
                bail!(
                    "workspace.use_root_config is true but no workspace root config was found at {}",
                    workspace_config_path.display()
                );
            }
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!("Configuration layer '{}' failed to load: {}", layer.source.label(), error.message);
        }

        let t2 = std::time::Instant::now();
        let (effective_toml, origins) = layer_stack.effective_config_with_origins();
        let mut config: VTCodeConfig = effective_toml
            .try_into()
            .context("Failed to deserialize effective configuration")?;
        let merge_duration = t2.elapsed();

        let t3 = std::time::Instant::now();
        Self::validate_restricted_agent_fields(&layer_stack, &origins)?;
        Self::validate_provider_security_fields(&layer_stack, &origins, &config, false)?;

        config.validate().context("Configuration failed validation")?;
        config.workspace_lifecycle_hooks = Some(Self::collect_workspace_lifecycle_hooks(&layer_stack, &config.hooks));

        // Migrate any plain-text API keys from config to secure storage
        migrate_custom_api_keys_if_needed(&mut config)?;
        let val_duration = t3.elapsed();

        let phase_timing = ConfigPhaseTiming {
            path_resolution_us: path_res_duration.as_micros() as u64,
            layer_loading_us: layer_load_duration.as_micros() as u64,
            merge_and_parse_us: merge_duration.as_micros() as u64,
            validation_us: val_duration.as_micros() as u64,
        };
        tracing::debug!(target: "vtcode_config", ?phase_timing, "Workspace configuration loaded");

        let config_path = layer_stack
            .layers()
            .iter()
            .rev()
            .find(|layer| layer.is_enabled())
            .and_then(|l| match &l.source {
                ConfigLayerSource::User { file } => Some(file.clone()),
                ConfigLayerSource::Project { file } => Some(file.clone()),
                ConfigLayerSource::Workspace { file } => Some(file.clone()),
                ConfigLayerSource::System { file } => Some(file.clone()),
                ConfigLayerSource::Runtime => None,
            });

        Ok(Self {
            config,
            config_path,
            canonical_user_config_path,
            tracked_user_config_paths,
            write_global_config_to_canonical: false,
            explicit_config_is_trusted: false,
            workspace_root: Some(workspace_root),
            config_file_name,
            layer_stack,
            phase_timing: Some(phase_timing),
        })
    }

    fn load_toml_from_file(path: &Path) -> Result<toml::Value> {
        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let mut value: toml::Value =
            toml::from_str(&content).with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        crate::loader::merge::normalize_legacy_top_level_provider_aliases(&mut value);
        Ok(value)
    }

    fn load_optional_layer(source: ConfigLayerSource) -> Option<ConfigLayerEntry> {
        let file = match &source {
            ConfigLayerSource::System { file }
            | ConfigLayerSource::User { file }
            | ConfigLayerSource::Project { file }
            | ConfigLayerSource::Workspace { file } => file,
            ConfigLayerSource::Runtime => {
                return Some(ConfigLayerEntry::new(source, toml::Value::Table(toml::Table::new())));
            }
        };

        // Read first so missing files return None without a redundant canonicalize.
        let content = match fs::read_to_string(file) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
            Err(err) if should_skip_optional_global_error(&source, &err) => {
                tracing::debug!(path = %file.display(), "skipping inaccessible optional configuration layer");
                return None;
            }
            Err(err) => {
                let resolved_file = canonicalize_workspace_root(file);
                let resolved_source = source.with_file(resolved_file);
                return Some(Self::disabled_layer_from_error(resolved_source, err.into()));
            }
        };

        let resolved_file = canonicalize_workspace_root(file);
        let resolved_source = source.with_file(resolved_file);

        match toml::from_str::<toml::Value>(&content) {
            Ok(mut toml) => {
                crate::loader::merge::normalize_legacy_top_level_provider_aliases(&mut toml);
                Some(ConfigLayerEntry::new(resolved_source, toml))
            }
            Err(err) => {
                let error =
                    anyhow::Error::from(err).context(format!("Failed to parse config file: {}", file.display()));
                Some(Self::disabled_layer_from_error(resolved_source, error))
            }
        }
    }

    fn disabled_layer_from_error(source: ConfigLayerSource, error: anyhow::Error) -> ConfigLayerEntry {
        let reason = if error.is::<toml::de::Error>() {
            LayerDisabledReason::ParseError
        } else {
            LayerDisabledReason::LoadError
        };
        ConfigLayerEntry::disabled(source, reason, format!("{error:#}"))
    }

    /// Check whether a parsed TOML value has `workspace.use_root_config = true`.
    ///
    /// This is checked against the already-loaded workspace root layer config
    /// to avoid a redundant file read.
    fn workspace_root_wants_root_config_only(config: &toml::Value) -> bool {
        config
            .get("workspace")
            .and_then(|v| v.get("use_root_config"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::load_from_file_impl(path, false)
    }

    fn load_from_file_impl(path: impl AsRef<Path>, write_global_config_to_canonical: bool) -> Result<Self> {
        let path = path.as_ref();
        let defaults_provider = defaults::current_config_defaults();
        // Global layer probing always uses the canonical config file name, never
        // the basename of the explicit override file. Deriving it from the file
        // previously caused global layers (e.g. `~/.config/vtcode/vtcode.toml`
        // with `[[custom_providers]]`) to be skipped whenever the override file
        // was named something else (e.g. `night.toml`), silently dropping
        // custom providers and other user-global settings.
        let config_file_name = defaults_provider.config_file_name().to_string();
        let canonical_user_config_path = defaults_provider.canonical_user_config_path(&config_file_name)?;
        let mut tracked_user_config_paths = defaults_provider.home_config_paths(&config_file_name);
        if let Some(path) = &canonical_user_config_path
            && !tracked_user_config_paths.iter().any(|existing| existing == path)
        {
            tracked_user_config_paths.push(path.clone());
        }

        let mut layer_stack = ConfigLayerStack::default();
        let mut global_sources = Vec::new();

        for system_config in defaults_provider.system_config_paths(&config_file_name)? {
            push_unique_layer_source(&mut global_sources, ConfigLayerSource::System { file: system_config });
        }
        for home_config_path in &tracked_user_config_paths {
            push_unique_layer_source(&mut global_sources, ConfigLayerSource::User { file: home_config_path.clone() });
        }
        for source in global_sources {
            if let Some(layer) = Self::load_optional_layer(source) {
                layer_stack.push(layer);
            }
        }

        // 3. The specific file provided (Workspace layer)
        let file = path.to_path_buf();
        match Self::load_toml_from_file(path) {
            Ok(toml) => layer_stack.push(ConfigLayerEntry::new(ConfigLayerSource::Workspace { file }, toml)),
            Err(error) => {
                layer_stack.push(Self::disabled_layer_from_error(ConfigLayerSource::Workspace { file }, error))
            }
        }

        // If the provided file sets workspace.use_root_config = true, discard
        // lower-precedence layers so only this file and runtime overrides apply.
        let use_root_config = layer_stack
            .layers()
            .iter()
            .find(|l| {
                matches!(
                    &l.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == path
                )
            })
            .map(|l| Self::workspace_root_wants_root_config_only(&l.config))
            .unwrap_or(false);
        if use_root_config {
            layer_stack.retain(|layer| {
                matches!(
                    &layer.source,
                    ConfigLayerSource::Workspace { file }
                        if *file == path
                ) || matches!(&layer.source, ConfigLayerSource::Runtime)
            });
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!("Configuration layer '{}' failed to load: {}", layer.source.label(), error.message);
        }

        let (effective_toml, origins) = layer_stack.effective_config_with_origins();
        let mut config: VTCodeConfig = effective_toml
            .try_into()
            .with_context(|| format!("Failed to parse effective config with file: {}", path.display()))?;
        Self::validate_restricted_agent_fields(&layer_stack, &origins)?;
        // A path supplied explicitly by the user is an opt-in trusted layer,
        // even though it uses the Workspace source variant for precedence and
        // compatibility with the existing loader.
        Self::validate_provider_security_fields(&layer_stack, &origins, &config, true)?;

        config
            .validate()
            .with_context(|| format!("Failed to validate effective config with file: {}", path.display()))?;
        config.workspace_lifecycle_hooks = Some(Self::collect_workspace_lifecycle_hooks(&layer_stack, &config.hooks));

        Ok(Self {
            config,
            config_path: Some(canonicalize_workspace_root(path)),
            canonical_user_config_path,
            tracked_user_config_paths,
            write_global_config_to_canonical,
            explicit_config_is_trusted: true,
            workspace_root: path.parent().map(canonicalize_workspace_root),
            config_file_name,
            layer_stack,
            phase_timing: None,
        })
    }

    /// Resolve the current project name used for project-level config overlays.
    pub fn current_project_name(workspace_root: &Path) -> Option<String> {
        Self::identify_current_project(workspace_root)
    }

    fn validate_restricted_agent_fields(
        layer_stack: &ConfigLayerStack,
        origins: &hashbrown::HashMap<String, ConfigLayerMetadata>,
    ) -> Result<()> {
        if let Some(origin) = origins.get("agent.persistent_memory.directory_override")
            && let Some(layer) = layer_stack.layers().iter().find(|layer| layer.metadata == *origin)
        {
            match layer.source {
                ConfigLayerSource::System { .. }
                | ConfigLayerSource::User { .. }
                | ConfigLayerSource::Project { .. } => {}
                ConfigLayerSource::Workspace { .. } | ConfigLayerSource::Runtime => {
                    bail!(
                        "agent.persistent_memory.directory_override may only be set in system, user, or project-profile configuration layers"
                    );
                }
            }
        }

        Ok(())
    }

    /// Reject provider settings from repository-controlled layers before they
    /// can reach provider registration or request handling.
    ///
    /// Workspace and project files are repository-controlled input. They may
    /// configure ordinary agent behavior, but must not introduce executable
    /// authentication commands or redirect provider requests/credentials.
    /// `load_from_file` is an explicit user opt-in and therefore passes
    /// `explicit_config_is_trusted = true`.
    fn validate_provider_security_fields(
        layer_stack: &ConfigLayerStack,
        origins: &hashbrown::HashMap<String, ConfigLayerMetadata>,
        config: &VTCodeConfig,
        explicit_config_is_trusted: bool,
    ) -> Result<()> {
        if explicit_config_is_trusted {
            return Ok(());
        }

        if !config.custom_providers.is_empty()
            && let Some(origin) = origins.get("custom_providers")
            && let Some(layer) = Self::enabled_layer_for_origin(layer_stack, origin)
            && Self::is_repository_controlled_source(&layer.source)
        {
            return Err(RepositoryProviderSecurityViolation {
                source: layer.source.clone(),
                message: format!(
                    "repository-controlled configuration cannot define `custom_providers` (including `auth.command`); move custom provider settings to system, user, or an explicitly selected config file (source: {})",
                    layer.source.label()
                ),
            }
            .into());
        }

        for (path, origin) in origins {
            if path == "auth.copilot.command"
                && config
                    .auth
                    .copilot
                    .command
                    .as_deref()
                    .is_some_and(|command| !command.trim().is_empty())
                && let Some(layer) = Self::enabled_layer_for_origin(layer_stack, origin)
                && Self::is_repository_controlled_source(&layer.source)
            {
                bail!(
                    "repository-controlled configuration cannot set `auth.copilot.command`; configure it in system, user, or an explicitly selected config file (source: {})",
                    layer.source.label()
                );
            }
            if !Self::is_provider_endpoint_or_credential_path(path) {
                continue;
            }

            let Some(layer) = Self::enabled_layer_for_origin(layer_stack, origin) else {
                continue;
            };
            if Self::is_repository_controlled_source(&layer.source) {
                return Err(RepositoryProviderSecurityViolation {
                    source: layer.source.clone(),
                    message: format!(
                        "repository-controlled configuration cannot set `{path}`; provider endpoints and credential environment variables must be configured in system, user, or an explicitly selected config file (source: {})",
                        layer.source.label()
                    ),
                }
                .into());
            }
        }

        Ok(())
    }

    fn enabled_layer_for_origin<'a>(
        layer_stack: &'a ConfigLayerStack,
        origin: &ConfigLayerMetadata,
    ) -> Option<&'a ConfigLayerEntry> {
        layer_stack
            .layers()
            .iter()
            .find(|layer| layer.is_enabled() && layer.metadata == *origin)
    }

    fn is_repository_controlled_source(source: &ConfigLayerSource) -> bool {
        matches!(source, ConfigLayerSource::Project { .. } | ConfigLayerSource::Workspace { .. })
    }

    fn is_provider_endpoint_or_credential_path(path: &str) -> bool {
        let mut segments = path.split('.');
        matches!(segments.next(), Some("provider_overrides"))
            && segments.next().is_some()
            && matches!(segments.next(), Some("base_url" | "api_key_env"))
            && segments.next().is_none()
    }

    /// Collect lifecycle hook commands whose effective origin is a
    /// workspace-controlled layer: the workspace-root `vtcode.toml`, the
    /// workspace `.vtcode/vtcode.toml` fallback, or a project profile stored
    /// inside the workspace. These are attacker-influenceable in an untrusted
    /// repository and must be gated behind explicit user approval.
    pub(crate) fn collect_workspace_lifecycle_hooks(
        layer_stack: &ConfigLayerStack,
        hooks: &HooksConfig,
    ) -> WorkspaceLifecycleHooks {
        let (_, origins) = layer_stack.effective_config_with_origins();
        let mut commands = Vec::new();

        for event in crate::hooks::LIFECYCLE_HOOK_EVENTS {
            let origin_key = format!("hooks.lifecycle.{event}");
            let Some(origin) = origins.get(&origin_key) else {
                continue;
            };
            let workspace_controlled = layer_stack.layers().iter().any(|layer| {
                layer.is_enabled()
                    && layer.metadata == *origin
                    && matches!(layer.source, ConfigLayerSource::Workspace { .. } | ConfigLayerSource::Project { .. })
            });
            if !workspace_controlled {
                continue;
            }

            // Deprecated aliases fold into `stop` at engine normalization.
            let tag = match *event {
                "task_completion" | "task_completed" => "stop",
                other => other,
            };
            for group in hooks.lifecycle.groups_for_event(event) {
                for command in &group.hooks {
                    commands.push(WorkspaceHookCommand {
                        event: tag.to_string(),
                        matcher: group.matcher.clone(),
                        command: command.command.clone(),
                        timeout_seconds: command.timeout_seconds,
                    });
                }
            }
        }

        WorkspaceLifecycleHooks { commands }
    }
}

#[cfg(test)]
mod sparse_config_tests;
#[cfg(test)]
mod workspace_lifecycle_hooks_tests;
