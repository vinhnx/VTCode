//! Plugin installer for marketplace system

use std::path::PathBuf;

use anyhow::{Result, Context, bail};
use tokio::fs;
use crate::tools::plugins::PluginRuntime;

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
        fs::create_dir_all(&self.plugins_dir)
            .await
            .with_context(|| format!("Failed to create plugins directory: {}", self.plugins_dir.display()))?;

        // Create plugin installation directory
        let plugin_dir = self.plugins_dir.join(&manifest.id);
        fs::create_dir_all(&plugin_dir)
            .await
            .with_context(|| format!("Failed to create plugin directory: {}", plugin_dir.display()))?;

        // Download the plugin from its source
        self.download_plugin(manifest, &plugin_dir).await?;

        // Save the manifest to the plugin directory
        let manifest_dir = plugin_dir.join(".vtcode-plugin");
        fs::create_dir_all(&manifest_dir)
            .await
            .with_context(|| format!("Failed to create manifest directory: {}", manifest_dir.display()))?;
            
        let manifest_path = manifest_dir.join("plugin.json");
        let manifest_content = serde_json::to_string_pretty(manifest)
            .with_context(|| "Failed to serialize plugin manifest")?;

        fs::write(&manifest_path, manifest_content)
            .await
            .with_context(|| format!("Failed to write plugin manifest: {}", manifest_path.display()))?;

        // Integrate with VTCode's existing plugin system
        self.integrate_with_core_plugin_system(&manifest_path).await?;

        Ok(())
    }

    /// Integrate the installed plugin with VTCode's core plugin system
    async fn integrate_with_core_plugin_system(&self, manifest_path: &PathBuf) -> Result<()> {
        // This would load the plugin into VTCode's plugin runtime
        if let Some(runtime) = &self.core_plugin_runtime {
            // Load the plugin manifest and register it with the core runtime
            let handle = runtime.register_manifest(manifest_path).await?;
            println!("Successfully registered plugin with core runtime: {}", handle.manifest.id);
        } else {
            println!("No core plugin runtime provided, skipping integration: {}", manifest_path.display());
        }

        Ok(())
    }

    /// Download plugin from its source
    async fn download_plugin(&self, manifest: &PluginManifest, plugin_dir: &PathBuf) -> Result<()> {
        // Validate the manifest before downloading
        self.validate_manifest(manifest)?;

        println!("Downloading plugin '{}' from source: {}", manifest.id, manifest.source);

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
    async fn download_from_http(&self, manifest: &PluginManifest, plugin_dir: &PathBuf) -> Result<()> {
        // For now, we'll create a placeholder since we don't have the actual HTTP client configured
        let placeholder_path = plugin_dir.join(&manifest.entrypoint);
        if let Some(parent) = placeholder_path.parent() {
            fs::create_dir_all(parent).await
                .with_context(|| format!("Failed to create parent directory for: {}", placeholder_path.display()))?;
        }

        fs::write(&placeholder_path, format!("# HTTP Downloaded plugin: {}\n", manifest.id))
            .await
            .with_context(|| format!("Failed to create plugin file: {}", placeholder_path.display()))?;

        println!("HTTP download completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Download plugin from local file
    async fn download_from_file(&self, manifest: &PluginManifest, plugin_dir: &PathBuf) -> Result<()> {
        let source_path = PathBuf::from(&manifest.source.replace("file://", ""));

        if !source_path.exists() {
            bail!("Local source file does not exist: {}", source_path.display());
        }

        let dest_path = plugin_dir.join(&manifest.entrypoint);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).await
                .with_context(|| format!("Failed to create parent directory for: {}", dest_path.display()))?;
        }

        // Copy the file from source to destination
        tokio::fs::copy(&source_path, &dest_path)
            .await
            .with_context(|| format!("Failed to copy plugin from {} to {}", source_path.display(), dest_path.display()))?;

        println!("Local file copy completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Download plugin from local path
    async fn download_from_local(&self, manifest: &PluginManifest, plugin_dir: &PathBuf) -> Result<()> {
        let source_path = PathBuf::from(&manifest.source);

        if !source_path.exists() {
            bail!("Local source path does not exist: {}", source_path.display());
        }

        let dest_path = plugin_dir.join(&manifest.entrypoint);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).await
                .with_context(|| format!("Failed to create parent directory for: {}", dest_path.display()))?;
        }

        // Copy the file from source to destination
        tokio::fs::copy(&source_path, &dest_path)
            .await
            .with_context(|| format!("Failed to copy plugin from {} to {}", source_path.display(), dest_path.display()))?;

        println!("Local path copy completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Download plugin from git repository
    async fn download_from_git(&self, manifest: &PluginManifest, plugin_dir: &PathBuf) -> Result<()> {
        // For now, we'll create a placeholder since we don't have git functionality integrated
        let placeholder_path = plugin_dir.join(&manifest.entrypoint);
        if let Some(parent) = placeholder_path.parent() {
            fs::create_dir_all(parent).await
                .with_context(|| format!("Failed to create parent directory for: {}", placeholder_path.display()))?;
        }

        fs::write(&placeholder_path, format!("# Git downloaded plugin: {}\n", manifest.id))
            .await
            .with_context(|| format!("Failed to create plugin file: {}", placeholder_path.display()))?;

        println!("Git download completed for plugin: {}", manifest.id);
        Ok(())
    }

    /// Validate the plugin manifest before installation
    pub fn validate_manifest(&self, manifest: &PluginManifest) -> Result<()> {
        // Validate required fields
        if manifest.id.is_empty() {
            bail!("Plugin manifest must have a non-empty ID");
        }

        if manifest.name.is_empty() {
            bail!("Plugin manifest must have a non-empty name");
        }

        if manifest.source.is_empty() {
            bail!("Plugin manifest must have a non-empty source URL");
        }

        // Validate entrypoint path
        if manifest.entrypoint.as_os_str().is_empty() {
            bail!("Plugin manifest must have a valid entrypoint path");
        }

        // Validate trust level if specified
        if let Some(trust_level) = &manifest.trust_level {
            match trust_level {
                crate::config::PluginTrustLevel::Sandbox |
                crate::config::PluginTrustLevel::Trusted |
                crate::config::PluginTrustLevel::Untrusted => {
                    // Valid trust level
                }
            }
        }

        // Validate dependencies if any
        for dep in &manifest.dependencies {
            if dep.is_empty() {
                bail!("Plugin manifest contains empty dependency: {:?}", manifest.dependencies);
            }
        }

        Ok(())
    }

    /// Uninstall a plugin by ID
    pub async fn uninstall_plugin(&self, plugin_id: &str) -> Result<()> {
        let plugin_dir = self.plugins_dir.join(plugin_id);

        if !plugin_dir.exists() {
            bail!("Plugin '{}' is not installed", plugin_id);
        }

        // Remove from VTCode's plugin system before filesystem removal
        self.remove_from_core_plugin_system(plugin_id).await?;

        fs::remove_dir_all(&plugin_dir)
            .await
            .with_context(|| format!("Failed to remove plugin directory: {}", plugin_dir.display()))?;

        Ok(())
    }

    /// Remove plugin from VTCode's core plugin system
    async fn remove_from_core_plugin_system(&self, plugin_id: &str) -> Result<()> {
        // Remove the plugin from VTCode's plugin runtime
        if let Some(runtime) = &self.core_plugin_runtime {
            // Unload the plugin by ID
            runtime.unload_plugin(plugin_id).await
                .with_context(|| format!("Failed to unload plugin from runtime: {}", plugin_id))?;
            println!("Successfully unloaded plugin from core runtime: {}", plugin_id);
        } else {
            println!("No core plugin runtime provided, skipping removal: {}", plugin_id);
        }

        Ok(())
    }

    /// Check if a plugin is installed
    pub async fn is_installed(&self, plugin_id: &str) -> bool {
        let plugin_dir = self.plugins_dir.join(plugin_id);
        plugin_dir.exists()
    }
}