//! Marketplace registry for managing known marketplaces

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{MarketplaceId, MarketplaceManifest, PluginManifest};

/// Source of a marketplace
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum MarketplaceSource {
    /// GitHub repository (owner/repo format)
    GitHub {
        id: String,
        owner: String,
        repo: String,
        refspec: Option<String>, // branch, tag, or commit
    },
    /// Git URL with optional refspec
    Git {
        id: String,
        url: String,
        refspec: Option<String>,
    },
    /// Local directory path
    Local {
        id: String,
        path: String,
    },
    /// Remote URL to a marketplace manifest
    Remote {
        id: String,
        url: String,
    },
}

impl MarketplaceSource {
    pub fn id(&self) -> &str {
        match self {
            MarketplaceSource::GitHub { id, .. } => id,
            MarketplaceSource::Git { id, .. } => id,
            MarketplaceSource::Local { id, .. } => id,
            MarketplaceSource::Remote { id, .. } => id,
        }
    }
}

/// Registry for managing marketplaces
pub struct MarketplaceRegistry {
    /// Base directory for marketplace data
    base_dir: PathBuf,
    
    /// Registered marketplaces
    marketplaces: RwLock<HashMap<MarketplaceId, MarketplaceSource>>,
    
    /// Cache of marketplace manifests
    manifest_cache: RwLock<HashMap<MarketplaceId, MarketplaceManifest>>,
}

impl MarketplaceRegistry {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            marketplaces: RwLock::new(HashMap::new()),
            manifest_cache: RwLock::new(HashMap::new()),
        }
    }
    
    /// Add a new marketplace source
    pub async fn add_marketplace(&self, source: MarketplaceSource) -> Result<()> {
        let mut marketplaces = self.marketplaces.write().await;
        marketplaces.insert(source.id().to_string(), source);
        Ok(())
    }
    
    /// Remove a marketplace by ID
    pub async fn remove_marketplace(&self, id: &str) -> Result<()> {
        let mut marketplaces = self.marketplaces.write().await;
        if marketplaces.remove(id).is_none() {
            bail!("Marketplace '{}' not found", id);
        }
        
        // Remove from cache as well
        let mut cache = self.manifest_cache.write().await;
        cache.remove(id);
        
        Ok(())
    }
    
    /// List all registered marketplaces
    pub async fn list_marketplaces(&self) -> Vec<MarketplaceSource> {
        let marketplaces = self.marketplaces.read().await;
        marketplaces.values().cloned().collect()
    }
    
    /// Get a specific marketplace source
    pub async fn get_marketplace(&self, id: &str) -> Option<MarketplaceSource> {
        let marketplaces = self.marketplaces.read().await;
        marketplaces.get(id).cloned()
    }
    
    /// Update marketplace manifest cache
    pub async fn update_marketplace(&self, id: &str) -> Result<()> {
        let source = {
            let marketplaces = self.marketplaces.read().await;
            marketplaces.get(id).cloned()
        };
        
        let source = match source {
            Some(s) => s,
            None => bail!("Marketplace '{}' not found", id),
        };
        
        let manifest = self.fetch_manifest(&source).await?;
        
        let mut cache = self.manifest_cache.write().await;
        cache.insert(id.to_string(), manifest);
        
        Ok(())
    }
    
    /// Fetch manifest from a source
    async fn fetch_manifest(&self, source: &MarketplaceSource) -> Result<MarketplaceManifest> {
        match source {
            MarketplaceSource::GitHub { owner, repo, refspec, .. } => {
                // For now, we'll simulate fetching from GitHub
                // In a real implementation, this would fetch from GitHub API
                self.fetch_github_manifest(owner, repo, refspec.as_deref()).await
            }
            MarketplaceSource::Git { url, refspec, .. } => {
                // For now, we'll simulate fetching from Git
                self.fetch_git_manifest(url, refspec.as_deref()).await
            }
            MarketplaceSource::Local { path, .. } => {
                self.fetch_local_manifest(path).await
            }
            MarketplaceSource::Remote { url, .. } => {
                self.fetch_remote_manifest(url).await
            }
        }
    }
    
    /// Fetch manifest from GitHub repository
    async fn fetch_github_manifest(&self, owner: &str, repo: &str, refspec: Option<&str>) -> Result<MarketplaceManifest> {
        // This is a placeholder implementation
        // In a real implementation, this would fetch from GitHub API
        println!("Fetching manifest from GitHub: {}/{} (ref: {:?})", owner, repo, refspec);
        
        // For now, return an empty manifest
        Ok(MarketplaceManifest {
            name: format!("{}-{}", owner, repo),
            description: format!("Marketplace from GitHub: {}/{}", owner, repo),
            plugins: vec![],
        })
    }
    
    /// Fetch manifest from Git repository
    async fn fetch_git_manifest(&self, url: &str, refspec: Option<&str>) -> Result<MarketplaceManifest> {
        // This is a placeholder implementation
        // In a real implementation, this would clone the repo and read the manifest
        println!("Fetching manifest from Git: {} (ref: {:?})", url, refspec);
        
        // For now, return an empty manifest
        Ok(MarketplaceManifest {
            name: url.to_string(),
            description: format!("Marketplace from Git: {}", url),
            plugins: vec![],
        })
    }
    
    /// Fetch manifest from local path
    async fn fetch_local_manifest(&self, path: &str) -> Result<MarketplaceManifest> {
        use tokio::fs;
        use std::path::Path;
        
        let manifest_path = Path::new(path).join(".vtcode-plugin/marketplace.json");
        let content = fs::read_to_string(&manifest_path)
            .await
            .with_context(|| format!("Failed to read marketplace manifest from {}", manifest_path.display()))?;
        
        let manifest: MarketplaceManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse marketplace manifest from {}", manifest_path.display()))?;
        
        Ok(manifest)
    }
    
    /// Fetch manifest from remote URL
    async fn fetch_remote_manifest(&self, url: &str) -> Result<MarketplaceManifest> {
        // This is a placeholder implementation
        // In a real implementation, this would make an HTTP request
        println!("Fetching manifest from remote URL: {}", url);
        
        // For now, return an empty manifest
        Ok(MarketplaceManifest {
            name: url.to_string(),
            description: format!("Marketplace from remote URL: {}", url),
            plugins: vec![],
        })
    }
    
    /// Get cached manifest for a marketplace
    pub async fn get_cached_manifest(&self, id: &str) -> Option<MarketplaceManifest> {
        let cache = self.manifest_cache.read().await;
        cache.get(id).cloned()
    }
    
    /// List all plugins from all registered marketplaces
    pub async fn list_all_plugins(&self) -> Vec<(MarketplaceId, PluginManifest)> {
        let mut all_plugins = Vec::new();
        
        let marketplaces = self.list_marketplaces().await;
        for marketplace in marketplaces {
            if let Some(manifest) = self.get_cached_manifest(marketplace.id()).await {
                for plugin in manifest.plugins {
                    all_plugins.push((marketplace.id().to_string(), plugin));
                }
            }
        }
        
        all_plugins
    }
    
    /// Find a specific plugin across all marketplaces
    pub async fn find_plugin(&self, plugin_id: &str) -> Option<(MarketplaceId, PluginManifest)> {
        let all_plugins = self.list_all_plugins().await;
        all_plugins.into_iter()
            .find(|(_, plugin)| plugin.id == plugin_id)
    }
}