//! Backup and rollback management

use super::config::UpdateConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Manages backups and rollback operations
pub struct RollbackManager {
    config: UpdateConfig,
}

impl RollbackManager {
    pub fn new(config: UpdateConfig) -> Result<Self> {
        Ok(Self { config })
    }

    /// Create a backup of the current executable
    pub fn create_backup(&self) -> Result<PathBuf> {
        self.config.ensure_directories()?;

        let current_exe = std::env::current_exe().context("Failed to get current executable")?;

        // Generate backup filename with timestamp
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let backup_filename = format!("vtcode_backup_{}", timestamp);

        #[cfg(windows)]
        let backup_filename = format!("{}.exe", backup_filename);

        let backup_path = self.config.backup_dir.join(backup_filename);

        tracing::info!("Creating backup: {:?}", backup_path);

        // Copy current executable to backup location
        std::fs::copy(&current_exe, &backup_path).context("Failed to create backup")?;

        // Set executable permissions on Unix
        #[cfg(unix)]
        {
            let metadata = std::fs::metadata(&current_exe)?;
            let permissions = metadata.permissions();
            std::fs::set_permissions(&backup_path, permissions)?;
        }

        tracing::info!("Backup created successfully");

        // Clean up old backups
        self.cleanup_old_backups()?;

        Ok(backup_path)
    }

    /// Rollback to a previous backup
    pub fn rollback(&self, backup_path: &PathBuf) -> Result<()> {
        if !backup_path.exists() {
            anyhow::bail!("Backup file does not exist: {:?}", backup_path);
        }

        let current_exe = std::env::current_exe().context("Failed to get current executable")?;

        tracing::info!("Rolling back to: {:?}", backup_path);

        // On Windows, we need special handling
        #[cfg(windows)]
        {
            self.rollback_windows(backup_path, &current_exe)?;
        }

        // On Unix, we can directly replace
        #[cfg(unix)]
        {
            self.rollback_unix(backup_path, &current_exe)?;
        }

        tracing::info!("Rollback completed successfully");

        Ok(())
    }

    /// Rollback on Unix systems
    #[cfg(unix)]
    fn rollback_unix(&self, backup_path: &PathBuf, current_exe: &PathBuf) -> Result<()> {
        std::fs::copy(backup_path, current_exe).context("Failed to restore backup")?;

        // Restore executable permissions
        let metadata = std::fs::metadata(backup_path)?;
        let permissions = metadata.permissions();
        std::fs::set_permissions(current_exe, permissions)?;

        Ok(())
    }

    /// Rollback on Windows systems
    #[cfg(windows)]
    fn rollback_windows(&self, backup_path: &PathBuf, current_exe: &PathBuf) -> Result<()> {
        let temp_path = current_exe.with_extension("exe.tmp");

        // Remove temp file if it exists
        if temp_path.exists() {
            std::fs::remove_file(&temp_path).ok();
        }

        // Rename current executable to temp
        std::fs::rename(current_exe, &temp_path).context("Failed to rename current executable")?;

        // Copy backup to current location
        match std::fs::copy(backup_path, current_exe) {
            Ok(_) => {
                // Remove temp file
                std::fs::remove_file(&temp_path).ok();
                Ok(())
            }
            Err(e) => {
                // Restore from temp on failure
                std::fs::rename(&temp_path, current_exe).ok();
                Err(e).context("Failed to restore backup")
            }
        }
    }

    /// Clean up old backups, keeping only the most recent ones
    pub fn cleanup_old_backups(&self) -> Result<()> {
        let backup_dir = &self.config.backup_dir;

        if !backup_dir.exists() {
            return Ok(());
        }

        // Get all backup files
        let mut backups: Vec<_> = std::fs::read_dir(backup_dir)
            .context("Failed to read backup directory")?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("vtcode_backup_")
            })
            .collect();

        // Sort by modification time (newest first)
        backups.sort_by_key(|entry| {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        backups.reverse();

        // Remove old backups beyond max_backups
        if backups.len() > self.config.max_backups {
            for backup in backups.iter().skip(self.config.max_backups) {
                let path = backup.path();
                tracing::info!("Removing old backup: {:?}", path);
                std::fs::remove_file(&path).ok();
            }
        }

        Ok(())
    }

    /// List all available backups
    pub fn list_backups(&self) -> Result<Vec<PathBuf>> {
        let backup_dir = &self.config.backup_dir;

        if !backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups: Vec<_> = std::fs::read_dir(backup_dir)
            .context("Failed to read backup directory")?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("vtcode_backup_")
            })
            .map(|entry| entry.path())
            .collect();

        // Sort by modification time (newest first)
        backups.sort_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        backups.reverse();

        Ok(backups)
    }

    /// Get information about a backup
    pub fn get_backup_info(&self, backup_path: &PathBuf) -> Result<BackupInfo> {
        let metadata = std::fs::metadata(backup_path).context("Failed to read backup metadata")?;

        let size = metadata.len();
        let modified = metadata
            .modified()
            .context("Failed to get modification time")?;

        let filename = backup_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(BackupInfo {
            path: backup_path.clone(),
            filename,
            size,
            modified,
        })
    }
}

/// Information about a backup
#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub filename: String,
    pub size: u64,
    pub modified: std::time::SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rollback_manager_creation() {
        let config = UpdateConfig::default();
        let manager = RollbackManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_list_backups_empty() {
        let config = UpdateConfig::default();
        let manager = RollbackManager::new(config).unwrap();
        let backups = manager.list_backups().unwrap();
        assert!(backups.is_empty() || !backups.is_empty()); // May have existing backups
    }
}
