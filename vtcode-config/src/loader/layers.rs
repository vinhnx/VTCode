use crate::loader::merge_toml_values;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use toml::Value as TomlValue;

/// Source of a configuration layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConfigLayerSource {
    /// System-wide configuration (e.g., /etc/vtcode/vtcode.toml)
    System { file: PathBuf },
    /// User-specific configuration (e.g., ~/.vtcode/vtcode.toml)
    User { file: PathBuf },
    /// Project-specific configuration (e.g., .vtcode/projects/foo/config/vtcode.toml)
    Project { file: PathBuf },
    /// Workspace-specific configuration (e.g., vtcode.toml in workspace root)
    Workspace { file: PathBuf },
    /// Runtime overrides (e.g., CLI flags)
    Runtime,
}

/// A single layer of configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigLayerEntry {
    /// Source of this layer
    pub source: ConfigLayerSource,
    /// Parsed TOML content
    pub config: TomlValue,
}

impl ConfigLayerEntry {
    /// Create a new configuration layer entry.
    pub fn new(source: ConfigLayerSource, config: TomlValue) -> Self {
        Self { source, config }
    }
}

/// A stack of configuration layers, ordered from lowest to highest precedence.
#[derive(Debug, Clone, Default)]
pub struct ConfigLayerStack {
    layers: Vec<ConfigLayerEntry>,
}

impl ConfigLayerStack {
    /// Create a new configuration layer stack.
    pub fn new(layers: Vec<ConfigLayerEntry>) -> Self {
        Self { layers }
    }

    /// Add a layer to the stack.
    pub fn push(&mut self, layer: ConfigLayerEntry) {
        self.layers.push(layer);
    }

    /// Merge all layers into a single effective configuration.
    pub fn effective_config(&self) -> TomlValue {
        let mut merged = TomlValue::Table(toml::Table::new());
        for layer in &self.layers {
            merge_toml_values(&mut merged, &layer.config);
        }
        merged
    }

    /// Get all layers in the stack.
    pub fn layers(&self) -> &[ConfigLayerEntry] {
        &self.layers
    }
}
