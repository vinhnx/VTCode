//! Marketplace configuration system for VT Code
//!
//! This module handles the integration of marketplace settings with VT Code's configuration system.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::marketplace::MarketplaceSource;
use crate::utils::file_utils::{read_file_with_context, write_file_with_context};

/// Configuration for marketplace settings that integrates with VT Code's config system
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MarketplaceSettings {
    /// List of configured marketplaces
    #[serde(default)]
    pub marketplaces: Vec<MarketplaceSource>,

    /// List of installed plugins with their settings
    #[serde(default)]
    pub installed_plugins: Vec<InstalledPlugin>,

    /// Auto-update settings
    #[serde(default)]
    pub auto_update: AutoUpdateSettings,

    /// Security and trust settings
    #[serde(default)]
    pub security: SecuritySettings,
}

/// Information about an installed plugin
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstalledPlugin {
    /// Plugin ID
    pub id: String,

    /// Name of the plugin
    pub name: String,

    /// Version of the plugin
    pub version: String,

    /// Source marketplace
    pub source: String,

    /// Installation path
    pub install_path: PathBuf,

    /// Whether the plugin is enabled
    pub enabled: bool,

    /// Trust level of the plugin
    pub trust_level: crate::config::PluginTrustLevel,

    /// Installation timestamp
    pub installed_at: String, // Using string for simplicity, could be a proper datetime type
}

/// Auto-update settings for marketplaces and plugins
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutoUpdateSettings {
    /// Whether to auto-update marketplaces
    #[serde(default = "default_true")]
    pub marketplaces: bool,

    /// Whether to auto-update plugins
    #[serde(default = "default_true")]
    pub plugins: bool,

    /// Check for updates interval in hours
    #[serde(default = "default_update_interval")]
    pub check_interval_hours: u32,
}

/// Security and trust settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecuritySettings {
    /// Default trust level for new plugins
    #[serde(default)]
    pub default_trust_level: crate::config::PluginTrustLevel,

    /// Whether to require confirmation for untrusted plugins
    #[serde(default = "default_true")]
    pub require_confirmation: bool,

    /// List of allowed plugin sources (whitelist)
    #[serde(default)]
    pub allowed_sources: Vec<String>,

    /// List of blocked plugin sources (blacklist)
    #[serde(default)]
    pub blocked_sources: Vec<String>,
}

impl Default for AutoUpdateSettings {
    fn default() -> Self {
        Self {
            marketplaces: true,
            plugins: true,
            check_interval_hours: 24,
        }
    }
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            default_trust_level: crate::config::PluginTrustLevel::Sandbox,
            require_confirmation: true,
            allowed_sources: Vec::new(),
            blocked_sources: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_update_interval() -> u32 {
    24
}

impl MarketplaceSettings {
    /// Load marketplace settings from a configuration file
    pub async fn load_from_file(config_path: &Path) -> Result<Self> {
        if !config_path.exists() {
            // Return default settings if file doesn't exist
            return Ok(Self::default());
        }

        let content = read_file_with_context(config_path, "marketplace config file").await?;

        let settings: MarketplaceSettings = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse marketplace config file: {}",
                config_path.display()
            )
        })?;

        Ok(settings)
    }

    /// Save marketplace settings to a configuration file
    pub async fn save_to_file(&self, config_path: &Path) -> Result<()> {
        let content =
            toml::to_string(&self).with_context(|| "Failed to serialize marketplace settings")?;

        write_file_with_context(config_path, &content, "marketplace config file").await?;

        Ok(())
    }

    /// Add a marketplace to the configuration
    pub fn add_marketplace(&mut self, marketplace: MarketplaceSource) {
        // Check if marketplace already exists
        if !self.marketplaces.iter().any(|m| m.id() == marketplace.id()) {
            self.marketplaces.push(marketplace);
        }
    }

    /// Remove a marketplace from the configuration
    pub fn remove_marketplace(&mut self, id: &str) -> bool {
        let initial_len = self.marketplaces.len();
        self.marketplaces.retain(|m| m.id() != id);
        self.marketplaces.len() != initial_len
    }

    /// Check if a plugin source is allowed based on security settings
    pub fn is_source_allowed(&self, source_url: &str) -> bool {
        // If allowed sources list is empty, all sources are allowed (except blocked ones)
        let allowed = self.security.allowed_sources.is_empty()
            || self
                .security
                .allowed_sources
                .iter()
                .any(|s| source_url.contains(s));

        // Check if source is blocked
        let blocked = self
            .security
            .blocked_sources
            .iter()
            .any(|s| source_url.contains(s));

        allowed && !blocked
    }

    /// Add an installed plugin to the configuration
    pub fn add_installed_plugin(&mut self, plugin: InstalledPlugin) {
        // Check if plugin already exists and update it, or add as new
        match self
            .installed_plugins
            .iter_mut()
            .find(|p| p.id == plugin.id)
        {
            Some(existing) => {
                // Update existing plugin info
                existing.name = plugin.name;
                existing.version = plugin.version;
                existing.source = plugin.source;
                existing.install_path = plugin.install_path;
                existing.enabled = plugin.enabled;
                existing.trust_level = plugin.trust_level;
                existing.installed_at = plugin.installed_at;
            }
            None => {
                self.installed_plugins.push(plugin);
            }
        }
    }

    /// Remove an installed plugin from the configuration
    pub fn remove_installed_plugin(&mut self, plugin_id: &str) -> bool {
        let initial_len = self.installed_plugins.len();
        self.installed_plugins.retain(|p| p.id != plugin_id);
        self.installed_plugins.len() != initial_len
    }

    /// Get an installed plugin by ID
    pub fn get_installed_plugin(&self, plugin_id: &str) -> Option<&InstalledPlugin> {
        self.installed_plugins.iter().find(|p| p.id == plugin_id)
    }

    /// Enable a plugin
    pub fn enable_plugin(&mut self, plugin_id: &str) -> Result<()> {
        match self
            .installed_plugins
            .iter_mut()
            .find(|p| p.id == plugin_id)
        {
            Some(plugin) => {
                plugin.enabled = true;
                Ok(())
            }
            None => bail!("Plugin '{}' not found in installed plugins", plugin_id),
        }
    }

    /// Disable a plugin
    pub fn disable_plugin(&mut self, plugin_id: &str) -> Result<()> {
        match self
            .installed_plugins
            .iter_mut()
            .find(|p| p.id == plugin_id)
        {
            Some(plugin) => {
                plugin.enabled = false;
                Ok(())
            }
            None => bail!("Plugin '{}' not found in installed plugins", plugin_id),
        }
    }
}

