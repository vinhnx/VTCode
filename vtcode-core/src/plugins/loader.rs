//! Plugin loader for VT Code
//!
//! Handles the discovery, installation, and loading of plugins from various sources.

use std::path::{Path, PathBuf};
use tokio::fs;

use super::{PluginError, PluginManifest, PluginResult, PluginRuntime};

/// Plugin source types
#[derive(Debug, Clone)]
pub enum PluginSource {
    /// Local directory
    Local(PathBuf),
    /// Git repository
    Git(String),
    /// HTTP URL
    Http(String),
    /// Marketplace identifier
    Marketplace(String),
}

/// Plugin loader that handles plugin installation and management
pub struct PluginLoader {
    /// Base directory for plugin installations
    plugins_dir: PathBuf,
    /// Runtime for managing loaded plugins
    runtime: PluginRuntime,
}

impl PluginLoader {
    /// Create a new plugin loader
    pub fn new(plugins_dir: PathBuf, runtime: PluginRuntime) -> Self {
        Self {
            plugins_dir,
            runtime,
        }
    }

    /// Install a plugin from a source
    pub async fn install_plugin(
        &self,
        source: PluginSource,
        name: Option<String>,
    ) -> PluginResult<()> {
        let plugin_dir = match source {
            PluginSource::Local(path) => self.install_from_local(path).await?,
            PluginSource::Git(url) => self.install_from_git(&url, name.as_deref()).await?,
            PluginSource::Http(url) => self.install_from_http(&url, name.as_deref()).await?,
            PluginSource::Marketplace(id) => {
                self.install_from_marketplace(&id, name.as_deref()).await?
            }
        };

        // Load the installed plugin
        self.runtime.load_plugin(&plugin_dir).await?;
        Ok(())
    }

    /// Install plugin from local directory
    async fn install_from_local(&self, source_path: PathBuf) -> PluginResult<PathBuf> {
        if !source_path.exists() {
            return Err(PluginError::LoadingError(format!(
                "Source path does not exist: {}",
                source_path.display()
            )));
        }

        // Validate that it contains a plugin manifest
        let manifest_path = source_path.join(".vtcode-plugin/plugin.json");
        if !manifest_path.exists() {
            return Err(PluginError::ManifestValidationError(format!(
                "Plugin manifest not found in source: {}",
                manifest_path.display()
            )));
        }

        // Load the manifest to get the plugin name
        let manifest_content = fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| PluginError::LoadingError(format!("Failed to read manifest: {}", e)))?;

        let manifest: PluginManifest = serde_json::from_str(&manifest_content).map_err(|e| {
            PluginError::ManifestValidationError(format!("Invalid manifest JSON: {}", e))
        })?;

