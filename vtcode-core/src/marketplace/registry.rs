//! Marketplace registry for managing known marketplaces

use std::collections::HashMap;
use std::path::PathBuf;

use crate::utils::file_utils::{parse_json_with_context, read_json_file};
use crate::utils::http_client;
use anyhow::{Context, Result, bail};
use base64;
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
    Local { id: String, path: String },
    /// Remote URL to a marketplace manifest
    Remote { id: String, url: String },
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
    #[allow(dead_code)]
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
            MarketplaceSource::GitHub {
                owner,
                repo,
                refspec,
                ..
            } => {
                // Fetch manifest from GitHub API using the authenticated client
                self.fetch_github_manifest(owner, repo, refspec.as_deref())
                    .await
            }
            MarketplaceSource::Git { url, refspec, .. } => {
                // Fetch manifest by cloning the git repository
                self.fetch_git_manifest(url, refspec.as_deref()).await
            }
            MarketplaceSource::Local { path, .. } => {
                // Fetch manifest from local directory
                self.fetch_local_manifest(path).await
            }
            MarketplaceSource::Remote { url, .. } => {
                // Fetch manifest from remote HTTP/HTTPS URL
                self.fetch_remote_manifest(url).await
            }
        }
    }

    /// Fetch manifest from GitHub repository
    async fn fetch_github_manifest(
        &self,
        owner: &str,
        repo: &str,
        refspec: Option<&str>,
    ) -> Result<MarketplaceManifest> {
        use serde_json::Value;

        // Determine the refspec (default to 'main' if not specified)
        let refspec = refspec.unwrap_or("main");

        // Construct the GitHub API URL to fetch the file
        let api_url = format!(
            "https://api.github.com/repos/{}/{}/contents/.vtcode-plugin/marketplace.json?ref={}",
            owner, repo, refspec
        );

        // Create HTTP client with appropriate headers
        let client = http_client::create_client_with_user_agent("vtcode");
        let response = client
            .get(&api_url)
            .header("User-Agent", "vtcode")
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| {
                format!(
                    "Failed to fetch manifest from GitHub: {}/{} (ref: {})",
                    owner, repo, refspec
                )
            })?;

        if !response.status().is_success() {
            if response.status() == 404 {
                bail!(
                    "Marketplace manifest not found in GitHub repository: {}/{} (ref: {})",
                    owner,
                    repo,
                    refspec
                );
            } else {
                bail!(
                    "Failed to fetch manifest from GitHub API: HTTP {} - {}",
                    response.status(),
                    response.text().await.unwrap_or_default()
                );
            }
        }

        // Parse the GitHub API response
        let json_response: Value = response.json().await.with_context(|| {
            format!("Failed to parse GitHub API response for {}/{}", owner, repo)
        })?;

        // Extract the content from the response
        let content_encoded = json_response
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("GitHub API response missing content field"))?;

        // Decode the base64 content
        let content_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, content_encoded)
                .with_context(|| {
                    format!(
                        "Failed to decode base64 content from GitHub: {}/{}",
                        owner, repo
                    )
                })?;

        let content = String::from_utf8(content_bytes).with_context(|| {
            format!(
                "Failed to decode UTF-8 content from GitHub: {}/{}",
                owner, repo
            )
        })?;

        // Parse the manifest from the content
        parse_json_with_context(&content, &format!("GitHub: {}/{}", owner, repo))
    }

    /// Fetch manifest from Git repository
    async fn fetch_git_manifest(
        &self,
        url: &str,
        refspec: Option<&str>,
    ) -> Result<MarketplaceManifest> {
        use tempfile::TempDir;
        use tokio::process::Command;

        // Create a temporary directory for the git clone
        let temp_dir =
            TempDir::new().with_context(|| "Failed to create temporary directory for git clone")?;
        let temp_path = temp_dir.path();

        // Build the git clone command
        let mut git_cmd = Command::new("git");
        git_cmd.arg("clone").arg(url).arg(temp_path);

        // Add branch/tag/commit if specified
        if let Some(refspec) = refspec {
            git_cmd.arg("--branch").arg(refspec);
        }

        // Execute the git clone
        let output = git_cmd
            .output()
            .await
            .with_context(|| format!("Failed to execute git clone for {}", url))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Git clone failed for {}: {}", url, stderr);
        }

        // Look for the manifest file in the cloned repository
        let manifest_path = temp_path.join(".vtcode-plugin/marketplace.json");
        if !manifest_path.exists() {
            bail!("Marketplace manifest not found in repository: {}", url);
        }

        read_json_file(&manifest_path).await
    }

    /// Fetch manifest from local path
    async fn fetch_local_manifest(&self, path: &str) -> Result<MarketplaceManifest> {
        use std::path::Path;

        let manifest_path = Path::new(path).join(".vtcode-plugin/marketplace.json");
        read_json_file(&manifest_path).await
    }

    /// Fetch manifest from remote URL
    async fn fetch_remote_manifest(&self, url: &str) -> Result<MarketplaceManifest> {
        let client = http_client::create_default_client();
        let response = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch remote manifest from {}", url))?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch remote manifest: HTTP {}",
                response.status()
            );
        }

        let content = response
            .text()
            .await
            .with_context(|| format!("Failed to read response body from {}", url))?;

        parse_json_with_context(&content, &format!("remote manifest: {}", url))
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
        all_plugins
            .into_iter()
            .find(|(_, plugin)| plugin.id == plugin_id)
    }
}
