//! Marketplace and plugin manifest formats

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Marketplace manifest format
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarketplaceManifest {
    /// Name of the marketplace
    pub name: String,
    
    /// Description of the marketplace
    pub description: String,
    
    /// List of plugins available in this marketplace
    pub plugins: Vec<PluginManifest>,
}

/// Plugin manifest format - similar to existing PluginManifest but with marketplace additions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    /// Unique identifier for the plugin
    pub id: String,
    
    /// Human-readable name
    pub name: String,
    
    /// Semantic version
    pub version: String,
    
    /// Description of the plugin
    pub description: String,
    
    /// Entrypoint for the plugin
    pub entrypoint: PathBuf,
    
    /// Capabilities provided by the plugin
    pub capabilities: Vec<String>,
    
    /// Source URL where the plugin can be downloaded
    pub source: String,
    
    /// Optional trust level
    pub trust_level: Option<crate::config::PluginTrustLevel>,
    
    /// Dependencies (other plugins or system requirements)
    #[serde(default)]
    pub dependencies: Vec<String>,
    
    /// Author information
    #[serde(default)]
    pub author: String,
    
    /// License information
    #[serde(default)]
    pub license: String,
    
    /// Homepage URL
    #[serde(default)]
    pub homepage: String,
    
    /// Repository URL
    #[serde(default)]
    pub repository: String,
}

impl PluginManifest {
    pub fn new(id: String, name: String, version: String) -> Self {
        Self {
            id,
            name,
            version,
            description: String::new(),
            entrypoint: PathBuf::new(),
            capabilities: Vec::new(),
            source: String::new(),
            trust_level: None,
            dependencies: Vec::new(),
            author: String::new(),
            license: String::new(),
            homepage: String::new(),
            repository: String::new(),
        }
    }
}