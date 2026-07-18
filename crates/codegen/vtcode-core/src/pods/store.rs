use super::catalog::PodCatalog;
use super::state::PodsState;
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use vtcode_commons::fs::{ensure_dir_exists, read_json_file, write_json_file};

/// Persisted pod storage rooted in `~/.vtcode/pods`.
#[derive(Debug, Clone)]
pub struct PodsStore {
    base_dir: PathBuf,
}

impl PodsStore {
    /// Create a store rooted at the given directory.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    /// Create a store using the default `~/.vtcode/pods` directory.
    pub fn default_store() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
        Ok(Self::new(home.join(".vtcode").join("pods")))
    }

    /// Return the root directory of this store.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Return the path to the `state.json` file.
    pub fn state_path(&self) -> PathBuf {
        self.base_dir.join("state.json")
    }

    /// Return the path to the `catalog.json` file.
    pub fn catalog_path(&self) -> PathBuf {
        self.base_dir.join("catalog.json")
    }

    /// Create the base directory and seed default files if they do not exist.
    pub async fn ensure_initialized(&self) -> Result<()> {
        ensure_dir_exists(&self.base_dir).await?;

        if !tokio::fs::try_exists(&self.catalog_path()).await.unwrap_or(false) {
            self.save_catalog(&PodCatalog::embedded_default()).await?;
        }

        if !tokio::fs::try_exists(&self.state_path()).await.unwrap_or(false) {
            self.save_state(&PodsState::default()).await?;
        }

        Ok(())
    }

    /// Load the persisted pod state from disk.
    pub async fn load_state(&self) -> Result<PodsState> {
        self.ensure_initialized().await?;
        read_json_file(&self.state_path())
            .await
            .with_context(|| format!("failed to read pod state at {}", self.state_path().display()))
    }

    /// Persist the pod state to disk.
    pub async fn save_state(&self, state: &PodsState) -> Result<()> {
        ensure_dir_exists(&self.base_dir).await?;
        write_json_file(&self.state_path(), state)
            .await
            .with_context(|| format!("failed to write pod state at {}", self.state_path().display()))
    }

    /// Load the deployment catalog from disk.
    pub async fn load_catalog(&self) -> Result<PodCatalog> {
        self.ensure_initialized().await?;
        read_json_file(&self.catalog_path())
            .await
            .with_context(|| format!("failed to read pod catalog at {}", self.catalog_path().display()))
    }

    pub async fn save_catalog(&self, catalog: &PodCatalog) -> Result<()> {
        ensure_dir_exists(&self.base_dir).await?;
        write_json_file(&self.catalog_path(), catalog)
            .await
            .with_context(|| format!("failed to write pod catalog at {}", self.catalog_path().display()))
    }
}
