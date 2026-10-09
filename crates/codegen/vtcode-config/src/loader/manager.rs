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

type CachedManager = Arc<ConfigManager>;

#[derive(Debug)]
struct RepositoryProviderSecurityViolation {
    source: ConfigLayerSource,
    message: String,
}

impl fmt::Display for RepositoryProviderSecurityViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for RepositoryProviderSecurityViolation {}

#[cfg(not(test))]
static WORKSPACE_CACHE: Mutex<Option<HashMap<PathBuf, CachedManager>>> = Mutex::new(None);

#[cfg(not(test))]
fn with_cache_mut(f: impl FnOnce(&mut HashMap<PathBuf, CachedManager>)) {
    let mut guard = WORKSPACE_CACHE.lock().expect("config cache lock poisoned");
    guard.get_or_insert_with(HashMap::new);
    f(guard.as_mut().expect("cache initialized"))
}

#[cfg(not(test))]
fn cache_get(workspace: &Path) -> Option<CachedManager> {
    WORKSPACE_CACHE
        .lock()
        .expect("config cache lock poisoned")
        .as_ref()
        .and_then(|map| map.get(workspace).cloned())
}

#[cfg(not(test))]
fn cache_insert(workspace: PathBuf, manager: CachedManager) {
    with_cache_mut(move |map| {
        map.insert(workspace, manager);
    });
}

#[cfg(not(test))]
fn cache_remove(workspace: &Path) {
    with_cache_mut(|map| {
        map.remove(workspace);
    });
}

