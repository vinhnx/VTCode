use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::auth::migrate_custom_api_keys_to_keyring;
use crate::defaults::{self};
use crate::loader::config::VTCodeConfig;
use crate::loader::layers::{
    ConfigLayerEntry, ConfigLayerMetadata, ConfigLayerSource, ConfigLayerStack, LayerDisabledReason,
};

fn canonicalize_workspace_root(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Configuration manager for loading and validating configurations
#[derive(Clone)]
pub struct ConfigManager {
    pub(crate) config: VTCodeConfig,
    config_path: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
    config_file_name: String,
    pub(crate) layer_stack: ConfigLayerStack,
}

impl ConfigManager {
    /// Load configuration from the default locations rooted at the current directory.
    pub fn load() -> Result<Self> {
        if let Ok(config_path) = std::env::var("VTCODE_CONFIG_PATH") {
            let trimmed = config_path.trim();
            if !trimmed.is_empty() {
                return Self::load_from_file(trimmed).with_context(|| {
                    format!(
                        "Failed to load configuration from VTCODE_CONFIG_PATH={}",
                        trimmed
                    )
                });
            }
        }

        Self::load_from_workspace(std::env::current_dir()?)
    }

    /// Load configuration from a specific workspace
    pub fn load_from_workspace(workspace: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref();
        let defaults_provider = defaults::current_config_defaults();
        let workspace_paths = defaults_provider.workspace_paths_for(workspace);
        let workspace_root = canonicalize_workspace_root(workspace_paths.workspace_root());
        let config_dir = workspace_paths.config_dir();
        let config_file_name = defaults_provider.config_file_name().to_string();

        let mut layer_stack = ConfigLayerStack::default();

        // 1. System config (e.g., /etc/vtcode/vtcode.toml)
        #[cfg(unix)]
        {
            let system_config = PathBuf::from("/etc/vtcode/vtcode.toml");
            if let Some(layer) = Self::load_optional_layer(ConfigLayerSource::System {
                file: system_config,
            }) {
                layer_stack.push(layer);
            }
        }

        // 2. User home config (~/.vtcode/vtcode.toml)
        for home_config_path in defaults_provider.home_config_paths(&config_file_name) {
            if let Some(layer) = Self::load_optional_layer(ConfigLayerSource::User {
                file: home_config_path,
            }) {
                layer_stack.push(layer);
            }
        }

        // 3. Project-specific config (.vtcode/projects/<project>/config/vtcode.toml)
        if let Some(project_config_path) =
            Self::project_config_path(&config_dir, &workspace_root, &config_file_name)
            && let Some(layer) = Self::load_optional_layer(ConfigLayerSource::Project {
                file: project_config_path,
            })
        {
            layer_stack.push(layer);
        }

        // 4. Config directory fallback (.vtcode/vtcode.toml)
        let fallback_path = config_dir.join(&config_file_name);
        let workspace_config_path = workspace_root.join(&config_file_name);
        if fallback_path.exists()
            && fallback_path != workspace_config_path
            && let Some(layer) = Self::load_optional_layer(ConfigLayerSource::Workspace {
                file: fallback_path,
            })
        {
            layer_stack.push(layer);
        }

        // 5. Workspace config (vtcode.toml in workspace root)
        if let Some(layer) = Self::load_optional_layer(ConfigLayerSource::Workspace {
            file: workspace_config_path.clone(),
        }) {
            layer_stack.push(layer);
        }

        // If no layers found, use default config
        if layer_stack.layers().is_empty() {
            let config = VTCodeConfig::default();
            config
                .validate()
                .context("Default configuration failed validation")?;

            return Ok(Self {
                config,
                config_path: None,
                workspace_root: Some(workspace_root),
                config_file_name,
                layer_stack,
            });
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!(
                "Configuration layer '{}' failed to load: {}",
                layer.source.label(),
                error.message
            );
        }

        let (effective_toml, origins) = layer_stack.effective_config_with_origins();
        let mut config: VTCodeConfig = effective_toml
            .try_into()
            .context("Failed to deserialize effective configuration")?;
        Self::validate_restricted_agent_fields(&layer_stack, &origins)?;

        config
            .validate()
            .context("Configuration failed validation")?;

        // Migrate any plain-text API keys from config to secure storage
        migrate_custom_api_keys_if_needed(&mut config)?;

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
            workspace_root: Some(workspace_root),
            config_file_name,
            layer_stack,
        })
    }

    fn load_toml_from_file(path: &Path) -> Result<toml::Value> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let value: toml::Value = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(value)
    }

    fn load_optional_layer(source: ConfigLayerSource) -> Option<ConfigLayerEntry> {
        let file = match &source {
            ConfigLayerSource::System { file }
            | ConfigLayerSource::User { file }
            | ConfigLayerSource::Project { file }
            | ConfigLayerSource::Workspace { file } => file,
            ConfigLayerSource::Runtime => {
                return Some(ConfigLayerEntry::new(
                    source,
                    toml::Value::Table(toml::Table::new()),
                ));
            }
        };

        if !file.exists() {
            return None;
        }

        let resolved_file = canonicalize_workspace_root(file);
        let resolved_source = match source {
            ConfigLayerSource::System { .. } => ConfigLayerSource::System {
                file: resolved_file.clone(),
            },
            ConfigLayerSource::User { .. } => ConfigLayerSource::User {
                file: resolved_file.clone(),
            },
            ConfigLayerSource::Project { .. } => ConfigLayerSource::Project {
                file: resolved_file.clone(),
            },
            ConfigLayerSource::Workspace { .. } => ConfigLayerSource::Workspace {
                file: resolved_file.clone(),
            },
            ConfigLayerSource::Runtime => unreachable!(),
        };

        match Self::load_toml_from_file(&resolved_file) {
            Ok(toml) => Some(ConfigLayerEntry::new(resolved_source, toml)),
            Err(error) => Some(Self::disabled_layer_from_error(resolved_source, error)),
        }
    }

    fn disabled_layer_from_error(
        source: ConfigLayerSource,
        error: anyhow::Error,
    ) -> ConfigLayerEntry {
        let reason = if error.to_string().contains("parse") {
            LayerDisabledReason::ParseError
        } else {
            LayerDisabledReason::LoadError
        };
        ConfigLayerEntry::disabled(source, reason, format!("{:#}", error))
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let defaults_provider = defaults::current_config_defaults();
        let config_file_name = path
            .file_name()
            .and_then(|name| name.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| defaults_provider.config_file_name().to_string());

        let mut layer_stack = ConfigLayerStack::default();

        // 1. System config
        #[cfg(unix)]
        {
            let system_config = PathBuf::from("/etc/vtcode/vtcode.toml");
            if let Some(layer) = Self::load_optional_layer(ConfigLayerSource::System {
                file: system_config,
            }) {
                layer_stack.push(layer);
            }
        }

        // 2. User home config
        for home_config_path in defaults_provider.home_config_paths(&config_file_name) {
            if let Some(layer) = Self::load_optional_layer(ConfigLayerSource::User {
                file: home_config_path,
            }) {
                layer_stack.push(layer);
            }
        }

        // 3. The specific file provided (Workspace layer)
        match Self::load_toml_from_file(path) {
            Ok(toml) => layer_stack.push(ConfigLayerEntry::new(
                ConfigLayerSource::Workspace {
                    file: path.to_path_buf(),
                },
                toml,
            )),
            Err(error) => layer_stack.push(Self::disabled_layer_from_error(
                ConfigLayerSource::Workspace {
                    file: path.to_path_buf(),
                },
                error,
            )),
        }

        if let Some((layer, error)) = layer_stack.first_layer_error() {
            bail!(
                "Configuration layer '{}' failed to load: {}",
                layer.source.label(),
                error.message
            );
        }

        let (effective_toml, origins) = layer_stack.effective_config_with_origins();
        let config: VTCodeConfig = effective_toml.try_into().with_context(|| {
            format!(
                "Failed to parse effective config with file: {}",
                path.display()
            )
        })?;
        Self::validate_restricted_agent_fields(&layer_stack, &origins)?;

        config.validate().with_context(|| {
            format!(
                "Failed to validate effective config with file: {}",
                path.display()
            )
        })?;

        Ok(Self {
            config,
            config_path: Some(canonicalize_workspace_root(path)),
            workspace_root: path.parent().map(canonicalize_workspace_root),
            config_file_name,
            layer_stack,
        })
    }

    /// Get the loaded configuration
    pub fn config(&self) -> &VTCodeConfig {
        &self.config
    }

    /// Get the configuration file path (if loaded from file)
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Get the active workspace root for this manager.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Get the config filename used by this manager (usually `vtcode.toml`).
    pub fn config_file_name(&self) -> &str {
        &self.config_file_name
    }

    /// Get the configuration layer stack
    pub fn layer_stack(&self) -> &ConfigLayerStack {
        &self.layer_stack
    }

    /// Get the effective TOML configuration
    pub fn effective_config(&self) -> toml::Value {
        self.layer_stack.effective_config()
    }

    /// Get session duration from agent config
    pub fn session_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60 * 60) // Default 1 hour
    }

    /// Persist configuration to a specific path, preserving comments
    pub fn save_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        let path = path.as_ref();
        let sparse_value =
            Self::sparse_config_value(config).context("Failed to prepare sparse configuration")?;
        let sparse_content = toml::to_string_pretty(&sparse_value)
            .context("Failed to serialize sparse configuration")?;

        // If file exists, preserve comments by using toml_edit
        if path.exists() {
            let original_content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read existing config: {}", path.display()))?;

            let mut doc = original_content
                .parse::<toml_edit::DocumentMut>()
                .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;
            Self::remove_deprecated_config_keys(&mut doc);

            let new_doc: toml_edit::DocumentMut = sparse_content
                .parse()
                .context("Failed to parse sparse serialized configuration")?;
            let default_value = toml::Value::try_from(VTCodeConfig::default())
                .context("Failed to serialize default configuration")?;
            let default_doc: toml_edit::DocumentMut = toml::to_string_pretty(&default_value)
                .context("Failed to serialize default configuration")?
                .parse()
                .context("Failed to parse default serialized configuration")?;

            // Update values while preserving structure and comments
            Self::merge_sparse_toml_documents(&mut doc, &new_doc, &default_doc);

            fs::write(path, doc.to_string())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        } else {
            fs::write(path, sparse_content)
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }

        Ok(())
    }

    fn remove_deprecated_config_keys(doc: &mut toml_edit::DocumentMut) {
        let table = doc.as_table_mut();
        table.remove("project_doc_max_bytes");
        table.remove("project_doc_fallback_filenames");
        Self::remove_table_keys(table, "agent", &["autonomous_mode", "default_editing_mode"]);
        Self::remove_table_keys(table, "permissions", &["allowed_tools", "disallowed_tools"]);
    }

    fn remove_table_keys(table: &mut toml_edit::Table, section: &str, keys: &[&str]) {
        let Some(section) = table
            .get_mut(section)
            .and_then(toml_edit::Item::as_table_mut)
        else {
            return;
        };

        for key in keys {
            section.remove(key);
        }
    }

    pub fn sparse_config_value(config: &VTCodeConfig) -> Result<toml::Value> {
        let mut value =
            toml::Value::try_from(config).context("Failed to serialize configuration")?;
        let default_value = toml::Value::try_from(VTCodeConfig::default())
            .context("Failed to serialize default configuration")?;
        Self::prune_default_values(&mut value, &default_value);
        Ok(value)
    }

    fn prune_default_values(value: &mut toml::Value, default_value: &toml::Value) -> bool {
        match (value, default_value) {
            (toml::Value::Table(table), toml::Value::Table(default_table)) => {
                table.retain(|key, child| {
                    default_table.get(key).is_none_or(|default_child| {
                        !Self::prune_default_values(child, default_child)
                    })
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
        Self::merge_sparse_tables(
            original.as_table_mut(),
            new.as_table(),
            default_doc.as_table(),
        );
    }

    fn merge_sparse_tables(
        original: &mut toml_edit::Table,
        new: &toml_edit::Table,
        default_table: &toml_edit::Table,
    ) {
        let mut remove_keys = Vec::new();

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

    fn merge_sparse_items(
        original: &mut toml_edit::Item,
        new: &toml_edit::Item,
        default_value: &toml_edit::Item,
    ) {
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

    fn remove_known_default_item(
        original: &mut toml_edit::Item,
        default_value: &toml_edit::Item,
    ) -> bool {
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

    fn project_config_path(
        config_dir: &Path,
        workspace_root: &Path,
        config_file_name: &str,
    ) -> Option<PathBuf> {
        let project_name = Self::identify_current_project(workspace_root)?;
        let project_config_path = config_dir
            .join("projects")
            .join(project_name)
            .join("config")
            .join(config_file_name);

        if project_config_path.exists() {
            Some(project_config_path)
        } else {
            None
        }
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
            && let Some(layer) = layer_stack
                .layers()
                .iter()
                .find(|layer| layer.metadata == *origin)
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

    /// Persist configuration to the manager's associated path or workspace
    pub fn save_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        if let Some(path) = &self.config_path {
            Self::save_config_to_path(path, config)?;
        } else if let Some(workspace_root) = &self.workspace_root {
            let path = workspace_root.join(&self.config_file_name);
            Self::save_config_to_path(path, config)?;
        } else {
            let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
            let path = cwd.join(&self.config_file_name);
            Self::save_config_to_path(path, config)?;
        }

        self.sync_from_config(config)
    }

    /// Sync internal config from a saved config
    /// Call this after save_config to keep internal state in sync
    pub fn sync_from_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        self.config = config.clone();
        Ok(())
    }
}

/// Migrate plain-text API keys from config to secure storage.
///
/// This function checks if there are any API keys stored in plain-text in the config
/// and migrates them to secure storage (keyring). After successful migration, the
/// keys are cleared from the config (kept as empty strings for tracking).
///
/// # Arguments
/// * `config` - The configuration to migrate
fn migrate_custom_api_keys_if_needed(config: &mut VTCodeConfig) -> Result<()> {
    let storage_mode = config.agent.credential_storage_mode;

    // Check if there are any non-empty API keys in the config
    let has_plain_text_keys = config
        .agent
        .custom_api_keys
        .values()
        .any(|key| !key.is_empty());

    if has_plain_text_keys {
        tracing::info!("Detected plain-text API keys in config, migrating to secure storage...");

        // Migrate keys to secure storage
        let migration_results =
            migrate_custom_api_keys_to_keyring(&config.agent.custom_api_keys, storage_mode)?;

        // Clear keys from config (keep provider names for tracking)
        let mut migrated_count = 0;
        for (provider, success) in migration_results {
            if success {
                // Replace with empty string to track that this provider has a stored key
                config.agent.custom_api_keys.insert(provider, String::new());
                migrated_count += 1;
            }
        }

        if migrated_count > 0 {
            tracing::info!(
                "Successfully migrated {} API key(s) to secure storage",
                migrated_count
            );
            tracing::warn!(
                "Plain-text API keys have been cleared from config file. \
                 Please commit the updated config to remove sensitive data from version control."
            );
        }
    }

    Ok(())
}
