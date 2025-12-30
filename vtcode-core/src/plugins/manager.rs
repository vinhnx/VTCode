//! Plugin manager for VT Code
//!
//! Coordinates all plugin system components and provides a unified interface
//! for plugin management.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use super::{PluginCache, PluginLoader, PluginManifest, PluginResult, PluginRuntime};
use crate::config::PluginRuntimeConfig;

/// Main plugin manager that coordinates all plugin system components
pub struct PluginManager {
    /// Plugin runtime for managing loaded plugins
    runtime: Arc<PluginRuntime>,
    /// Plugin loader for installing/uninstalling plugins
    loader: Arc<PluginLoader>,
    /// Plugin cache for security and verification
    cache: Arc<RwLock<PluginCache>>,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new(config: PluginRuntimeConfig, base_dir: PathBuf) -> Result<Self> {
        let runtime = Arc::new(PluginRuntime::new(config.clone(), base_dir.join("runtime")));
        let cache = Arc::new(RwLock::new(PluginCache::new(base_dir.join("cache"))));

        let loader = Arc::new(PluginLoader::new(
            base_dir.join("installed"),
            runtime.as_ref().clone(),
        ));

        Ok(Self {
            runtime,
            loader,
            cache,
        })
    }

    /// Install a plugin from a source
    pub async fn install_plugin(
        &self,
        source: super::loader::PluginSource,
        name: Option<String>,
    ) -> PluginResult<()> {
        // Install the plugin using the loader
        self.loader.install_plugin(source, name).await?;
        Ok(())
    }

    /// Uninstall a plugin
    pub async fn uninstall_plugin(&self, plugin_name: &str) -> PluginResult<()> {
        // Uninstall the plugin using the loader
        self.loader.uninstall_plugin(plugin_name).await?;
        Ok(())
    }

    /// Enable a plugin
    pub async fn enable_plugin(&self, plugin_name: &str) -> PluginResult<()> {
        self.runtime.enable_plugin(plugin_name).await?;
        Ok(())
    }

    /// Disable a plugin
    pub async fn disable_plugin(&self, plugin_name: &str) -> PluginResult<()> {
        self.runtime.disable_plugin(plugin_name).await?;
        Ok(())
    }

    /// Load a plugin
    pub async fn load_plugin(&self, plugin_path: &std::path::Path) -> PluginResult<()> {
        self.runtime.load_plugin(plugin_path).await?;
        Ok(())
    }

    /// Get information about a plugin
    pub async fn get_plugin(&self, plugin_id: &str) -> PluginResult<super::runtime::PluginHandle> {
        self.runtime.get_plugin(plugin_id).await
    }

    /// List all installed plugins
    pub async fn list_installed_plugins(&self) -> PluginResult<Vec<String>> {
        self.loader.list_installed_plugins().await
    }

    /// List all loaded plugins
    pub async fn list_loaded_plugins(&self) -> Vec<super::runtime::PluginHandle> {
        self.runtime.list_plugins().await
    }

    /// Process all components for a plugin
    pub async fn process_plugin_components(
        &self,
        plugin_path: &std::path::Path,
        manifest: &PluginManifest,
    ) -> Result<super::components::PluginComponents> {
        super::components::PluginComponentsHandler::process_all_components(plugin_path, manifest)
            .await
    }

    /// Check if a plugin is enabled
    pub async fn is_plugin_enabled(&self, plugin_id: &str) -> bool {
        self.runtime.is_plugin_enabled(plugin_id).await
    }

    /// Cache a plugin for security
    pub async fn cache_plugin(
        &self,
        plugin_id: &str,
        source_path: &std::path::Path,
    ) -> PluginResult<std::path::PathBuf> {
        let mut cache = self.cache.write().await;
        cache.cache_plugin(plugin_id, source_path).await
    }

    /// Get cached plugin path
    pub async fn get_cached_plugin(&self, plugin_id: &str) -> Option<std::path::PathBuf> {
        let cache = self.cache.read().await;
        cache.get_cached_plugin(plugin_id).cloned()
    }
}
