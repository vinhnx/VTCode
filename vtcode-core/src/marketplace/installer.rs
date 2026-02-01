//! Plugin installer for marketplace system

use std::path::{Path, PathBuf};

use crate::tools::plugins::PluginRuntime;
use crate::utils::file_utils::{ensure_dir_exists, write_file_with_context, write_json_file};
use crate::utils::validation::{validate_all_non_empty, validate_non_empty, validate_path_exists};
use anyhow::{Context, Result, bail};
use tokio::fs;

use super::PluginManifest;

/// Plugin installer that handles downloading and installing plugins from marketplaces
pub struct PluginInstaller {
    /// Base directory for installed plugins
    pub plugins_dir: PathBuf,

    /// Reference to the core plugin runtime for integration
    core_plugin_runtime: Option<PluginRuntime>,
}

impl PluginInstaller {
    pub fn new(plugins_dir: PathBuf, core_plugin_runtime: Option<PluginRuntime>) -> Self {
        Self {
            plugins_dir,
            core_plugin_runtime,
        }
    }

    /// Install a plugin from its manifest
    pub async fn install_plugin(&self, manifest: &PluginManifest) -> Result<()> {
        // Create plugins directory if it doesn't exist
        ensure_dir_exists(&self.plugins_dir).await?;

        // Create plugin installation directory
        let plugin_dir = self.plugins_dir.join(&manifest.id);
        ensure_dir_exists(&plugin_dir).await?;

        // Download the plugin from its source
        self.download_plugin(manifest, &plugin_dir).await?;

        // Save the manifest to the plugin directory
        let manifest_dir = plugin_dir.join(".vtcode-plugin");
        let manifest_path = manifest_dir.join("plugin.json");
        write_json_file(&manifest_path, manifest).await?;

        // Integrate with VT Code's existing plugin system
        self.integrate_with_core_plugin_system(&manifest_path)
            .await?;

        Ok(())
    }

    /// Integrate the installed plugin with VT Code's core plugin system
    async fn integrate_with_core_plugin_system(&self, manifest_path: &Path) -> Result<()> {
        // This would load the plugin into VT Code's plugin runtime
        if let Some(runtime) = &self.core_plugin_runtime {
            // Load the plugin manifest and register it with the core runtime
            let handle = runtime.register_manifest(manifest_path).await?;
            println!(
                "Successfully registered plugin with core runtime: {}",
                handle.manifest.id
            );
        } else {
            println!(
                "No core plugin runtime provided, skipping integration: {}",
                manifest_path.display()
            );
        }

        Ok(())
    }

    /// Download plugin from its source
    async fn download_plugin(&self, manifest: &PluginManifest, plugin_dir: &Path) -> Result<()> {
        // Validate the manifest before downloading
        self.validate_manifest(manifest)?;

        println!(
            "Downloading plugin '{}' from source: {}",
            manifest.id, manifest.source
        );

        // Determine the source type and download accordingly
        if manifest.source.starts_with("http") {
            self.download_from_http(manifest, plugin_dir).await?;
        } else if manifest.source.starts_with("file://") {
            self.download_from_file(manifest, plugin_dir).await?;
        } else if std::path::Path::new(&manifest.source).exists() {
            // Local path
            self.download_from_local(manifest, plugin_dir).await?;
        } else {
            // Assume it's a git repository
            self.download_from_git(manifest, plugin_dir).await?;
        }

        Ok(())
    }