pub(crate) fn canonicalize_workspace_root(path: &Path) -> PathBuf {
    canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn push_unique_layer_source(layer_sources: &mut Vec<ConfigLayerSource>, source: ConfigLayerSource) {
    let file = match &source {
        ConfigLayerSource::System { file }
        | ConfigLayerSource::User { file }
        | ConfigLayerSource::Project { file }
        | ConfigLayerSource::Workspace { file } => file,
        ConfigLayerSource::Runtime => {
            layer_sources.push(source);
            return;
        }
    };
    if !layer_sources.iter().any(|existing| {
        matches!(existing,
            ConfigLayerSource::System { file: existing_file }
            | ConfigLayerSource::User { file: existing_file }
            | ConfigLayerSource::Project { file: existing_file }
            | ConfigLayerSource::Workspace { file: existing_file }
            if existing_file == file)
    }) {
        layer_sources.push(source);
    }
}

fn is_optional_global_layer(source: &ConfigLayerSource) -> bool {
    matches!(source, ConfigLayerSource::System { .. } | ConfigLayerSource::User { .. })
}

fn should_skip_optional_global_error(source: &ConfigLayerSource, error: &std::io::Error) -> bool {
    is_optional_global_layer(source) && error.kind() != std::io::ErrorKind::InvalidData
}

fn ensure_private_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let _ = VtCodePaths::ensure_user_dir(parent)
        .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    Ok(())
}

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
    config_path: Option<PathBuf>,
    canonical_user_config_path: Option<PathBuf>,
    tracked_user_config_paths: Vec<PathBuf>,
    write_global_config_to_canonical: bool,
    /// Whether the associated file was explicitly selected by the user.
    ///
    /// Explicit files are trusted even when they are represented as a
    /// workspace layer internally. Repository and project layers are not.
    explicit_config_is_trusted: bool,
    workspace_root: Option<PathBuf>,
    config_file_name: String,
    pub(crate) layer_stack: ConfigLayerStack,
    phase_timing: Option<ConfigPhaseTiming>,
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

    fn repository_config_paths(workspace: &Path) -> Vec<PathBuf> {
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

    fn repository_source_matches_path(source: &ConfigLayerSource, path: &Path) -> bool {
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

    /// Persist configuration to a specific path, preserving comments.
    pub fn save_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        Self::save_config_to_path_internal(path, config, false)
    }

    /// Persist configuration to a repository-controlled path.
    ///
    /// Repository and project files may contain ordinary settings, but they
    /// must never receive trusted provider definitions or provider endpoint and
    /// credential overrides from the merged configuration. Existing protected
    /// keys are removed as well so this method can repair files written by an
    /// older version that flattened the effective configuration.
    pub fn save_repository_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        Self::save_config_to_path_internal(path, config, true)
    }

    fn save_config_to_path_internal(
        path: impl AsRef<Path>,
        config: &VTCodeConfig,
        repository_controlled: bool,
    ) -> Result<()> {
        let path = path.as_ref();
        ensure_private_parent_dir(path)?;
        let config_to_persist = if repository_controlled {
            Self::repository_safe_config(config)
        } else {
            config.clone()
        };
        let sparse_value =
            Self::sparse_config_value(&config_to_persist).context("Failed to prepare sparse configuration")?;
        let sparse_content =
            toml::to_string_pretty(&sparse_value).context("Failed to serialize sparse configuration")?;

        // If file exists, preserve comments by using toml_edit. Read and
        // publish through the shared no-follow/atomic file policy so a
        // user-controlled symlink cannot redirect configuration writes.
        let existing_content = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Refusing to read symlinked config file: {}", path.display())
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("Config path is not a regular file: {}", path.display())
            }
            Ok(_) => Some(
                String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
                    .with_context(|| format!("Failed to read existing config: {}", path.display()))?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to inspect config: {}", path.display()));
            }
        };

        if let Some(original_content) = existing_content {
            let mut doc = original_content
                .parse::<toml_edit::DocumentMut>()
                .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;
            Self::remove_deprecated_config_keys(&mut doc);
            if repository_controlled {
                Self::remove_repository_provider_settings(&mut doc);
            }

            let new_doc: toml_edit::DocumentMut = sparse_content
                .parse()
                .context("Failed to parse sparse serialized configuration")?;
            let default_value =
                toml::Value::try_from(VTCodeConfig::default()).context("Failed to serialize default configuration")?;
            let default_doc: toml_edit::DocumentMut = toml::to_string_pretty(&default_value)
                .context("Failed to serialize default configuration")?
                .parse()
                .context("Failed to parse default serialized configuration")?;

            // Update values while preserving structure and comments
            Self::merge_sparse_toml_documents(&mut doc, &new_doc, &default_doc);
            // The sparse merge cannot express the deletion of optional fields
            // whose default serializes to absent (they appear in neither the
            // sparse update nor the defaults), so resets issued through save
            // paths (e.g. service_tier back to Project default) would leave
            // stale file values behind forever. Reconcile those explicitly.
            Self::prune_cleared_optional_fields(&mut doc, &config_to_persist);

            VtCodePaths::write_private_file_atomic(path, doc.to_string().as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        } else {
            VtCodePaths::write_private_file_atomic(path, sparse_content.as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }

        Ok(())
    }

    fn repository_safe_config(config: &VTCodeConfig) -> VTCodeConfig {
        let mut safe_config = config.clone();
        safe_config.custom_providers.clear();
        for provider_override in safe_config.provider_overrides.values_mut() {
            provider_override.base_url = None;
            provider_override.api_key_env = None;
        }
        safe_config
    }

    /// Remove file values for optional fields the effective config reset to
    /// their absent default (e.g. `provider.openai.service_tier` back to
    /// Project default). The sparse merge cannot express these deletions
    /// (absent from both the sparse update and the defaults), so each entry
    /// pairs a dotted path with a typed cleared-check. Only listed paths are
    /// touched; foreign keys are never removed. Extend the list when a new
    /// resettable optional reaches a save path.
    fn prune_cleared_optional_fields(doc: &mut toml_edit::DocumentMut, config: &VTCodeConfig) {
        // Each entry: (dotted path, is_cleared).
        let cleared: &[(&[&str], bool)] =
            &[(&["provider", "openai", "service_tier"], config.provider.openai.service_tier.is_none())];
        for (path, is_cleared) in cleared {
            if *is_cleared {
                Self::remove_path_and_prune_empty_parents(doc.as_table_mut(), path);
            }
        }
    }

    /// Remove `path` from `table`, then drop ancestor tables left empty.
    /// Returns true when `table` itself is now empty.
    fn remove_path_and_prune_empty_parents(table: &mut toml_edit::Table, path: &[&str]) -> bool {
        let Some((first, rest)) = path.split_first() else {
            return table.is_empty();
        };
        if rest.is_empty() {
            table.remove(first);
        } else if let Some(child) = table.get_mut(first).and_then(toml_edit::Item::as_table_mut) {
            if Self::remove_path_and_prune_empty_parents(child, rest) {
                table.remove(first);
            }
        }
        table.is_empty()
    }

    fn repair_repository_config_file(path: &Path) -> Result<bool> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Refusing to repair symlinked config file: {}", path.display())
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("Config path is not a regular file: {}", path.display())
            }
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to inspect config: {}", path.display()));
            }
        };
        debug_assert!(metadata.is_file());

        let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
            .with_context(|| format!("Failed to read existing config: {}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;
        if !Self::remove_repository_provider_settings(&mut doc) {
            return Ok(false);
        }

        VtCodePaths::write_private_file_atomic(path, doc.to_string().as_bytes())
            .with_context(|| format!("Failed to repair config file: {}", path.display()))?;
        Ok(true)
    }

    fn remove_repository_provider_settings(doc: &mut toml_edit::DocumentMut) -> bool {
        let table = doc.as_table_mut();
        let mut changed = table.remove("custom_providers").is_some();

        let remove_provider_overrides = {
            let Some(overrides_item) = table.get_mut("provider_overrides") else {
                return changed;
            };

            if let Some(overrides) = overrides_item.as_table_mut() {
                changed |= Self::remove_provider_override_settings_from_table(overrides);
                overrides.is_empty()
            } else if let Some(overrides) = overrides_item.as_inline_table_mut() {
                changed |= Self::remove_provider_override_settings_from_inline_table(overrides);
                overrides.is_empty()
            } else {
                false
            }
        };

        if remove_provider_overrides {
            changed |= table.remove("provider_overrides").is_some();
        }
        changed
    }

    fn remove_provider_override_settings_from_table(overrides: &mut toml_edit::Table) -> bool {
        let provider_names = overrides.iter().map(|(name, _)| name.to_string()).collect::<Vec<_>>();
        let mut empty_providers = Vec::new();
        let mut changed = false;

        for provider_name in provider_names {
            let Some(provider_item) = overrides.get_mut(&provider_name) else {
                continue;
            };

            let is_empty = if let Some(provider) = provider_item.as_table_mut() {
                changed |= provider.remove("base_url").is_some();
                changed |= provider.remove("api_key_env").is_some();
                provider.is_empty()
            } else if let Some(provider) = provider_item.as_inline_table_mut() {
                changed |= provider.remove("base_url").is_some();
                changed |= provider.remove("api_key_env").is_some();
                provider.is_empty()
            } else {
                false
            };

            if is_empty {
                empty_providers.push(provider_name);
            }
        }

        for provider_name in empty_providers {
            changed |= overrides.remove(&provider_name).is_some();
        }
        changed
    }

    fn remove_provider_override_settings_from_inline_table(overrides: &mut toml_edit::InlineTable) -> bool {
        let provider_names = overrides.iter().map(|(name, _)| name.to_string()).collect::<Vec<_>>();
        let mut empty_providers = Vec::new();
        let mut changed = false;

        for provider_name in provider_names {
            let Some(provider) = overrides
                .get_mut(&provider_name)
                .and_then(toml_edit::Value::as_inline_table_mut)
            else {
                continue;
            };

            changed |= provider.remove("base_url").is_some();
            changed |= provider.remove("api_key_env").is_some();
            if provider.is_empty() {
                empty_providers.push(provider_name);
            }
        }

        for provider_name in empty_providers {
            changed |= overrides.remove(&provider_name).is_some();
        }
        changed
    }

    fn remove_deprecated_config_keys(doc: &mut toml_edit::DocumentMut) {
        let table = doc.as_table_mut();
        table.remove("project_doc_max_bytes");
        table.remove("project_doc_fallback_filenames");
        Self::remove_table_keys(table, "agent", &["autonomous_mode", "default_editing_mode"]);
        Self::remove_table_keys(table, "permissions", &["allowed_tools", "disallowed_tools", "auto_permission"]);
    }

    fn remove_table_keys(table: &mut toml_edit::Table, section: &str, keys: &[&str]) {
        let Some(section) = table.get_mut(section).and_then(toml_edit::Item::as_table_mut) else {
            return;
        };

        for key in keys {
            section.remove(key);
        }
    }

    pub fn sparse_config_value(config: &VTCodeConfig) -> Result<toml::Value> {
        let mut value = toml::Value::try_from(config).context("Failed to serialize configuration")?;
        let default_value =
            toml::Value::try_from(VTCodeConfig::default()).context("Failed to serialize default configuration")?;
        Self::prune_default_values(&mut value, &default_value);
        Ok(value)
    }

    fn prune_default_values(value: &mut toml::Value, default_value: &toml::Value) -> bool {
        match (value, default_value) {
            (toml::Value::Table(table), toml::Value::Table(default_table)) => {
                table.retain(|key, child| {
                    default_table
                        .get(key)
                        .is_none_or(|default_child| !Self::prune_default_values(child, default_child))
                });
                table.is_empty()
            }
            (value, default_value) => value == default_value,
        }
    }

    /// Merge TOML documents, preserving comments and structure from original
    fn merge_sparse_toml_documents(
        original: &mut toml_edit::DocumentMut,
        new: &toml_edit::DocumentMut,
        default_doc: &toml_edit::DocumentMut,
    ) {
        Self::merge_sparse_tables(original.as_table_mut(), new.as_table(), default_doc.as_table());
    }

    fn merge_sparse_tables(original: &mut toml_edit::Table, new: &toml_edit::Table, default_table: &toml_edit::Table) {
        let mut remove_keys = Vec::with_capacity(default_table.len());

        for (key, default_value) in default_table.iter() {
            if let Some(new_value) = new.get(key) {
                if let Some(original_value) = original.get_mut(key) {
                    Self::merge_sparse_items(original_value, new_value, default_value);
                } else {
                    original[key] = new_value.clone();
                }
            } else {
                let Some(original_value) = original.get_mut(key) else {
                    continue;
                };
                if Self::remove_known_default_item(original_value, default_value) {
                    remove_keys.push(key.to_string());
                }
            }
        }

        for key in remove_keys {
            original.remove(&key);
        }

        for (key, new_value) in new.iter() {
            if default_table.contains_key(key) {
                continue;
            }
            if let Some(original_value) = original.get_mut(key) {
                *original_value = new_value.clone();
            } else {
                original[key] = new_value.clone();
            }
        }
    }

    fn merge_sparse_items(original: &mut toml_edit::Item, new: &toml_edit::Item, default_value: &toml_edit::Item) {
        match (original, new, default_value) {
            (
                toml_edit::Item::Table(orig_table),
                toml_edit::Item::Table(new_table),
                toml_edit::Item::Table(default_table),
            ) => Self::merge_sparse_tables(orig_table, new_table, default_table),
            (orig, new, _) => {
                *orig = new.clone();
            }
        }
    }

    fn remove_known_default_item(original: &mut toml_edit::Item, default_value: &toml_edit::Item) -> bool {
        match (original, default_value) {
            (toml_edit::Item::Table(orig_table), toml_edit::Item::Table(default_table)) => {
                let mut remove_keys = Vec::new();
                for (key, default_child) in default_table.iter() {
                    let Some(orig_child) = orig_table.get_mut(key) else {
                        continue;
                    };
                    if Self::remove_known_default_item(orig_child, default_child) {
                        remove_keys.push(key.to_string());
                    }
                }
                for key in remove_keys {
                    orig_table.remove(&key);
                }
                orig_table.is_empty()
            }
            _ => true,
        }
    }

    fn project_config_path(config_dir: &Path, workspace_root: &Path, config_file_name: &str) -> Option<PathBuf> {
        let project_name = Self::identify_current_project(workspace_root)?;
        Some(
            config_dir
                .join("projects")
                .join(project_name)
                .join("config")
                .join(config_file_name),
        )
    }

    fn identify_current_project(workspace_root: &Path) -> Option<String> {
        let project_file = workspace_root.join(".vtcode-project");
        if let Ok(contents) = fs::read_to_string(&project_file) {
            let name = contents.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }

        workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
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

    /// Persist configuration to the manager's associated path or workspace
    pub fn save_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        let (target, repository_controlled) = if let Some(path) = &self.config_path {
            if self.write_global_config_to_canonical || self.is_global_config_path(path) {
                (self.preferred_user_config_path().unwrap_or_else(|| path.clone()), false)
            } else {
                (path.clone(), !self.explicit_config_is_trusted)
            }
        } else if let Some(workspace_root) = &self.workspace_root {
            (workspace_root.join(&self.config_file_name), !self.explicit_config_is_trusted)
        } else {
            let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
            (cwd.join(&self.config_file_name), !self.explicit_config_is_trusted)
        };

        if repository_controlled {
            Self::save_repository_config_to_path(target, config)?;
        } else {
            Self::save_config_to_path(target, config)?;
        }

        #[cfg(not(test))]
        if let Some(workspace) = &self.workspace_root {
            Self::invalidate_workspace_cache(workspace);
        }

        self.sync_from_config(config)
    }

    fn is_global_config_path(&self, path: &Path) -> bool {
        self.layer_stack.layers().iter().any(|layer| {
            layer.is_enabled()
                && matches!(layer.source, ConfigLayerSource::System { .. } | ConfigLayerSource::User { .. })
                && match &layer.source {
                    ConfigLayerSource::System { file } | ConfigLayerSource::User { file } => file == path,
                    _ => false,
                }
        })
    }

    /// Sync internal config from a saved config
    /// Call this after save_config to keep internal state in sync
    fn sync_from_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        self.config = config.clone();
        Ok(())
    }
}

