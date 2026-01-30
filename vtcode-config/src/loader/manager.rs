use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::defaults::{self};
use crate::loader::config::VTCodeConfig;
use crate::loader::layers::{ConfigLayerEntry, ConfigLayerSource, ConfigLayerStack};

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
    /// Load configuration from the default locations
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

        if let Ok(workspace_path) = std::env::var("VTCODE_WORKSPACE") {
            let trimmed = workspace_path.trim();
            if !trimmed.is_empty() {
                return Self::load_from_workspace(trimmed).with_context(|| {
                    format!(
                        "Failed to load configuration from VTCODE_WORKSPACE={}",
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
        let workspace_root = workspace_paths.workspace_root().to_path_buf();
        let config_dir = workspace_paths.config_dir();
        let config_file_name = defaults_provider.config_file_name().to_string();

        let mut layer_stack = ConfigLayerStack::default();

        // 1. System config (e.g., /etc/vtcode/vtcode.toml)
        #[cfg(unix)]
        {
            let system_config = PathBuf::from("/etc/vtcode/vtcode.toml");
            if system_config.exists()
                && let Ok(toml) = Self::load_toml_from_file(&system_config)
            {
                layer_stack.push(ConfigLayerEntry::new(
                    ConfigLayerSource::System {
                        file: system_config,
                    },
                    toml,
                ));
            }
        }

        // 2. User home config (~/.vtcode/vtcode.toml)
        for home_config_path in defaults_provider.home_config_paths(&config_file_name) {
            if home_config_path.exists()
                && let Ok(toml) = Self::load_toml_from_file(&home_config_path)
            {
                layer_stack.push(ConfigLayerEntry::new(
                    ConfigLayerSource::User {
                        file: home_config_path,
                    },
                    toml,
                ));
            }
        }

        // 2. Project-specific config (.vtcode/projects/<project>/config/vtcode.toml)
        if let Some(project_config_path) =
            Self::project_config_path(&config_dir, &workspace_root, &config_file_name)
            && let Ok(toml) = Self::load_toml_from_file(&project_config_path)
        {
            layer_stack.push(ConfigLayerEntry::new(
                ConfigLayerSource::Project {
                    file: project_config_path,
                },
                toml,
            ));
        }

        // 3. Config directory fallback (.vtcode/vtcode.toml)
        let fallback_path = config_dir.join(&config_file_name);
        let workspace_config_path = workspace_root.join(&config_file_name);
        if fallback_path.exists()
            && fallback_path != workspace_config_path
            && let Ok(toml) = Self::load_toml_from_file(&fallback_path)
        {
            layer_stack.push(ConfigLayerEntry::new(
                ConfigLayerSource::Workspace {
                    file: fallback_path,
                },
                toml,
            ));
        }

        // 4. Workspace config (vtcode.toml in workspace root)
        if workspace_config_path.exists()
            && let Ok(toml) = Self::load_toml_from_file(&workspace_config_path)
        {
            layer_stack.push(ConfigLayerEntry::new(
                ConfigLayerSource::Workspace {
                    file: workspace_config_path.clone(),
                },
                toml,
            ));
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

        let effective_toml = layer_stack.effective_config();
        let config: VTCodeConfig = effective_toml
            .try_into()
            .context("Failed to deserialize effective configuration")?;

        config
            .validate()
            .context("Configuration failed validation")?;

        let config_path = layer_stack.layers().last().and_then(|l| match &l.source {
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
            if system_config.exists()
                && let Ok(toml) = Self::load_toml_from_file(&system_config)
            {
                layer_stack.push(ConfigLayerEntry::new(
                    ConfigLayerSource::System {
                        file: system_config,
                    },
                    toml,
                ));
            }
        }

        // 2. User home config
        for home_config_path in defaults_provider.home_config_paths(&config_file_name) {
            if home_config_path.exists()
                && let Ok(toml) = Self::load_toml_from_file(&home_config_path)
            {
                layer_stack.push(ConfigLayerEntry::new(
                    ConfigLayerSource::User {
                        file: home_config_path,
                    },
                    toml,
                ));
            }
        }

        // 3. The specific file provided (Workspace layer)
        let toml = Self::load_toml_from_file(path)?;
        layer_stack.push(ConfigLayerEntry::new(
            ConfigLayerSource::Workspace {
                file: path.to_path_buf(),
            },
            toml,
        ));

        let effective_toml = layer_stack.effective_config();
        let config: VTCodeConfig = effective_toml.try_into().with_context(|| {
            format!(
                "Failed to parse effective config with file: {}",
                path.display()
            )
        })?;

        config.validate().with_context(|| {
            format!(
                "Failed to validate effective config with file: {}",
                path.display()
            )
        })?;

        Ok(Self {
            config,
            config_path: Some(path.to_path_buf()),
            workspace_root: path.parent().map(Path::to_path_buf),
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

        // If file exists, preserve comments by using toml_edit
        if path.exists() {
            let original_content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read existing config: {}", path.display()))?;

            let mut doc = original_content
                .parse::<toml_edit::DocumentMut>()
                .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;

            // Serialize new config to TOML value
            let new_value =
                toml::to_string_pretty(config).context("Failed to serialize configuration")?;
            let new_doc: toml_edit::DocumentMut = new_value
                .parse()
                .context("Failed to parse serialized configuration")?;

            // Update values while preserving structure and comments
            Self::merge_toml_documents(&mut doc, &new_doc);

            fs::write(path, doc.to_string())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        } else {
            // New file, just write normally
            let content =
                toml::to_string_pretty(config).context("Failed to serialize configuration")?;
            fs::write(path, content)
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }

        Ok(())
    }

    /// Merge TOML documents, preserving comments and structure from original
    fn merge_toml_documents(original: &mut toml_edit::DocumentMut, new: &toml_edit::DocumentMut) {
        for (key, new_value) in new.iter() {
            if let Some(original_value) = original.get_mut(key) {
                Self::merge_toml_items(original_value, new_value);
            } else {
                original[key] = new_value.clone();
            }
        }
    }

    /// Recursively merge TOML items
    fn merge_toml_items(original: &mut toml_edit::Item, new: &toml_edit::Item) {
        match (original, new) {
            (toml_edit::Item::Table(orig_table), toml_edit::Item::Table(new_table)) => {
                for (key, new_value) in new_table.iter() {
                    if let Some(orig_value) = orig_table.get_mut(key) {
                        Self::merge_toml_items(orig_value, new_value);
                    } else {
                        orig_table[key] = new_value.clone();
                    }
                }
            }
            (orig, new) => {
                *orig = new.clone();
            }
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
