use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::RwLock;

use crate::config::{PluginRuntimeConfig, PluginTrustLevel};
use crate::tools::registry::ToolRegistration;
use crate::tools::ToolRegistry;
use crate::utils::error_messages::{ERR_DESERIALIZE, ERR_READ_FILE};

pub type PluginId = String;

/// Declarative metadata for a plugin manifest.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    /// Unique identifier. Falls back to the `name` when omitted.
    #[serde(default)]
    pub id: PluginId,

    /// Human-readable plugin name.
    pub name: String,

    /// Semantic version string for compatibility checks.
    #[serde(default)]
    pub version: String,

    /// Short description of the plugin.
    #[serde(default)]
    pub description: String,

    /// Entrypoint script or binary to launch.
    #[serde(default)]
    pub entrypoint: PathBuf,

    /// Capability descriptors advertised by the plugin.
    #[serde(default)]
    pub capabilities: Vec<String>,

    /// Optional explicit trust level.
    #[serde(default)]
    pub trust_level: Option<PluginTrustLevel>,

    /// Arbitrary metadata for adapters (environment, labels, etc.).
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl PluginManifest {
    fn normalized(mut self, default_trust: PluginTrustLevel) -> Self {
        if self.id.is_empty() {
            self.id = self.name.clone();
        }

        if self.trust_level.is_none() {
            self.trust_level = Some(default_trust);
        }

        self
    }
}

#[derive(Debug, Clone)]
pub struct PluginHandle {
    pub manifest: PluginManifest,
    pub manifest_path: PathBuf,
    pub loaded_at: SystemTime,
}

#[async_trait]
pub trait PluginInstaller: Send + Sync {
    /// Convert a manifest into a runtime tool registration.
    async fn materialize(&self, manifest: &PluginManifest) -> Result<ToolRegistration>;
}

/// Runtime for loading, tracking, and hot-swapping plugin manifests.
#[derive(Debug)]
pub struct PluginRuntime {
    workspace_root: PathBuf,
    config: PluginRuntimeConfig,
    plugins: RwLock<HashMap<PluginId, PluginHandle>>,
}

impl PluginRuntime {
    pub fn new(workspace_root: PathBuf, config: PluginRuntimeConfig) -> Self {
        Self {
            workspace_root,
            config,
            plugins: RwLock::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> &PluginRuntimeConfig {
        &self.config
    }

    pub async fn load_manifest(&self, manifest_path: impl AsRef<Path>) -> Result<PluginManifest> {
        let path = manifest_path.as_ref();
        let data = fs::read_to_string(path)
            .await
            .with_context(|| format!("{ERR_READ_FILE}: {}", path.display()))?;

        let manifest: PluginManifest = toml::from_str(&data)
            .with_context(|| format!("{ERR_DESERIALIZE}: {}", path.display()))?;

        Ok(manifest.normalized(self.config.default_trust))
    }

    pub async fn register_manifest(
        &self,
        manifest_path: impl AsRef<Path>,
    ) -> Result<PluginHandle> {
        let path = manifest_path.as_ref();
        ensure!(
            self.config.enabled,
            "plugin runtime disabled by configuration"
        );

        let manifest = self.load_manifest(path).await?;
        self.validate_trust(&manifest)?;

        let handle = PluginHandle {
            manifest: manifest.clone(),
            manifest_path: path.to_path_buf(),
            loaded_at: SystemTime::now(),
        };

        let mut plugins = self.plugins.write().await;
        plugins.insert(manifest.id.clone(), handle.clone());
        Ok(handle)
    }

    pub async fn hot_swap(&self, manifest_path: impl AsRef<Path>) -> Result<PluginHandle> {
        let handle = self.register_manifest(manifest_path).await?;
        Ok(handle)
    }

    pub async fn attach_to_registry(
        &self,
        registry: &mut ToolRegistry,
        manifest: &PluginManifest,
        installer: &dyn PluginInstaller,
    ) -> Result<()> {
        ensure!(
            self.config.enabled,
            "plugin runtime disabled by configuration"
        );
        self.validate_trust(manifest)?;

        let registration = installer.materialize(manifest).await?;
        registry.register_tool(registration)?;
        Ok(())
    }

    pub async fn list_registered(&self) -> Vec<PluginHandle> {
        let plugins = self.plugins.read().await;
        plugins.values().cloned().collect()
    }

    fn validate_trust(&self, manifest: &PluginManifest) -> Result<()> {
        if self
            .config
            .deny
            .iter()
            .any(|blocked| blocked == &manifest.id)
        {
            bail!("plugin {} is blocked by deny list", manifest.id);
        }

        if !self.config.allow.is_empty()
            && !self
                .config
                .allow
                .iter()
                .any(|allowed| allowed == &manifest.id)
        {
            bail!("plugin {} not present in allow list", manifest.id);
        }

        let trust = manifest.trust_level.unwrap_or(self.config.default_trust);
        ensure!(
            matches!(
                trust,
                PluginTrustLevel::Sandbox | PluginTrustLevel::Trusted | PluginTrustLevel::Untrusted
            ),
            "invalid trust level for plugin {}",
            manifest.id
        );
        Ok(())
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_toml(id: &str) -> String {
        format!(
            r#"
name = "{id}"
version = "0.1.0"
description = "test plugin"
entrypoint = "bin/plugin"
"#
        )
    }

    #[tokio::test]
    async fn deny_list_blocks_manifest() {
        let tmp_dir = std::env::temp_dir();
        let manifest_path = tmp_dir.join("vtcode_plugin_deny.toml");
        fs::write(&manifest_path, manifest_toml("blocked"))
            .await
            .expect("write manifest");

        let runtime = PluginRuntime::new(
            tmp_dir.clone(),
            PluginRuntimeConfig {
                deny: vec!["blocked".into()],
                ..PluginRuntimeConfig::default()
            },
        );

        let err = runtime.register_manifest(&manifest_path).await.unwrap_err();
        assert!(
            err.to_string().contains("blocked"),
            "expected deny list rejection"
        );
    }

    #[tokio::test]
    async fn allow_list_enforced() {
        let tmp_dir = std::env::temp_dir();
        let manifest_path = tmp_dir.join("vtcode_plugin_allow.toml");
        fs::write(&manifest_path, manifest_toml("allowed"))
            .await
            .expect("write manifest");

        let runtime = PluginRuntime::new(
            tmp_dir.clone(),
            PluginRuntimeConfig {
                allow: vec!["allowed".into()],
                ..PluginRuntimeConfig::default()
            },
        );

        let handle = runtime
            .register_manifest(&manifest_path)
            .await
            .expect("allowed manifest to register");

        assert_eq!(handle.manifest.id, "allowed");
    }
}