/// Migrate plain-text API keys from config to secure storage.
///
/// This function checks if there are any API keys stored in plain-text in the config
/// and migrates them to secure storage (keyring). After successful migration,
/// the key material is cleared and a normalized provider/key metadata marker is
/// retained for configuration introspection.
///
/// # Arguments
/// * `config` - The configuration to migrate
fn migrate_custom_api_keys_if_needed(config: &mut VTCodeConfig) -> Result<()> {
    let storage_mode = config.agent.credential_storage_mode;

    // Check if there are any non-empty API keys in the config
    let has_plain_text_keys = config.agent.custom_api_keys.values().any(|key| !key.is_empty());

    if has_plain_text_keys {
        tracing::info!("Detected plain-text API keys in config, migrating to secure storage...");

        let mut migrated_count = 0;
        let pending = config
            .agent
            .custom_api_keys
            .iter()
            .filter(|(_, key)| !key.is_empty())
            .map(|(provider, key)| (provider.clone(), key.clone()))
            .collect::<Vec<_>>();
        for (provider, api_key) in pending {
            let key_name = config
                .configured_api_key_env(&provider)
                .unwrap_or_else(|| api_key_env_var(&provider));
            if let Some(identity) = store_credential_with_mode(&provider, &key_name, &api_key, storage_mode)? {
                config.agent.custom_api_keys.remove(&provider);
                if let Some(metadata_key) = credential_metadata_key(identity.provider(), identity.key_name())? {
                    config.agent.custom_api_keys.insert(metadata_key, String::new());
                }
                migrated_count += 1;
            }
        }

        if migrated_count > 0 {
            tracing::info!("Successfully migrated {} API key(s) to secure storage", migrated_count);
            tracing::warn!(
                "Plain-text API keys have been cleared from config file. \
                 Please commit the updated config to remove sensitive data from version control."
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod sparse_config_tests {
    use super::*;

    #[test]
    fn sparse_config_omits_default_primary_agent_when_unmodified() {
        let config = VTCodeConfig::default();
        let value = ConfigManager::sparse_config_value(&config).expect("sparse config");

        assert!(value.get("default_primary_agent").is_none());
    }

    #[test]
    fn sparse_config_persists_non_default_primary_agent() {
        let config = VTCodeConfig {
            default_primary_agent: "auto".to_string(),
            ..Default::default()
        };

        let value = ConfigManager::sparse_config_value(&config).expect("sparse config");

        assert_eq!(value.get("default_primary_agent").and_then(toml::Value::as_str), Some("auto"));
    }

    #[test]
    fn has_explicit_top_level_key_matches_effective_config() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace = temp_dir.path().join("workspace");
        fs::create_dir_all(workspace.join(".vtcode")).expect("workspace dot dir");

        let manager = crate::loader::ConfigBuilder::new()
            .workspace(workspace.clone())
            .build()
            .expect("manager");
        assert!(!manager.has_explicit_top_level_key("default_primary_agent"));
        assert!(
            manager
                .effective_config()
                .as_table()
                .is_none_or(|table| !table.contains_key("default_primary_agent"))
        );

        let overridden = crate::loader::ConfigBuilder::new()
            .workspace(workspace)
            .cli_override("default_primary_agent".to_owned(), toml::Value::String("duck".to_owned()))
            .build()
            .expect("overridden manager");
        assert!(overridden.has_explicit_top_level_key("default_primary_agent"));
        assert!(
            overridden
                .effective_config()
                .as_table()
                .is_some_and(|table| table.contains_key("default_primary_agent"))
        );
        assert!(!overridden.has_explicit_top_level_key("definitely_missing_key"));
    }

    #[test]
    fn save_config_migrates_legacy_permissions_auto_permission_table() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("vtcode.toml");
        fs::write(
            &config_path,
            r#"
[permissions.auto_permission]
model = "legacy-reviewer"
max_consecutive_denials = 2
"#,
        )
        .expect("write legacy config");

        let mut config = VTCodeConfig::default();
        config.permissions.auto_permission.model = "legacy-reviewer".to_string();
        config.permissions.auto_permission.max_consecutive_denials = 2;

        ConfigManager::save_config_to_path(&config_path, &config).expect("save config");

        let saved_content = fs::read_to_string(&config_path).expect("read saved config");
        assert!(
            saved_content.contains("[permissions.auto]"),
            "canonical auto permission table should be persisted. Got:\n{saved_content}"
        );
        assert!(
            !saved_content.contains("auto_permission"),
            "legacy auto_permission table should be removed. Got:\n{saved_content}"
        );

        let reloaded = ConfigManager::load_from_file(&config_path).expect("reload saved config");
        assert_eq!(reloaded.config().permissions.auto_permission.model, "legacy-reviewer");
        assert_eq!(reloaded.config().permissions.auto_permission.max_consecutive_denials, 2);
    }

    #[test]
    fn save_config_removes_legacy_permissions_auto_permission_when_sparse_default() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("vtcode.toml");
        fs::write(
            &config_path,
            r#"
[permissions.auto_permission]
max_consecutive_denials = 3
"#,
        )
        .expect("write legacy config");

        ConfigManager::save_config_to_path(&config_path, &VTCodeConfig::default()).expect("save config");

        let saved_content = fs::read_to_string(&config_path).expect("read saved config");
        assert!(
            !saved_content.contains("auto_permission"),
            "legacy auto_permission table should be removed. Got:\n{saved_content}"
        );
        assert!(
            !saved_content.contains("[permissions.auto]"),
            "default auto permission config should remain sparse. Got:\n{saved_content}"
        );

        ConfigManager::load_from_file(&config_path).expect("reload saved config");
    }

    #[test]
    fn inaccessible_optional_global_layers_are_skipped() {
        let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(should_skip_optional_global_error(
            &ConfigLayerSource::System { file: PathBuf::from("/etc/vtcode/vtcode.toml") },
            &error
        ));
        assert!(should_skip_optional_global_error(
            &ConfigLayerSource::User {
                file: PathBuf::from("/home/user/.config/vtcode/vtcode.toml")
            },
            &error
        ));
        assert!(!should_skip_optional_global_error(
            &ConfigLayerSource::Workspace { file: PathBuf::from("/workspace/vtcode.toml") },
            &error
        ));
    }

    #[test]
    fn disabled_layer_reason_tracks_concrete_error_type() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let parse_path = temp_dir.path().join("invalid.toml");
        fs::write(&parse_path, "key = [").expect("write invalid TOML");
        let parse_error = ConfigManager::load_toml_from_file(&parse_path).expect_err("invalid TOML should fail");
        let parse_layer =
            ConfigManager::disabled_layer_from_error(ConfigLayerSource::Project { file: parse_path }, parse_error);

        assert_eq!(parse_layer.disabled_reason, Some(LayerDisabledReason::ParseError));

        let read_path = temp_dir.path().join("parse");
        fs::create_dir(&read_path).expect("create directory at config path");
        let read_error = ConfigManager::load_toml_from_file(&read_path).expect_err("reading a directory should fail");
        assert!(read_error.to_string().contains("parse"));
        let read_layer =
            ConfigManager::disabled_layer_from_error(ConfigLayerSource::Project { file: read_path }, read_error);

        assert_eq!(read_layer.disabled_reason, Some(LayerDisabledReason::LoadError));
    }
}