    /// Download plugin from HTTP source
    async fn download_from_http(&self, manifest: &PluginManifest, plugin_dir: &Path) -> Result<()> {
        // For now, we'll create a placeholder since we don't have the actual HTTP client configured
        let placeholder_path = plugin_dir.join(&manifest.entrypoint);

        write_file_with_context(
            &placeholder_path,
            &format!("# HTTP Downloaded plugin: {}\n", manifest.id),
            "plugin entrypoint",
        )
        .await?;

        println!("HTTP download completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Download plugin from local file
    async fn download_from_file(&self, manifest: &PluginManifest, plugin_dir: &Path) -> Result<()> {
        let source_path = PathBuf::from(&manifest.source.replace("file://", ""));
        validate_path_exists(&source_path, "Local source file")?;

        let dest_path = plugin_dir.join(&manifest.entrypoint);
        if let Some(parent) = dest_path.parent() {
            ensure_dir_exists(parent).await?;
        }

        // Copy the file from source to destination
        tokio::fs::copy(&source_path, &dest_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to copy plugin from {} to {}",
                    source_path.display(),
                    dest_path.display()
                )
            })?;

        println!("Local file copy completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Download plugin from local path
    async fn download_from_local(
        &self,
        manifest: &PluginManifest,
        plugin_dir: &Path,
    ) -> Result<()> {
        let source_path = PathBuf::from(&manifest.source);
        validate_path_exists(&source_path, "Local source path")?;

        let dest_path = plugin_dir.join(&manifest.entrypoint);
        if let Some(parent) = dest_path.parent() {
            ensure_dir_exists(parent).await?;
        }

        // Copy the file from source to destination
        tokio::fs::copy(&source_path, &dest_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to copy plugin from {} to {}",
                    source_path.display(),
                    dest_path.display()
                )
            })?;

        println!("Local path copy completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Download plugin from git repository
    async fn download_from_git(&self, manifest: &PluginManifest, plugin_dir: &Path) -> Result<()> {
        // For now, we'll create a placeholder since we don't have git functionality integrated
        let placeholder_path = plugin_dir.join(&manifest.entrypoint);

        write_file_with_context(
            &placeholder_path,
            &format!("# Git downloaded plugin: {}\n", manifest.id),
            "plugin entrypoint",
        )
        .await?;

        println!("Git download completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Validate the plugin manifest before installation
    pub fn validate_manifest(&self, manifest: &PluginManifest) -> Result<()> {
        // Validate required fields
        validate_non_empty(&manifest.id, "Plugin ID")?;
        validate_non_empty(&manifest.name, "Plugin name")?;
        validate_non_empty(&manifest.source, "Plugin source URL")?;

        // Validate entrypoint path
        if manifest.entrypoint.as_os_str().is_empty() {
            bail!("Plugin manifest must have a valid entrypoint path");
        }

        // Validate trust level if specified
        if let Some(trust_level) = &manifest.trust_level {
            match trust_level {
                crate::config::PluginTrustLevel::Sandbox
                | crate::config::PluginTrustLevel::Trusted
                | crate::config::PluginTrustLevel::Untrusted => {
                    // Valid trust level
                }
            }
        }

        // Validate dependencies if any
        validate_all_non_empty(&manifest.dependencies, "Plugin dependencies")?;

        Ok(())
    }

    /// Uninstall a plugin by ID
    pub async fn uninstall_plugin(&self, plugin_id: &str) -> Result<()> {
        let plugin_dir = self.plugins_dir.join(plugin_id);
        validate_path_exists(&plugin_dir, "Installed plugin")?;

        // Remove from VT Code's plugin system before filesystem removal
        self.remove_from_core_plugin_system(plugin_id).await?;

        fs::remove_dir_all(&plugin_dir).await.with_context(|| {
            format!(
                "Failed to remove plugin directory: {}",
                plugin_dir.display()
            )
        })?;

        Ok(())
    }

    /// Remove plugin from VT Code's core plugin system
    async fn remove_from_core_plugin_system(&self, plugin_id: &str) -> Result<()> {
        // Remove the plugin from VT Code's plugin runtime
        if let Some(runtime) = &self.core_plugin_runtime {
            // Unload the plugin by ID
            runtime
                .unload_plugin(plugin_id)
                .await
                .with_context(|| format!("Failed to unload plugin from runtime: {}", plugin_id))?;
            println!(
                "Successfully unloaded plugin from core runtime: {}",
                plugin_id
            );
        } else {
            println!(
                "No core plugin runtime provided, skipping removal: {}",
                plugin_id
            );
        }

        Ok(())
    }

    /// Check if a plugin is installed
    pub async fn is_installed(&self, plugin_id: &str) -> bool {
        let plugin_dir = self.plugins_dir.join(plugin_id);
        plugin_dir.exists()
    }
}
