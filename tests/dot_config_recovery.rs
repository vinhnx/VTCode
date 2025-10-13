use std::fs;

use anyhow::Result;
use tempfile::TempDir;
use vtcode_core::utils::dot_config::{
    ConfigLoadSource, ConfigRecoveryStrategy, DotError, DotManager,
};

#[test]
fn corrupted_config_without_quarantine_preserves_original_file() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let manager = DotManager::with_root_dir(temp_dir.path().join(".product"))?;

    manager.initialize()?;
    manager.update_config(|cfg| {
        cfg.preferences.default_model = "stable".to_string();
    })?;
    let backup_path = manager.create_backup()?;

    fs::write(manager.config_file_path(), "invalid = true")?;

    let outcome = manager.load_or_recover(ConfigRecoveryStrategy {
        quarantine_corrupted: false,
        ..ConfigRecoveryStrategy::default()
    })?;

    assert!(matches!(
        &outcome.source,
        ConfigLoadSource::BackupRestored(restored) if restored == &backup_path
    ));
    assert!(outcome.quarantined_config.is_none());
    assert!(manager.config_file_path().exists());

    Ok(())
}

#[test]
fn recovery_without_fallback_errors_when_no_backup_available() {
    let temp_dir = TempDir::new().unwrap();
    let manager = DotManager::with_root_dir(temp_dir.path().join(".product")).unwrap();

    manager.initialize().unwrap();
    fs::write(manager.config_file_path(), "[broken").unwrap();

    let error = manager
        .load_or_recover(ConfigRecoveryStrategy {
            prefer_backups: true,
            fallback_to_default: false,
            max_backup_attempts: 2,
            quarantine_corrupted: true,
        })
        .unwrap_err();

    assert!(matches!(error, DotError::RecoveryFailed(_)));
}