        // Create installation directory
        let install_dir = self.plugins_dir.join(&manifest.name);
        fs::create_dir_all(&install_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to create plugin directory: {}", e))
        })?;

        // Copy plugin files to installation directory
        self.copy_directory(&source_path, &install_dir).await?;

        Ok(install_dir)
    }

    /// Install plugin from Git repository
    async fn install_from_git(&self, url: &str, name: Option<&str>) -> PluginResult<PathBuf> {
        use tempfile::TempDir;
        use tokio::process::Command;

        // Create a temporary directory for the git clone
        let temp_dir = TempDir::new().map_err(|e| {
            PluginError::LoadingError(format!("Failed to create temporary directory: {}", e))
        })?;
        let temp_path = temp_dir.path();

        // Execute git clone command
        let output = Command::new("git")
            .arg("clone")
            .arg(url)
            .arg(temp_path)
            .output()
            .await
            .map_err(|e| {
                PluginError::LoadingError(format!("Failed to execute git clone: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PluginError::LoadingError(format!(
                "Git clone failed: {}",
                stderr
            )));
        }

        // Determine plugin name
        let plugin_name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.extract_name_from_git_url(url));

        // Create installation directory
        let install_dir = self.plugins_dir.join(&plugin_name);
        fs::create_dir_all(&install_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to create plugin directory: {}", e))
        })?;

        // Copy the cloned repository contents to the installation directory
        self.copy_directory(temp_path, &install_dir).await?;

        // Verify that the plugin manifest exists in the installed directory
        let manifest_path = install_dir.join(".vtcode-plugin/plugin.json");
        if !manifest_path.exists() {
            return Err(PluginError::ManifestValidationError(
                "Plugin manifest not found in cloned repository".to_string(),
            ));
        }

        Ok(install_dir)
    }

    /// Install plugin from HTTP URL
    async fn install_from_http(&self, url: &str, name: Option<&str>) -> PluginResult<PathBuf> {
        // For now, create a placeholder implementation
        let plugin_name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.extract_name_from_url(url));

        let install_dir = self.plugins_dir.join(&plugin_name);
        fs::create_dir_all(&install_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to create plugin directory: {}", e))
        })?;

        // Create a placeholder manifest file
        let placeholder_manifest = format!(
            r#"{{
  "name": "{}",
  "version": "1.0.0",
  "description": "Placeholder for HTTP-installed plugin from {}"
}}"#,
            plugin_name, url
        );

        let manifest_path = install_dir.join(".vtcode-plugin/plugin.json");
        fs::create_dir_all(manifest_path.parent().unwrap()).await?;
        fs::write(&manifest_path, placeholder_manifest)
            .await
            .map_err(|e| {
                PluginError::LoadingError(format!("Failed to create placeholder manifest: {}", e))
            })?;

        Ok(install_dir)
    }

    /// Install plugin from marketplace
    async fn install_from_marketplace(
        &self,
        marketplace_id: &str,
        name: Option<&str>,
    ) -> PluginResult<PathBuf> {
        // For now, create a placeholder implementation
        let plugin_name = name.map(|s| s.to_string()).unwrap_or_else(|| {
            marketplace_id
                .split('/')
                .next_back()
                .unwrap_or(marketplace_id)
                .to_string()
        });

        let install_dir = self.plugins_dir.join(&plugin_name);
        fs::create_dir_all(&install_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to create plugin directory: {}", e))
        })?;

        // Create a placeholder manifest file
        let placeholder_manifest = format!(
            r#"{{
  "name": "{}",
  "version": "1.0.0",
  "description": "Placeholder for marketplace-installed plugin from {}"
}}"#,
            plugin_name, marketplace_id
        );

        let manifest_path = install_dir.join(".vtcode-plugin/plugin.json");
        fs::create_dir_all(manifest_path.parent().unwrap()).await?;
        fs::write(&manifest_path, placeholder_manifest)
            .await
            .map_err(|e| {
                PluginError::LoadingError(format!("Failed to create placeholder manifest: {}", e))
            })?;

        Ok(install_dir)
    }

    /// Uninstall a plugin
    pub async fn uninstall_plugin(&self, plugin_name: &str) -> PluginResult<()> {
        let plugin_dir = self.plugins_dir.join(plugin_name);

        if !plugin_dir.exists() {
            return Err(PluginError::NotFound(plugin_name.to_string()));
        }

        // Unload the plugin from runtime first
        self.runtime.unload_plugin(plugin_name).await?;

        // Remove the plugin directory
        fs::remove_dir_all(&plugin_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to remove plugin directory: {}", e))
        })?;

        Ok(())
    }

    /// List installed plugins
    pub async fn list_installed_plugins(&self) -> PluginResult<Vec<String>> {
        let mut plugins = Vec::new();

        if !self.plugins_dir.exists() {
            return Ok(plugins);
        }

        let mut entries = fs::read_dir(&self.plugins_dir).await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to read plugins directory: {}", e))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            PluginError::LoadingError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();
            if path.is_dir() {
                // Check if it contains a plugin manifest
                let manifest_path = path.join(".vtcode-plugin/plugin.json");
                if manifest_path.exists()
                    && let Some(name) = path.file_name()
                {
                    plugins.push(name.to_string_lossy().to_string());
                }
            }
        }

        Ok(plugins)
    }

    /// Copy directory recursively
    async fn copy_directory(&self, src: &Path, dst: &Path) -> PluginResult<()> {
        Box::pin(async {
            if !src.is_dir() {
                return Err(PluginError::LoadingError(format!(
                    "Source is not a directory: {}",
                    src.display()
                )));
            }

            fs::create_dir_all(dst).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to create destination directory: {}", e))
            })?;

            let mut entries = fs::read_dir(src).await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to read source directory: {}", e))
            })?;

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                PluginError::LoadingError(format!("Failed to read directory entry: {}", e))
            })? {
                let src_path = entry.path();
                let dst_path = dst.join(entry.file_name());

                if src_path.is_dir() {
                    self.copy_directory(&src_path, &dst_path).await?;
                } else {
                    fs::copy(&src_path, &dst_path).await.map_err(|e| {
                        PluginError::LoadingError(format!("Failed to copy file: {}", e))
                    })?;
                }
            }

            Ok(())
        })
        .await
    }

    /// Extract plugin name from Git URL
    fn extract_name_from_git_url(&self, url: &str) -> String {
        // Extract name from git URL (e.g., https://github.com/user/repo.git -> repo)
        url.trim_end_matches(".git")
            .split('/')
            .next_back()
            .unwrap_or("unknown-plugin")
            .to_string()
    }

    /// Extract plugin name from URL
    fn extract_name_from_url(&self, url: &str) -> String {
        // Extract name from URL path
        url.split('/')
            .next_back()
            .unwrap_or("unknown-plugin")
            .to_string()
    }
}
