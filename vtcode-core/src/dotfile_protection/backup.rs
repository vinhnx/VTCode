//! Backup and restore functionality for dotfiles.
//!
//! Creates versioned backups before any permitted modification,
//! preserving original permissions and ownership.

#[cfg(unix)]
use std::fs::Permissions;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::utils::file_utils::{ensure_dir_exists, read_json_file, write_json_file};

/// Metadata for a dotfile backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotfileBackup {
    /// Original file path.
    pub original_path: String,
    /// Backup file path.
    pub backup_path: String,
    /// Timestamp of backup creation.
    pub created_at: DateTime<Utc>,
    /// SHA-256 hash of the original content.
    pub content_hash: String,
    /// Original file size in bytes.
    pub size_bytes: u64,
    /// Original file permissions (Unix mode).
    #[cfg(unix)]
    pub permissions: u32,
    /// Reason for the backup.
    pub reason: String,
    /// Session that triggered the backup.
    pub session_id: String,
}

impl DotfileBackup {
    /// Restore this backup to the original location.
    pub async fn restore(&self) -> Result<()> {
        let backup_path = Path::new(&self.backup_path);
        let original_path = Path::new(&self.original_path);

        if !backup_path.exists() {
            bail!("Backup file does not exist: {}", self.backup_path);
        }

        // Verify backup integrity
        let content = tokio::fs::read(backup_path)
            .await
            .with_context(|| format!("Failed to read backup: {}", self.backup_path))?;

        let hash = format!("{:x}", Sha256::digest(&content));
        if hash != self.content_hash {
            bail!(
                "Backup integrity check failed: hash mismatch for {}",
                self.backup_path
            );
        }

        // Restore content
        tokio::fs::write(original_path, &content)
            .await
            .with_context(|| format!("Failed to restore to: {}", self.original_path))?;

        // Restore permissions
        #[cfg(unix)]
        {
            let perms = Permissions::from_mode(self.permissions);
            tokio::fs::set_permissions(original_path, perms)
                .await
                .with_context(|| {
                    format!("Failed to restore permissions for: {}", self.original_path)
                })?;
        }

        tracing::info!(
            "Restored dotfile {} from backup {}",
            self.original_path,
            self.backup_path
        );

        Ok(())
    }
}

/// Manager for dotfile backups.
pub struct BackupManager {
    /// Base directory for backups.
    backup_dir: PathBuf,
    /// Maximum backups to retain per file.
    max_backups: usize,
}

impl BackupManager {
    /// Create a new backup manager.
    pub async fn new(backup_dir: impl AsRef<Path>, max_backups: usize) -> Result<Self> {
        let backup_dir = backup_dir.as_ref().to_path_buf();

        // Create backup directory if it doesn't exist
        ensure_dir_exists(&backup_dir)
            .await
            .with_context(|| format!("Failed to create backup directory: {:?}", backup_dir))?;

        Ok(Self {
            backup_dir,
            max_backups,
        })
    }

    /// Create a backup of a dotfile before modification.
    pub async fn create_backup(
        &self,
        file_path: &Path,
        reason: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Result<DotfileBackup> {
        if !file_path.exists() {
            bail!("Cannot backup non-existent file: {:?}", file_path);
        }

        // Read original content
        let content = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("Failed to read file for backup: {:?}", file_path))?;

        // Get file metadata
        let metadata = tokio::fs::metadata(file_path)
            .await
            .with_context(|| format!("Failed to get metadata: {:?}", file_path))?;

        // Compute content hash
        let content_hash = format!("{:x}", Sha256::digest(&content));

        // Generate backup path
        let timestamp = Utc::now();
        let safe_name = self.safe_filename(file_path);
        let backup_filename = format!(
            "{}.{}.backup",
            safe_name,
            timestamp.format("%Y%m%d_%H%M%S_%3f")
        );
        let backup_path = self.backup_dir.join(&backup_filename);

        // Write backup
        tokio::fs::write(&backup_path, &content)
            .await
            .with_context(|| format!("Failed to write backup: {:?}", backup_path))?;

        // Preserve permissions on backup
        #[cfg(unix)]
        {
            let perms = metadata.permissions();
            tokio::fs::set_permissions(&backup_path, perms.clone())
                .await
                .with_context(|| format!("Failed to set backup permissions: {:?}", backup_path))?;
        }

        #[cfg(unix)]
        let permissions = metadata.permissions().mode();

        let backup = DotfileBackup {
            original_path: file_path.to_string_lossy().into_owned(),
            backup_path: backup_path.to_string_lossy().into_owned(),
            created_at: timestamp,
            content_hash,
            size_bytes: metadata.len(),
            #[cfg(unix)]
            permissions,
            reason: reason.into(),
            session_id: session_id.into(),
        };

        // Save backup metadata
        self.save_backup_metadata(&backup).await?;

        // Cleanup old backups
        self.cleanup_old_backups(file_path).await?;

        tracing::info!("Created backup for {:?} at {:?}", file_path, backup_path);

        Ok(backup)
    }

    /// Convert a file path to a safe filename for backup.
    fn safe_filename(&self, path: &Path) -> String {
        path.to_string_lossy()
            .replace(['/', '\\', ':', '.'], "_")
            .trim_start_matches('_')
            .to_string()
    }

    /// Save backup metadata to a JSON index.
    async fn save_backup_metadata(&self, backup: &DotfileBackup) -> Result<()> {
        let index_path = self.backup_dir.join("backups.json");

        let mut backups = self.load_backup_index().await.unwrap_or_default();
        backups.push(backup.clone());

        write_json_file(&index_path, &backups)
            .await
            .with_context(|| format!("Failed to write backup index: {:?}", index_path))?;

        Ok(())
    }

