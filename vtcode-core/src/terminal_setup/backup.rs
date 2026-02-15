//! Configuration backup and restore system.
//!
//! Provides timestamped backups with retention policies to safely modify terminal configs.

use crate::utils::file_utils::ensure_dir_exists_sync;
use anyhow::{Context, Result};
use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

use super::detector::TerminalType;

/// Maximum number of backups to retain per config file
const MAX_BACKUPS: usize = 5;

/// Manages configuration file backups for terminal setup
pub struct ConfigBackupManager {
    #[allow(dead_code)]
    terminal_type: TerminalType,
}

impl ConfigBackupManager {
    /// Create a new backup manager for the given terminal type
    pub fn new(terminal_type: TerminalType) -> Self {
        Self { terminal_type }
    }

    /// Create a timestamped backup of the config file
    ///
    /// Returns the path to the created backup file
    pub fn backup_config(&self, config_path: &Path) -> Result<PathBuf> {
        // Check if config file exists
        if !config_path.exists() {
            anyhow::bail!("Config file does not exist: {}", config_path.display());
        }

        // Generate backup path with timestamp
        let backup_path = self.generate_backup_path(config_path)?;

        // Create parent directory if needed
        if let Some(parent) = backup_path.parent() {
            ensure_dir_exists_sync(parent).with_context(|| {
                format!("Failed to create backup directory: {}", parent.display())
            })?;
        }

        // Copy config to backup
        fs::copy(config_path, &backup_path).with_context(|| {
            format!(
                "Failed to backup config from {} to {}",
                config_path.display(),
                backup_path.display()
            )
        })?;

        // Cleanup old backups
        self.cleanup_old_backups(config_path)?;

        Ok(backup_path)
    }

    /// Restore a config file from a backup
    pub fn restore_backup(&self, original_path: &Path, backup_path: &Path) -> Result<()> {
        if !backup_path.exists() {
            anyhow::bail!("Backup file does not exist: {}", backup_path.display());
        }

        // Create parent directory if needed
        if let Some(parent) = original_path.parent() {
            ensure_dir_exists_sync(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        fs::copy(backup_path, original_path).with_context(|| {
            format!(
                "Failed to restore backup from {} to {}",
                backup_path.display(),
                original_path.display()
            )
        })?;

        Ok(())
    }

    /// List all available backups for a config file
    pub fn list_backups(&self, config_path: &Path) -> Result<Vec<PathBuf>> {
        let config_name = config_path
            .file_name()
            .context("Invalid config path")?
            .to_string_lossy();

        let config_dir = config_path
            .parent()
            .context("Config file has no parent directory")?;

        if !config_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();

        for entry in fs::read_dir(config_dir)
            .with_context(|| format!("Failed to read directory: {}", config_dir.display()))?
        {
            let entry = entry.with_context(|| "Failed to read directory entry")?;
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Match pattern: config_name.vtcode_backup_YYYYMMDD_HHMMSS
            if file_name_str.starts_with(&*config_name) && file_name_str.contains(".vtcode_backup_")
            {
                backups.push(entry.path());
            }
        }

        // Sort by modification time (newest first)
        backups.sort_by(|a, b| {
            let a_meta = fs::metadata(a).ok();
            let b_meta = fs::metadata(b).ok();

            match (a_meta, b_meta) {
                (Some(a_m), Some(b_m)) => b_m
                    .modified()
                    .unwrap_or_else(|_| std::time::SystemTime::now())
                    .cmp(
                        &a_m.modified()
                            .unwrap_or_else(|_| std::time::SystemTime::now()),
                    ),
                _ => std::cmp::Ordering::Equal,
            }
        });

        Ok(backups)
    }

    /// Clean up old backups, keeping only the most recent MAX_BACKUPS
    pub fn cleanup_old_backups(&self, config_path: &Path) -> Result<()> {
        let backups = self.list_backups(config_path)?;

        // Remove backups beyond MAX_BACKUPS
        for backup in backups.iter().skip(MAX_BACKUPS) {
            fs::remove_file(backup)
                .with_context(|| format!("Failed to remove old backup: {}", backup.display()))?;
        }

        Ok(())
    }

    /// Generate a timestamped backup path
    fn generate_backup_path(&self, config_path: &Path) -> Result<PathBuf> {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");

        let config_name = config_path
            .file_name()
            .context("Invalid config path")?
            .to_string_lossy();

        let backup_name = format!("{}.vtcode_backup_{}", config_name, timestamp);

        let backup_path = config_path
            .parent()
            .context("Config file has no parent directory")?
            .join(backup_name);

        Ok(backup_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_backup_and_restore() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test.conf");

        // Create original config
        fs::write(&config_path, "original content").unwrap();

        // Create backup
        let manager = ConfigBackupManager::new(TerminalType::Kitty);
        let backup_path = manager.backup_config(&config_path).unwrap();

        assert!(backup_path.exists());
        assert_eq!(
            fs::read_to_string(&backup_path).unwrap(),
            "original content"
        );

        // Modify original
        fs::write(&config_path, "modified content").unwrap();

        // Restore from backup
        manager.restore_backup(&config_path, &backup_path).unwrap();

        assert_eq!(
            fs::read_to_string(&config_path).unwrap(),
            "original content"
        );
    }

    #[test]
    fn test_list_backups() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test.conf");

        fs::write(&config_path, "content").unwrap();

        let manager = ConfigBackupManager::new(TerminalType::Kitty);

        // Create multiple backups
        let backup1 = manager.backup_config(&config_path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let backup2 = manager.backup_config(&config_path).unwrap();

        let backups = manager.list_backups(&config_path).unwrap();

        assert_eq!(backups.len(), 2);
        // Newest should be first
        assert_eq!(backups[0], backup2);
        assert_eq!(backups[1], backup1);
    }

    #[test]
    fn test_cleanup_old_backups() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test.conf");

        fs::write(&config_path, "content").unwrap();

        let manager = ConfigBackupManager::new(TerminalType::Kitty);

        // Create more than MAX_BACKUPS
        for _ in 0..7 {
            manager.backup_config(&config_path).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let backups = manager.list_backups(&config_path).unwrap();

        // Should only have MAX_BACKUPS (5) remaining
        assert_eq!(backups.len(), MAX_BACKUPS);
    }

    #[test]
    fn test_backup_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nonexistent.conf");

        let manager = ConfigBackupManager::new(TerminalType::Kitty);
        let result = manager.backup_config(&config_path);

        assert!(result.is_err());
    }
}