#[cfg(test)]
mod workspace_lifecycle_hooks_tests {
    use super::*;
    use crate::hooks::WorkspaceLifecycleHooks;

    fn layer(source: ConfigLayerSource, content: &str) -> ConfigLayerEntry {
        ConfigLayerEntry::new(source, toml::from_str(content).expect("valid test toml"))
    }

    fn collect(stack: &ConfigLayerStack) -> WorkspaceLifecycleHooks {
        let (effective_toml, _) = stack.effective_config_with_origins();
        let config: VTCodeConfig = effective_toml.try_into().expect("effective config parses");
        ConfigManager::collect_workspace_lifecycle_hooks(stack, &config.hooks)
    }

    const WS_SESSION_START: &str = r#"
[[hooks.lifecycle.session_start]]
matcher = "startup"

[[hooks.lifecycle.session_start.hooks]]
command = "echo workspace-session"
timeout_seconds = 30
"#;

    const USER_SESSION_END: &str = r#"
[[hooks.lifecycle.session_end]]
[[hooks.lifecycle.session_end.hooks]]
command = "echo user-session-end"
"#;

    #[test]
    fn workspace_layer_hooks_are_collected_user_hooks_are_not() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(ConfigLayerSource::User { file: "/home/u/.vtcode/vtcode.toml".into() }, USER_SESSION_END));
        stack.push(layer(ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() }, WS_SESSION_START));

        let collected = collect(&stack);

        assert_eq!(collected.commands.len(), 1, "only workspace hooks should be collected");
        let command = &collected.commands[0];
        assert_eq!(command.event, "session_start");
        assert_eq!(command.command, "echo workspace-session");
        assert_eq!(command.matcher.as_deref(), Some("startup"));
        assert_eq!(command.timeout_seconds, Some(30));
        assert!(!collected.is_empty());
    }

    #[test]
    fn user_only_hooks_are_not_workspace_controlled() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(ConfigLayerSource::User { file: "/home/u/.vtcode/vtcode.toml".into() }, USER_SESSION_END));

        let collected = collect(&stack);

        assert!(collected.is_empty());
    }

    #[test]
    fn deprecated_task_completion_aliases_fold_into_stop() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(
            ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() },
            r#"
