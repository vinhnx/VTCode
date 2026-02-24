//! Plugin marketplace system for VT Code
//!
//! This module implements a marketplace system for VT Code,
//! allowing users to discover and install plugins from various sources.

use std::path::PathBuf;

use crate::tools::plugins::PluginRuntime;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub mod config;
pub mod installer;
pub mod manifest;
pub mod registry;
pub mod testing;

pub use installer::PluginInstaller;
pub use manifest::{MarketplaceManifest, PluginManifest};
pub use registry::{MarketplaceRegistry, MarketplaceSource};

/// Type alias for marketplace identifiers
pub type MarketplaceId = String;

/// Type alias for plugin identifiers
pub type PluginId = String;

/// Configuration for marketplace system
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarketplaceConfig {
    /// Enable marketplace functionality
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Auto-update marketplaces at startup
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,

    /// Default trust level for installed plugins
    #[serde(default)]
    pub default_trust: crate::config::PluginTrustLevel,

    /// List of default marketplaces to include
    #[serde(default)]
    pub default_marketplaces: Vec<MarketplaceSource>,
}

impl Default for MarketplaceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_update: true,
            default_trust: crate::config::PluginTrustLevel::Sandbox,
            default_marketplaces: vec![MarketplaceSource::Git {
                id: "vtcode-official".to_string(),
                url: "https://github.com/vinhnx/vtcode-marketplace".to_string(),
                refspec: None,
            }],
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_auto_update() -> bool {
    true
}

/// Main marketplace system that coordinates all marketplace functionality
pub struct MarketplaceSystem {
    /// Registry for managing marketplaces
    pub registry: MarketplaceRegistry,

    /// Installer for managing plugin installations
    pub installer: PluginInstaller,

    /// Configuration for the marketplace system
    pub config: MarketplaceConfig,
}

impl MarketplaceSystem {
    /// Create a new marketplace system
    pub fn new(
        base_dir: PathBuf,
        config: MarketplaceConfig,
        plugin_runtime: Option<PluginRuntime>,
    ) -> Self {
        let registry = MarketplaceRegistry::new(base_dir.clone());
        let installer = PluginInstaller::new(base_dir.join("plugins"), plugin_runtime);

        Self {
            registry,
            installer,
            config,
        }
    }

    /// Initialize the marketplace system with default marketplaces
    pub async fn initialize(&self) -> Result<()> {
        // Add default marketplaces if enabled
        for marketplace in &self.config.default_marketplaces {
            self.registry.add_marketplace(marketplace.clone()).await?;
        }

        Ok(())
    }
}
