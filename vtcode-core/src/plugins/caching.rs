//! Plugin caching system for VT Code
//!
//! Implements the caching mechanism for plugins to ensure security and verification
//! as described in the VT Code plugin reference.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::utils::path::resolve_workspace_path;

use super::{PluginError, PluginResult};

/// Plugin cache manager
pub struct PluginCache {
    /// Base directory for the plugin cache
    cache_dir: PathBuf,
    /// Mapping of plugin IDs to their cached paths
    cached_plugins: HashMap<String, PathBuf>,
}

impl PluginCache {
    /// Create a new plugin cache
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            cached_plugins: HashMap::new(),
        }
    }

    /// Cache a plugin from its source path
    pub async fn cache_plugin(
        &mut self,
        plugin_id: &str,
        source_path: &Path,
    ) -> PluginResult<PathBuf> {
        // Validate source path exists
        if !source_path.exists() {
            return Err(PluginError::LoadingError(format!(
                "Source path does not exist: {}",
                source_path.display()
            )));
        }

        // Create cache directory if it doesn't exist
        fs::create_dir_all(&self.cache_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to create cache directory: {}", e))
        })?;

        // Create plugin-specific cache directory
        let cache_path = self.cache_dir.join(plugin_id);

        // Remove existing cache if it exists
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to remove existing cache: {}", e))
            })?;
        }

        // Copy plugin to cache directory
        self.copy_plugin_to_cache(source_path, &cache_path).await?;

        // Store in cache mapping
        self.cached_plugins
            .insert(plugin_id.to_string(), cache_path.clone());

        Ok(cache_path)
    }

    /// Copy plugin files to cache directory
    async fn copy_plugin_to_cache(&self, source: &Path, destination: &Path) -> PluginResult<()> {
        Box::pin(async {
            fs::create_dir_all(destination).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to create destination directory: {}", e))
            })?;

            let mut entries = fs::read_dir(source).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to read source directory: {}", e))
            })?;

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to read directory entry: {}", e))
            })? {
                let src_path = entry.path();
                let dst_path = destination.join(entry.file_name());

                if src_path.is_dir() {
                    // Skip directories that are outside the plugin root (for security)
                    if self.is_valid_plugin_subdirectory(&src_path) {
                        self.copy_plugin_to_cache(&src_path, &dst_path).await?;
                    }
                } else {
                    // Copy file to cache
                    fs::copy(&src_path, &dst_path).await.map_err(|e| {
                        PluginError::LoadingError(format!("Failed to copy file: {}", e))
                    })?;
                }
            }

            Ok(())
        })
        .await
    }

    /// Check if a subdirectory is valid for caching (not traversing outside plugin root)
    fn is_valid_plugin_subdirectory(&self, path: &Path) -> bool {
        // For security, we only allow subdirectories that are within the plugin directory
        // This prevents path traversal attacks
        resolve_workspace_path(&self.cache_dir, path).is_ok()
    }

    /// Get cached plugin path
    pub fn get_cached_plugin(&self, plugin_id: &str) -> Option<&PathBuf> {
        self.cached_plugins.get(plugin_id)
    }

    /// Remove cached plugin
    pub async fn remove_cached_plugin(&mut self, plugin_id: &str) -> PluginResult<()> {
        if let Some(cache_path) = self.cached_plugins.get(plugin_id) {
            if cache_path.exists() {
                fs::remove_dir_all(cache_path).await.map_err(|e| {
                    PluginError::LoadingError(format!("Failed to remove cached plugin: {}", e))
                })?;
            }
            self.cached_plugins.remove(plugin_id);
        }
        Ok(())
    }

    /// Clear entire cache
    pub async fn clear_cache(&mut self) -> PluginResult<()> {
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)
                .await
                .map_err(|e| PluginError::LoadingError(format!("Failed to clear cache: {}", e)))?;
        }

        self.cached_plugins.clear();
        Ok(())
    }

    /// Validate plugin security by checking for path traversal
    pub fn validate_plugin_security(&self, plugin_path: &Path) -> PluginResult<()> {
        // Check that the plugin doesn't contain paths that traverse outside its expected directory
        // This is a more thorough check - we scan all files in the plugin directory
        if !plugin_path.exists() {
            return Err(PluginError::LoadingError(
                "Plugin path does not exist".to_string(),
            ));
        }

        // Check the plugin path itself for traversal attempts
        let plugin_str = plugin_path.to_string_lossy();
        if plugin_str.contains("../") || plugin_str.contains("..\\") {
            return Err(PluginError::LoadingError(
                "Plugin path contains path traversal attempts".to_string(),
            ));
        }

        // Walk through all files in the plugin directory to check for traversal attempts
        let mut stack = vec![plugin_path.to_path_buf()];
        while let Some(current_path) = stack.pop() {
            if current_path.is_dir()
                && let Ok(entries) = std::fs::read_dir(&current_path)
            {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    let entry_str = entry_path.to_string_lossy();

                    // Check for path traversal in the file/directory names
                    if entry_str.contains("../") || entry_str.contains("..\\") {
                        return Err(PluginError::LoadingError(format!(
                            "Plugin contains path traversal in file: {}",
                            entry_path.display()
                        )));
                    }

                    if entry_path.is_dir() {
                        stack.push(entry_path);
                    }
                }
            }
        }

        Ok(())
    }
}
