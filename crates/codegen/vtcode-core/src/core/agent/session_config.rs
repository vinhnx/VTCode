use std::path::Path;

use anyhow::Result;
use hashbrown::HashMap;

use crate::config::VTCodeConfig;
use crate::config::loader::ConfigManager;
use crate::config::loader::layers::ConfigLayerMetadata;
use crate::core::agent::features::FeatureSet;

/// Immutable session-scoped configuration snapshot.
///
/// This captures the resolved effective configuration and the winning layer for
/// each config path at session start so downstream runtime code can treat
/// configuration as frozen for the life of the session.
#[derive(Debug, Clone)]
pub struct ResolvedSessionConfig {
    effective: VTCodeConfig,
    features: FeatureSet,
    origins: HashMap<String, ConfigLayerMetadata>,
}

impl ResolvedSessionConfig {
    /// Build a session snapshot from an already-loaded configuration manager.
    pub fn from_manager(manager: &ConfigManager) -> Self {
        let (_, origins) = manager.layer_stack().effective_config_with_origins();
        let effective = manager.config().clone();
        Self {
            features: FeatureSet::from_config(Some(&effective)),
            effective,
            origins,
        }
    }

    /// Build a session snapshot from a raw config when layer metadata is not available.
    pub fn from_config(config: VTCodeConfig) -> Self {
        let features = FeatureSet::from_config(Some(&config));
        Self {
            effective: config,
            features,
            origins: HashMap::new(),
        }
    }

    /// Load and resolve a session snapshot for the provided workspace.
    pub fn load_from_workspace(workspace: impl AsRef<Path>) -> Result<Self> {
        let manager = ConfigManager::load_from_workspace(workspace)?;
        Ok(Self::from_manager(&manager))
    }

    /// Return the effective frozen configuration.
    pub fn effective(&self) -> &VTCodeConfig {
        &self.effective
    }

    /// Return the origin metadata map for resolved config paths.
    pub fn origins(&self) -> &HashMap<String, ConfigLayerMetadata> {
        &self.origins
    }

    /// Return the immutable session-scoped feature flags.
    pub fn features(&self) -> &FeatureSet {
        &self.features
    }

    /// Return the winning layer metadata for a config path, if present.
    pub fn origin_for(&self, path: &str) -> Option<&ConfigLayerMetadata> {
        self.origins.get(path)
    }
}

#[cfg(test)]
mod tests {
    use super::ResolvedSessionConfig;
    use anyhow::Result;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn captures_origin_metadata_from_workspace_config() -> Result<()> {
        let temp = TempDir::new()?;
        fs::write(
            temp.path().join("vtcode.toml"),
            "[agent]\nprovider = \"openai\"\n",
        )?;

        let snapshot = ResolvedSessionConfig::load_from_workspace(temp.path())?;

        assert_eq!(snapshot.effective().agent.provider, "openai");
        assert!(
            snapshot.origin_for("agent.provider").is_some(),
            "expected origin metadata for agent.provider"
        );

        Ok(())
    }

    #[test]
    fn snapshot_is_immutable_after_disk_changes() -> Result<()> {
        let temp = TempDir::new()?;
        let config_path = temp.path().join("vtcode.toml");
        fs::write(&config_path, "[agent]\nprovider = \"openai\"\n")?;

        let snapshot = ResolvedSessionConfig::load_from_workspace(temp.path())?;
        fs::write(&config_path, "[agent]\nprovider = \"anthropic\"\n")?;

        assert_eq!(snapshot.effective().agent.provider, "openai");

        let refreshed = ResolvedSessionConfig::load_from_workspace(temp.path())?;
        assert_eq!(refreshed.effective().agent.provider, "anthropic");

        Ok(())
    }
}
