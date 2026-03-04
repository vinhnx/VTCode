use crate::loader::merge::{merge_toml_values, merge_toml_values_with_origins};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use toml::Value as TomlValue;

use super::fingerprint::fingerprint_toml_value;

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

impl ConfigLayerSource {
    /// Lower numbers are lower precedence.
    pub const fn precedence(&self) -> i16 {
        match self {
            Self::System { .. } => 10,
            Self::User { .. } => 20,
            Self::Project { .. } => 25,
            Self::Workspace { .. } => 30,
            Self::Runtime => 40,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::System { file } => format!("system:{}", file.display()),
            Self::User { file } => format!("user:{}", file.display()),
            Self::Project { file } => format!("project:{}", file.display()),
            Self::Workspace { file } => format!("workspace:{}", file.display()),
            Self::Runtime => "runtime".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigLayerMetadata {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerDisabledReason {
    ParseError,
    LoadError,
    UntrustedWorkspace,
    PolicyDisabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigLayerLoadError {
    pub message: String,
}

/// A single layer of configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigLayerEntry {
    /// Source of this layer
    pub source: ConfigLayerSource,
    /// Stable metadata for this layer
    pub metadata: ConfigLayerMetadata,
    /// Parsed TOML content
    pub config: TomlValue,
    /// Optional reason the layer was disabled
    pub disabled_reason: Option<LayerDisabledReason>,
    /// Optional error attached to this layer
    pub error: Option<ConfigLayerLoadError>,
}

impl ConfigLayerEntry {
    /// Create a new configuration layer entry.
    pub fn new(source: ConfigLayerSource, config: TomlValue) -> Self {
        let metadata = ConfigLayerMetadata {
            name: source.label(),
            version: fingerprint_toml_value(&config),
        };
        Self {
            source,
            metadata,
            config,
            disabled_reason: None,
            error: None,
        }
    }

    /// Create a disabled layer entry while retaining layer metadata.
    pub fn disabled(
        source: ConfigLayerSource,
        reason: LayerDisabledReason,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let config = TomlValue::Table(toml::Table::new());
        let metadata = ConfigLayerMetadata {
            name: source.label(),
            version: fingerprint_toml_value(&TomlValue::String(format!(
                "{}:{}",
                source.label(),
                message
            ))),
        };
        Self {
            source,
            metadata,
            config,
            disabled_reason: Some(reason),
            error: Some(ConfigLayerLoadError { message }),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.disabled_reason.is_none() && self.error.is_none()
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
        self.effective_config_with_origins().0
    }

    /// Merge all layers and return an origin map (`path -> winning layer metadata`).
    pub fn effective_config_with_origins(
        &self,
    ) -> (TomlValue, HashMap<String, ConfigLayerMetadata>) {
        let mut merged = TomlValue::Table(toml::Table::new());
        let mut origins = HashMap::new();
        for layer in self.ordered_enabled_layers() {
            merge_toml_values_with_origins(
                &mut merged,
                &layer.config,
                &mut origins,
                &layer.metadata,
            );
        }
        (merged, origins)
    }

    /// Return the first layer error in precedence order.
    pub fn first_layer_error(&self) -> Option<(&ConfigLayerEntry, &ConfigLayerLoadError)> {
        for layer in self.ordered_layers() {
            if let Some(error) = layer.error.as_ref() {
                return Some((layer, error));
            }
        }
        None
    }

    /// Merge all enabled layers without origin tracking.
    pub fn effective_config_without_origins(&self) -> TomlValue {
        let mut merged = TomlValue::Table(toml::Table::new());
        for layer in self.ordered_enabled_layers() {
            merge_toml_values(&mut merged, &layer.config);
        }
        merged
    }

    fn ordered_layers(&self) -> Vec<&ConfigLayerEntry> {
        let mut with_index: Vec<(usize, &ConfigLayerEntry)> =
            self.layers.iter().enumerate().collect();
        with_index.sort_by(|(left_idx, left), (right_idx, right)| {
            left.source
                .precedence()
                .cmp(&right.source.precedence())
                .then(left_idx.cmp(right_idx))
        });
        with_index.into_iter().map(|(_, layer)| layer).collect()
    }

    fn ordered_enabled_layers(&self) -> Vec<&ConfigLayerEntry> {
        self.ordered_layers()
            .into_iter()
            .filter(|layer| layer.is_enabled())
            .collect()
    }

    /// Get all layers in the stack.
    pub fn layers(&self) -> &[ConfigLayerEntry] {
        &self.layers
    }
}
