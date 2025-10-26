//! Self-update mechanism for vtcode
//!
//! This module provides automatic version checking, downloading updates from GitHub releases,
//! verifying binary integrity, managing backups and rollbacks, and handling cross-platform
//! compatibility.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod checker;
mod config;
mod downloader;
mod installer;
mod rollback;
mod verifier;

pub use checker::UpdateChecker;
pub use config::{UpdateChannel, UpdateConfig, UpdateFrequency};
pub use downloader::UpdateDownloader;
pub use installer::UpdateInstaller;
pub use rollback::RollbackManager;
pub use verifier::UpdateVerifier;

/// Current version of vtcode
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// GitHub repository for releases
pub const GITHUB_REPO_OWNER: &str = "vinhnx";
pub const GITHUB_REPO_NAME: &str = "vtcode";

/// Update status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub download_url: Option<String>,
    pub release_notes: Option<String>,
    pub last_checked: Option<chrono::DateTime<chrono::Utc>>,
}

/// Update result after installation
#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub success: bool,
    pub old_version: String,
    pub new_version: String,
    pub backup_path: Option<PathBuf>,
    pub requires_restart: bool,
}

/// Main update manager coordinating all update operations
pub struct UpdateManager {
    config: UpdateConfig,
    checker: UpdateChecker,
    downloader: UpdateDownloader,
    installer: UpdateInstaller,
    verifier: UpdateVerifier,
    rollback: RollbackManager,
}

impl UpdateManager {
    /// Create a new update manager with the given configuration
    pub fn new(config: UpdateConfig) -> Result<Self> {
        let checker = UpdateChecker::new(config.clone())?;
        let downloader = UpdateDownloader::new(config.clone())?;
        let installer = UpdateInstaller::new(config.clone())?;
        let verifier = UpdateVerifier::new(config.clone())?;
        let rollback = RollbackManager::new(config.clone())?;

        Ok(Self {
            config,
            checker,
            downloader,
            installer,
            verifier,
            rollback,
        })
    }

    /// Check if an update is available
    pub async fn check_for_updates(&self) -> Result<UpdateStatus> {
        self.checker.check_for_updates().await
    }

    /// Download and install an available update
    pub async fn perform_update(&mut self) -> Result<UpdateResult> {
        // Check for updates
        let status = self.check_for_updates().await?;

        if !status.update_available {
            anyhow::bail!("No update available");
        }

        let download_url = status.download_url.context("No download URL available")?;
        let new_version = status
            .latest_version
            .context("No version information available")?;

        // Create backup before updating
        let backup_path = self
            .rollback
            .create_backup()
            .context("Failed to create backup")?;

        // Download the update
        let download_path = self
            .downloader
            .download(&download_url)
            .await
            .context("Failed to download update")?;

        // Verify the downloaded binary
        self.verifier
            .verify(&download_path)
            .await
            .context("Failed to verify update")?;

        // Install the update
        match self.installer.install(&download_path).await {
            Ok(_) => Ok(UpdateResult {
                success: true,
                old_version: CURRENT_VERSION.to_string(),
                new_version,
                backup_path: Some(backup_path),
                requires_restart: true,
            }),
            Err(e) => {
                // Rollback on failure
                self.rollback
                    .rollback(&backup_path)
                    .context("Failed to rollback after installation failure")?;
                Err(e).context("Update installation failed and was rolled back")
            }
        }
    }

    /// Rollback to a previous version
    pub fn rollback_to_backup(&self, backup_path: &PathBuf) -> Result<()> {
        self.rollback.rollback(backup_path)
    }

    /// Clean up old backups
    pub fn cleanup_old_backups(&self) -> Result<()> {
        self.rollback.cleanup_old_backups()
    }

    /// Get the current configuration
    pub fn config(&self) -> &UpdateConfig {
        &self.config
    }

    /// Update the configuration
    pub fn set_config(&mut self, config: UpdateConfig) -> Result<()> {
        self.config = config.clone();
        self.checker = UpdateChecker::new(config.clone())?;
        self.downloader = UpdateDownloader::new(config.clone())?;
        self.installer = UpdateInstaller::new(config.clone())?;
        self.verifier = UpdateVerifier::new(config.clone())?;
        self.rollback = RollbackManager::new(config)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_version() {
        assert!(!CURRENT_VERSION.is_empty());
        assert!(CURRENT_VERSION.contains('.'));
    }

    #[test]
    fn test_github_repo_constants() {
        assert_eq!(GITHUB_REPO_OWNER, "vinhnx");
        assert_eq!(GITHUB_REPO_NAME, "vtcode");
    }
}
