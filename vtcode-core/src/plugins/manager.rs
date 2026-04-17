//! Plugin manager for VT Code
//!
//! Coordinates all plugin system components and provides a unified interface
//! for plugin management.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

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
    /// Worker state for non-curated plugin cache refresh
    refresh_worker: Arc<RwLock<RefreshWorkerState>>,
}

/// State tracking for the non-curated plugin cache refresh worker.
/// Prevents race conditions when marking refresh workers as idle.
#[derive(Debug, Default)]
struct RefreshWorkerState {
    /// Whether the refresh worker is currently active
    is_idle: bool,
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
            refresh_worker: Arc::new(RwLock::new(RefreshWorkerState::default())),
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
    pub async fn load_plugin(&self, plugin_path: &Path) -> PluginResult<()> {
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
        plugin_path: &Path,
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
    pub async fn cache_plugin(&self, plugin_id: &str, source_path: &Path) -> PluginResult<PathBuf> {
        let mut cache = self.cache.write().await;
        cache.cache_plugin(plugin_id, source_path).await
    }

    /// Get cached plugin path
    pub async fn get_cached_plugin(&self, plugin_id: &str) -> Option<PathBuf> {
        let cache = self.cache.read().await;
        cache.get_cached_plugin(plugin_id).cloned()
    }

    /// Refresh the non-curated plugin cache using dynamic roots (workspace directories).
    ///
    /// This method is triggered from `plugin/list` commands rather than at startup,
    /// allowing it to access the current working directories (roots/cwds) required
    /// to locate `marketplace.json` files that are not persisted in the config.
    ///
    /// Uses version information from `plugin.json` to determine if a refresh is needed.
    /// The refresh worker state prevents race conditions during concurrent refreshes.
    ///
    /// # Errors
    ///
    /// Returns an error if the refresh worker lock is poisoned or if root scanning fails.
    pub async fn refresh_non_curated_plugin_cache(
        &self,
        roots: &[PathBuf],
    ) -> Result<RefreshResult> {
        // Prevent concurrent refreshes
        {
            let worker = self.refresh_worker.read().await;
            if !worker.is_idle {
                debug!("non-curated plugin cache refresh already in progress, skipping");
                return Ok(RefreshResult::SkippedAlreadyInProgress);
            }
        }

        // Mark busy before dropping read lock
        {
            let mut worker = self.refresh_worker.write().await;
            if !worker.is_idle {
                return Ok(RefreshResult::SkippedAlreadyInProgress);
            }
            worker.is_idle = false;
        }

        let result = self.refresh_non_curated_from_roots_impl(roots).await;

        let mut worker = self.refresh_worker.write().await;
        worker.is_idle = true;

        result
    }

    /// Internal implementation for refreshing non-curated plugins from workspace roots.
    async fn refresh_non_curated_from_roots_impl(
        &self,
        roots: &[PathBuf],
    ) -> Result<RefreshResult> {
        if roots.is_empty() {
            debug!("no workspace roots provided for non-curated plugin cache refresh");
            return Ok(RefreshResult::NoRootsProvided);
        }

        let mut refreshed_count = 0usize;
        let mut errors = Vec::with_capacity(roots.len());

        for root in roots {
            if !root.exists() {
                debug!(
                    "workspace root does not exist, skipping: {}",
                    root.display()
                );
                continue;
            }

            match self.scan_root_for_plugins(root).await {
                Ok(plugins) => {
                    for plugin_info in &plugins {
                        // Use version from plugin.json to determine if update is needed
                        if let Some(existing) = self.get_cached_plugin(&plugin_info.name).await
                            && existing.exists()
                            && plugin_info.version_matches_existing(&existing).await
                        {
                            debug!(
                                "plugin '{}' version unchanged, skipping cache update",
                                plugin_info.name
                            );
                            continue;
                        }

                        if let Err(e) = self
                            .cache_plugin(&plugin_info.name, &plugin_info.path)
                            .await
                        {
                            errors.push(format!(
                                "failed to cache plugin '{}': {e}",
                                plugin_info.name
                            ));
                        } else {
                            refreshed_count += 1;
                            info!("cached non-curated plugin: {}", plugin_info.name);
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!("failed to scan root {}: {e}", root.display()));
                }
            }
        }

        if errors.is_empty() {
            Ok(RefreshResult::Success {
                refreshed_count,
                errors: Vec::new(),
            })
        } else {
            Ok(RefreshResult::SuccessWithErrors {
                refreshed_count,
                errors,
            })
        }
    }

    /// Scan a workspace root for non-curated plugins (marketplace.json files).
    async fn scan_root_for_plugins(&self, root: &Path) -> Result<Vec<DiscoveredPluginInfo>> {
        let mut discovered = Vec::new();

        // Look for plugins in .vtcode/plugins/ or similar locations
        let plugin_roots = vec![root.join(".vtcode").join("plugins"), root.join("plugins")];

        for plugin_root in plugin_roots {
            if !plugin_root.exists() {
                continue;
            }

            // Read each subdirectory as a potential plugin
            let entries = match tokio::fs::read_dir(&plugin_root).await {
                Ok(entries) => entries,
                Err(e) => {
                    warn!(
                        "Failed to read plugin root {}: {}",
                        plugin_root.display(),
                        e
                    );
                    continue;
                }
            };

            // Collect entries first to avoid borrow issues
            let mut dirs = Vec::new();
            let mut entries = entries;
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.file_type().await.is_ok_and(|ft| ft.is_dir()) {
                    dirs.push(entry.path());
                }
            }

            for plugin_dir in dirs {
                let manifest_path = plugin_dir.join(".vtcode-plugin").join("plugin.json");

                if !manifest_path.exists() {
                    continue;
                }

                match self.load_plugin_manifest(&manifest_path).await {
                    Ok(info) => discovered.push(info),
                    Err(e) => {
                        warn!(
                            "Failed to load plugin manifest from {}: {}",
                            manifest_path.display(),
                            e
                        );
                    }
                }
            }
        }

        Ok(discovered)
    }

    /// Load a plugin manifest from a marketplace.json or plugin.json path.
    async fn load_plugin_manifest(&self, manifest_path: &Path) -> Result<DiscoveredPluginInfo> {
        let content = tokio::fs::read_to_string(manifest_path).await?;
        let manifest: PluginManifest = serde_json::from_str(&content)?;

        Ok(DiscoveredPluginInfo {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            path: manifest_path
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(manifest_path)
                .to_path_buf(),
        })
    }
}

/// Result of a non-curated plugin cache refresh operation.
#[derive(Debug)]
#[non_exhaustive]
pub enum RefreshResult {
    /// Refresh completed successfully
    Success {
        refreshed_count: usize,
        errors: Vec<String>,
    },
    /// Refresh completed with some errors
    SuccessWithErrors {
        refreshed_count: usize,
        errors: Vec<String>,
    },
    /// Refresh was skipped because one is already in progress
    SkippedAlreadyInProgress,
    /// No roots were provided for the refresh
    NoRootsProvided,
}

/// Information about a discovered non-curated plugin.
#[derive(Debug, Clone)]
pub struct DiscoveredPluginInfo {
    /// Plugin name from manifest
    pub name: String,
    /// Plugin version from manifest (used for cache invalidation)
    pub version: Option<String>,
    /// Path to the plugin directory
    pub path: PathBuf,
}

impl DiscoveredPluginInfo {
    /// Check if this plugin's version matches an existing cached version.
    async fn version_matches_existing(&self, existing_path: &Path) -> bool {
        // If no version is set, always refresh (conservative)
        let Some(ref current_version) = self.version else {
            return false;
        };

        // Try to read the cached manifest and compare versions
        let cached_manifest_path = existing_path.join(".vtcode-plugin").join("plugin.json");
        if !cached_manifest_path.exists() {
            return false;
        }

        match tokio::fs::read_to_string(&cached_manifest_path).await {
            Ok(content) => match serde_json::from_str::<PluginManifest>(&content) {
                Ok(cached) => cached.version.as_deref() == Some(current_version),
                Err(_) => false,
            },
            Err(_) => false,
        }
    }
}