/// Helper function to get the marketplace config path based on VT Code's config directory structure
pub fn get_marketplace_config_path(base_config_dir: &Path) -> PathBuf {
    base_config_dir.join("marketplace.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_marketplace_settings_default() {
        let settings = MarketplaceSettings::default();
        assert!(settings.marketplaces.is_empty());
        assert!(settings.installed_plugins.is_empty());
        assert!(settings.auto_update.marketplaces);
        assert_eq!(settings.auto_update.check_interval_hours, 24);
        assert_eq!(
            settings.security.default_trust_level,
            crate::config::PluginTrustLevel::Sandbox
        );
    }

    #[tokio::test]
    async fn test_marketplace_settings_save_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test_marketplace.toml");

        let mut settings = MarketplaceSettings::default();
        settings.auto_update.marketplaces = false;
        settings.security.default_trust_level = crate::config::PluginTrustLevel::Trusted;

        settings.save_to_file(&config_path).await.unwrap();

        let loaded_settings = MarketplaceSettings::load_from_file(&config_path)
            .await
            .unwrap();
        assert_eq!(
            settings.auto_update.marketplaces,
            loaded_settings.auto_update.marketplaces
        );
        assert_eq!(
            settings.security.default_trust_level,
            loaded_settings.security.default_trust_level
        );
    }

    #[tokio::test]
    async fn test_add_remove_marketplace() {
        let mut settings = MarketplaceSettings::default();

        let marketplace = crate::marketplace::MarketplaceSource::Git {
            id: "test".to_string(),
            url: "https://example.com/test".to_string(),
            refspec: None,
        };

        settings.add_marketplace(marketplace.clone());
        assert_eq!(settings.marketplaces.len(), 1);

        settings.remove_marketplace("test");
        assert_eq!(settings.marketplaces.len(), 0);
    }

    #[tokio::test]
    async fn test_plugin_enable_disable() {
        let mut settings = MarketplaceSettings::default();

        let plugin = InstalledPlugin {
            id: "test-plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            source: "test-marketplace".to_string(),
            install_path: PathBuf::from("/tmp/test"),
            enabled: false,
            trust_level: crate::config::PluginTrustLevel::Sandbox,
            installed_at: "2023-01-01".to_string(),
        };

        settings.add_installed_plugin(plugin);

        settings.enable_plugin("test-plugin").unwrap();
        assert!(
            settings
                .get_installed_plugin("test-plugin")
                .unwrap()
                .enabled
        );

        settings.disable_plugin("test-plugin").unwrap();
        assert!(
            !settings
                .get_installed_plugin("test-plugin")
                .unwrap()
                .enabled
        );
    }
}
