use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::loader::layers::{ConfigLayerEntry, ConfigLayerSource};
use crate::loader::manager::ConfigManager;

/// Builder for creating a [`ConfigManager`] with custom overrides.
#[derive(Debug, Clone, Default)]
pub struct ConfigBuilder {
    workspace: Option<PathBuf>,
    config_file: Option<PathBuf>,
    cli_overrides: Vec<(String, toml::Value)>,
}

impl ConfigBuilder {
    /// Create a new configuration builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the workspace directory.
    pub fn workspace(mut self, path: PathBuf) -> Self {
        self.workspace = Some(path);
        self
    }

    /// Set a specific configuration file to use instead of the default workspace config.
    pub fn config_file(mut self, path: PathBuf) -> Self {
        self.config_file = Some(path);
        self
    }

    /// Add a CLI override (e.g., "agent.provider", "openai").
    pub fn cli_override(mut self, key: String, value: toml::Value) -> Self {
        self.cli_overrides.push((key, value));
        self
    }

    /// Add multiple CLI overrides from string pairs.
    ///
    /// Values are parsed as TOML. If parsing fails, they are treated as strings.
    pub fn cli_overrides(mut self, overrides: &[(String, String)]) -> Self {
        for (key, value) in overrides {
            let toml_value = value
                .parse::<toml::Value>()
                .unwrap_or_else(|_| toml::Value::String(value.clone()));
            self.cli_overrides.push((key.clone(), toml_value));
        }
        self
    }

    /// Build the [`ConfigManager`].
    pub fn build(self) -> Result<ConfigManager> {
        let workspace = self
            .workspace
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let mut manager = if let Some(config_file) = self.config_file {
            ConfigManager::load_from_file(config_file)?
        } else {
            ConfigManager::load_from_workspace(workspace)?
        };

        if !self.cli_overrides.is_empty() {
            let mut runtime_toml = toml::Table::new();
            for (key, value) in self.cli_overrides {
                Self::insert_dotted_key(&mut runtime_toml, &key, value);
            }

            let runtime_layer =
                ConfigLayerEntry::new(ConfigLayerSource::Runtime, toml::Value::Table(runtime_toml));

            manager.layer_stack.push(runtime_layer);

            // Re-evaluate config
            let effective_toml = manager.layer_stack.effective_config();
            manager.config = effective_toml
                .try_into()
                .context("Failed to deserialize effective configuration after runtime overrides")?;
            manager
                .config
                .validate()
                .context("Configuration failed validation after runtime overrides")?;
        }

        Ok(manager)
    }

    fn insert_dotted_key(table: &mut toml::Table, key: &str, value: toml::Value) {
        let parts: Vec<&str> = key.split('.').collect();
        let mut current = table;
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                current.insert(part.to_string(), value);
                return;
            }

            if !current.contains_key(*part) || !current[*part].is_table() {
                current.insert(part.to_string(), toml::Value::Table(toml::Table::new()));
            }

            current = current
                .get_mut(*part)
                .and_then(|v| v.as_table_mut())
                .expect("Value must be a table");
        }
    }
}