[[hooks.lifecycle.task_completion]]
[[hooks.lifecycle.task_completion.hooks]]
command = "echo ws-task-completion"
"#,
        ));

        let collected = collect(&stack);

        assert_eq!(collected.commands.len(), 1);
        assert_eq!(collected.commands[0].event, "stop");
        assert_eq!(collected.commands[0].command, "echo ws-task-completion");
    }

    #[test]
    fn empty_workspace_hook_arrays_do_not_flag_content() {
        let mut stack = ConfigLayerStack::default();
        stack.push(layer(
            ConfigLayerSource::Workspace { file: "/ws/vtcode.toml".into() },
            "[hooks.lifecycle]\nsession_start = []\n",
        ));

        let collected = collect(&stack);

        assert!(collected.is_empty(), "an empty workspace hook array is not executable content");
    }

    #[test]
    fn load_from_file_populates_workspace_lifecycle_hooks() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("vtcode.toml");
        fs::write(&config_path, WS_SESSION_START).expect("write workspace config");

        let manager = ConfigManager::load_from_file(&config_path).expect("load config");
        let workspace_hooks = manager
            .config()
            .workspace_lifecycle_hooks
            .as_ref()
            .expect("workspace hooks populated at load");

        assert!(!workspace_hooks.is_empty());
        assert_eq!(workspace_hooks.commands.len(), 1);
        assert_eq!(workspace_hooks.commands[0].command, "echo workspace-session");
    }
}