    /// Load the backup index.
    async fn load_backup_index(&self) -> Result<Vec<DotfileBackup>> {
        let index_path = self.backup_dir.join("backups.json");

        if !index_path.exists() {
            return Ok(Vec::new());
        }

        let backups: Vec<DotfileBackup> = read_json_file(&index_path)
            .await
            .with_context(|| format!("Failed to parse backup index: {:?}", index_path))?;

        Ok(backups)
    }

    /// Cleanup old backups, keeping only the most recent N.
    async fn cleanup_old_backups(&self, file_path: &Path) -> Result<()> {
        let backups = self.load_backup_index().await?;
        let file_path_str = file_path.to_string_lossy();

        // Get backups for this file, sorted by date (newest first)
        let mut file_backups: Vec<_> = backups
            .iter()
            .filter(|b| b.original_path == file_path_str)
            .collect();

        file_backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Delete old backups beyond max_backups
        for backup in file_backups.iter().skip(self.max_backups) {
            let backup_path = Path::new(&backup.backup_path);
            if backup_path.exists() {
                if let Err(e) = tokio::fs::remove_file(backup_path).await {
                    tracing::warn!("Failed to remove old backup {:?}: {}", backup_path, e);
                } else {
                    tracing::debug!("Removed old backup: {:?}", backup_path);
                }
            }
        }

        // Update index (remove deleted backups)
        let remaining: Vec<_> = backups
            .into_iter()
            .filter(|b| {
                if b.original_path == file_path_str {
                    Path::new(&b.backup_path).exists()
                } else {
                    true
                }
            })
            .collect();

        let index_path = self.backup_dir.join("backups.json");
        write_json_file(&index_path, &remaining)
            .await
            .with_context(|| "Failed to update backup index")?;

        Ok(())
    }

    /// Get all backups for a specific file.
    pub async fn get_backups_for_file(&self, file_path: &Path) -> Result<Vec<DotfileBackup>> {
        let backups = self.load_backup_index().await?;
        let file_path_str = file_path.to_string_lossy();

        let mut file_backups: Vec<_> = backups
            .into_iter()
            .filter(|b| b.original_path == file_path_str)
            .collect();

        file_backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(file_backups)
    }

    /// Get the most recent backup for a file.
    pub async fn get_latest_backup(&self, file_path: &Path) -> Result<Option<DotfileBackup>> {
        let backups = self.get_backups_for_file(file_path).await?;
        Ok(backups.into_iter().next())
    }

    /// List all backups.
    pub async fn list_all_backups(&self) -> Result<Vec<DotfileBackup>> {
        self.load_backup_index().await
    }

    /// Restore the most recent backup for a file.
    pub async fn restore_latest(&self, file_path: &Path) -> Result<()> {
        let backup = self
            .get_latest_backup(file_path)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No backup found for: {:?}", file_path))?;

        backup.restore().await
    }

    /// Verify integrity of all backups.
    pub async fn verify_all_backups(&self) -> Result<Vec<(DotfileBackup, bool)>> {
        let backups = self.load_backup_index().await?;
        let mut results = Vec::new();

        for backup in backups {
            let backup_path = Path::new(&backup.backup_path);
            let valid = if backup_path.exists() {
                match tokio::fs::read(backup_path).await {
                    Ok(content) => {
                        let hash = format!("{:x}", Sha256::digest(&content));
                        hash == backup.content_hash
                    }
                    Err(_) => false,
                }
            } else {
                false
            };
            results.push((backup, valid));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_backup_creation() {
        let dir = tempdir().unwrap();
        let backup_dir = dir.path().join("backups");
        let test_file = dir.path().join(".testrc");

        // Create test file
        tokio::fs::write(&test_file, "test content").await.unwrap();

        let manager = BackupManager::new(&backup_dir, 5).await.unwrap();
        let backup = manager
            .create_backup(&test_file, "test backup", "test-session")
            .await
            .unwrap();

        assert_eq!(backup.original_path, test_file.to_string_lossy());
        assert!(Path::new(&backup.backup_path).exists());
    }

    #[tokio::test]
    async fn test_backup_restore() {
        let dir = tempdir().unwrap();
        let backup_dir = dir.path().join("backups");
        let test_file = dir.path().join(".testrc");

        // Create test file with original content
        let original_content = "original content";
        tokio::fs::write(&test_file, original_content)
            .await
            .unwrap();

        let manager = BackupManager::new(&backup_dir, 5).await.unwrap();
        let backup = manager
            .create_backup(&test_file, "before modification", "test-session")
            .await
            .unwrap();

        // Modify the file
        tokio::fs::write(&test_file, "modified content")
            .await
            .unwrap();

        // Restore from backup
        backup.restore().await.unwrap();

        // Verify content is restored
        let restored = tokio::fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(restored, original_content);
    }

    #[tokio::test]
    async fn test_backup_cleanup() {
        let dir = tempdir().unwrap();
        let backup_dir = dir.path().join("backups");
        let test_file = dir.path().join(".testrc");

        tokio::fs::write(&test_file, "test").await.unwrap();

        let manager = BackupManager::new(&backup_dir, 2).await.unwrap();

        // Create 5 backups (should keep only 2)
        for i in 0..5 {
            tokio::fs::write(&test_file, format!("content {}", i))
                .await
                .unwrap();
            manager
                .create_backup(&test_file, format!("backup {}", i), "test-session")
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let backups = manager.get_backups_for_file(&test_file).await.unwrap();
        assert_eq!(backups.len(), 2);
    }
}
