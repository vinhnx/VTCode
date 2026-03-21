use crate::pods::catalog::PodCatalog;
use crate::pods::state::PodsState;
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use vtcode_commons::fs::{ensure_dir_exists, read_json_file, write_json_file};

/// Persisted pod storage rooted in `~/.vtcode/pods`.
#[derive(Debug, Clone)]
pub struct PodsStore {
    base_dir: PathBuf,
}

impl PodsStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    pub fn default_store() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
        Ok(Self::new(home.join(".vtcode").join("pods")))
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn state_path(&self) -> PathBuf {
        self.base_dir.join("state.json")
    }

    pub fn catalog_path(&self) -> PathBuf {
        self.base_dir.join("catalog.json")
    }

    pub async fn ensure_initialized(&self) -> Result<()> {
        ensure_dir_exists(&self.base_dir).await?;

        if !tokio::fs::try_exists(&self.catalog_path())
            .await
            .unwrap_or(false)
        {
            self.save_catalog(&PodCatalog::embedded_default()).await?;
        }

        if !tokio::fs::try_exists(&self.state_path())
            .await
            .unwrap_or(false)
        {
            self.save_state(&PodsState::default()).await?;
        }

        Ok(())
    }

    pub async fn load_state(&self) -> Result<PodsState> {
        self.ensure_initialized().await?;
        read_json_file(&self.state_path()).await.with_context(|| {
            format!(
                "failed to read pod state at {}",
                self.state_path().display()
            )
        })
    }

    pub async fn save_state(&self, state: &PodsState) -> Result<()> {
        ensure_dir_exists(&self.base_dir).await?;
        write_json_file(&self.state_path(), state)
            .await
            .with_context(|| {
                format!(
                    "failed to write pod state at {}",
                    self.state_path().display()
                )
            })
    }

    pub async fn load_catalog(&self) -> Result<PodCatalog> {
        self.ensure_initialized().await?;
        read_json_file(&self.catalog_path()).await.with_context(|| {
            format!(
                "failed to read pod catalog at {}",
                self.catalog_path().display()
            )
        })
    }

    pub async fn save_catalog(&self, catalog: &PodCatalog) -> Result<()> {
        ensure_dir_exists(&self.base_dir).await?;
        write_json_file(&self.catalog_path(), catalog)
            .await
            .with_context(|| {
                format!(
                    "failed to write pod catalog at {}",
                    self.catalog_path().display()
                )
            })
    }
}
