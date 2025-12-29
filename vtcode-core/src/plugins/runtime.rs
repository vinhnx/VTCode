//! Plugin runtime system for VT Code
//!
//! Manages the lifecycle of plugins including loading, unloading, and execution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;

use super::{PluginError, PluginId, PluginManifest, PluginResult};
use crate::config::PluginRuntimeConfig;

/// Plugin state tracking
#[derive(Debug, Clone, PartialEq)]
pub enum PluginState {
    /// Plugin is loaded and ready
    Active,
    /// Plugin is installed but not loaded
    Installed,
    /// Plugin is disabled
    Disabled,
    /// Plugin is in error state
    Error,
}

/// Plugin handle containing runtime information
#[derive(Debug, Clone)]
pub struct PluginHandle {
    /// Plugin identifier
    pub id: PluginId,
    /// Plugin manifest
    pub manifest: PluginManifest,
    /// Plugin installation path
    pub path: PathBuf,
    /// Current state
    pub state: PluginState,
    /// Loaded at timestamp
    pub loaded_at: Option<std::time::SystemTime>,
}

/// Plugin runtime that manages plugin lifecycle
#[derive(Debug, Clone)]
pub struct PluginRuntime {
    /// Currently loaded plugins
    plugins: Arc<RwLock<HashMap<PluginId, PluginHandle>>>,
}

impl PluginRuntime {
    /// Create a new plugin runtime
    pub fn new(_config: PluginRuntimeConfig, _base_dir: PathBuf) -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load a plugin from the specified path
    pub async fn load_plugin(&self, plugin_path: &Path) -> PluginResult<PluginHandle> {
        // Validate plugin path
        if !plugin_path.exists() {
            return Err(PluginError::NotFound(plugin_path.display().to_string()));
        }

        // Load the plugin manifest
        let manifest = self.load_manifest(plugin_path).await?;

        // Validate the manifest
        self.validate_manifest(&manifest)?;

        // Create plugin handle
        let handle = PluginHandle {
            id: manifest.name.clone(),
            manifest: manifest.clone(),
            path: plugin_path.to_path_buf(),
            state: PluginState::Active,
            loaded_at: Some(std::time::SystemTime::now()),
        };

        // Store in runtime
        {
            let mut plugins = self.plugins.write().await;
            plugins.insert(manifest.name.clone(), handle.clone());
        }

        Ok(handle)
    }

    /// Load plugin manifest from path
    async fn load_manifest(&self, plugin_path: &Path) -> PluginResult<PluginManifest> {
        let manifest_path = plugin_path.join(".vtcode-plugin/plugin.json");

        if !manifest_path.exists() {
            return Err(PluginError::ManifestValidationError(format!(
                "Plugin manifest not found at: {}",
                manifest_path.display()
            )));
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| PluginError::LoadingError(format!("Failed to read manifest: {}", e)))?;

        let manifest: PluginManifest = serde_json::from_str(&manifest_content).map_err(|e| {
            PluginError::ManifestValidationError(format!("Invalid manifest JSON: {}", e))
        })?;

        Ok(manifest)
    }

    /// Validate plugin manifest
    fn validate_manifest(&self, manifest: &PluginManifest) -> PluginResult<()> {
        if manifest.name.is_empty() {
            return Err(PluginError::ManifestValidationError(
                "Plugin name is required".to_string(),
            ));
        }

        // Validate name format (kebab-case)
        if !self.is_valid_plugin_name(&manifest.name) {
            return Err(PluginError::ManifestValidationError(
                "Plugin name must be in kebab-case (lowercase with hyphens)".to_string(),
            ));
        }

        Ok(())
    }

    /// Check if plugin name is valid (kebab-case)
    fn is_valid_plugin_name(&self, name: &str) -> bool {
        // Check if name contains only lowercase letters, numbers, and hyphens
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.is_empty()
    }

    /// Unload a plugin
    pub async fn unload_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        if plugins.remove(plugin_id).is_none() {
            return Err(PluginError::NotFound(plugin_id.to_string()));
        }
        Ok(())
    }

    /// Get a plugin handle
    pub async fn get_plugin(&self, plugin_id: &str) -> PluginResult<PluginHandle> {
        let plugins = self.plugins.read().await;
        plugins
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))
    }

    /// List all loaded plugins
    pub async fn list_plugins(&self) -> Vec<PluginHandle> {
        let plugins = self.plugins.read().await;
        plugins.values().cloned().collect()
    }

    /// Enable a plugin
    pub async fn enable_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        if let Some(handle) = plugins.get_mut(plugin_id) {
            handle.state = PluginState::Active;
            Ok(())
        } else {
            Err(PluginError::NotFound(plugin_id.to_string()))
        }
    }

    /// Disable a plugin
    pub async fn disable_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        if let Some(handle) = plugins.get_mut(plugin_id) {
            handle.state = PluginState::Disabled;
            Ok(())
        } else {
            Err(PluginError::NotFound(plugin_id.to_string()))
        }
    }

    /// Check if a plugin is enabled
    pub async fn is_plugin_enabled(&self, plugin_id: &str) -> bool {
        if let Ok(handle) = self.get_plugin(plugin_id).await {
            handle.state == PluginState::Active
        } else {
            false
        }
    }
}
