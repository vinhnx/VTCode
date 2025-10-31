//! Integration tests for the self-update functionality

use anyhow::Result;
use vtcode_core::update::{UpdateChannel, UpdateConfig, UpdateFrequency, UpdateManager};

fn set_env_var(key: &str, value: &str) {
    // SAFETY: tests run in isolation and only touch process-local env vars.
    unsafe {
        std::env::set_var(key, value);
    }
}

fn remove_env_var(key: &str) {
    // SAFETY: reverting env changes made in this test scope.
    unsafe {
        std::env::remove_var(key);
    }
}

#[test]
fn test_update_config_default() {
    let config = UpdateConfig::default();

    assert!(!config.enabled);
    assert_eq!(config.channel, UpdateChannel::Stable);
    assert_eq!(config.frequency, UpdateFrequency::Daily);
    assert!(!config.auto_download);
    assert!(!config.auto_install);
    assert_eq!(config.max_backups, 3);
    assert!(config.verify_signatures);
    assert!(config.verify_checksums);
}

#[test]
fn test_update_config_from_env() {
    // Set environment variables
    unsafe {
        // SAFETY: The test controls these variables and resets them before finishing.
        std::env::set_var("VTCODE_UPDATE_ENABLED", "false");
        std::env::set_var("VTCODE_UPDATE_CHANNEL", "beta");
        std::env::set_var("VTCODE_UPDATE_FREQUENCY", "weekly");
        std::env::set_var("VTCODE_UPDATE_AUTO_DOWNLOAD", "true");
        std::env::set_var("VTCODE_UPDATE_MAX_BACKUPS", "5");
    }

    let config = UpdateConfig::from_env().unwrap();

    assert!(!config.enabled);
    assert_eq!(config.channel, UpdateChannel::Beta);
    assert_eq!(config.frequency, UpdateFrequency::Weekly);
    assert!(config.auto_download);
    assert_eq!(config.max_backups, 5);

    // Clean up
    unsafe {
        // SAFETY: Restores the environment to the state it had before the test mutated it.
        std::env::remove_var("VTCODE_UPDATE_ENABLED");
        std::env::remove_var("VTCODE_UPDATE_CHANNEL");
        std::env::remove_var("VTCODE_UPDATE_FREQUENCY");
        std::env::remove_var("VTCODE_UPDATE_AUTO_DOWNLOAD");
        std::env::remove_var("VTCODE_UPDATE_MAX_BACKUPS");
    }
}

#[test]
fn test_update_channel_display() {
    assert_eq!(UpdateChannel::Stable.to_string(), "stable");
    assert_eq!(UpdateChannel::Beta.to_string(), "beta");
    assert_eq!(UpdateChannel::Nightly.to_string(), "nightly");
}

#[test]
fn test_update_manager_creation() {
    let config = UpdateConfig::default();
    let manager = UpdateManager::new(config);
    assert!(manager.is_ok());
}

#[tokio::test]
async fn test_update_checker_version_parsing() {
    use vtcode_core::update::UpdateChecker;

    let config = UpdateConfig::default();
    let checker = UpdateChecker::new(config).unwrap();

    // Test version parsing through reflection (if methods are public)
    // This is a simplified test - in practice you'd need to expose the parse_version method
    // or test it indirectly through the public API
}

#[test]
fn test_update_config_ensure_directories() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut config = UpdateConfig::default();
    config.update_dir = temp_dir.path().join("updates");
    config.backup_dir = temp_dir.path().join("backups");

    assert!(config.ensure_directories().is_ok());
    assert!(config.update_dir.exists());
    assert!(config.backup_dir.exists());
}

#[test]
fn test_update_frequency_variants() {
    let frequencies = vec![
        UpdateFrequency::Always,
        UpdateFrequency::Daily,
        UpdateFrequency::Weekly,
        UpdateFrequency::Never,
    ];

    for freq in frequencies {
        // Just ensure they can be created and compared
        assert_eq!(freq, freq);
    }
}

#[test]
fn test_update_channel_variants() {
    let channels = vec![
        UpdateChannel::Stable,
        UpdateChannel::Beta,
        UpdateChannel::Nightly,
    ];

    for channel in channels {
        // Just ensure they can be created and compared
        assert_eq!(channel, channel);
    }
}

#[test]
fn test_github_api_base_url() {
    let config = UpdateConfig::default();
    assert_eq!(config.github_api_base(), "https://api.github.com");

    let mut config_custom = UpdateConfig::default();
    config_custom.github_api_base = Some("https://github.company.com/api/v3".to_string());
    assert_eq!(
        config_custom.github_api_base(),
        "https://github.company.com/api/v3"
    );
}

#[tokio::test]
async fn test_rollback_manager_creation() {
    use vtcode_core::update::RollbackManager;

    let config = UpdateConfig::default();
    let manager = RollbackManager::new(config);
    assert!(manager.is_ok());
}

#[tokio::test]
async fn test_rollback_manager_list_backups() {
    use vtcode_core::update::RollbackManager;

    let temp_dir = tempfile::tempdir().unwrap();
    let mut config = UpdateConfig::default();
    config.backup_dir = temp_dir.path().join("backups");
    config.ensure_directories().unwrap();

    let manager = RollbackManager::new(config).unwrap();
    let backups = manager.list_backups().unwrap();

    // Should be empty initially
    assert!(backups.is_empty());
}

#[test]
fn test_update_config_serialization() {
    let config = UpdateConfig::default();

    // Test that config can be serialized
    let json = serde_json::to_string(&config);
    assert!(json.is_ok());
}

#[test]
fn test_update_status_serialization() {
    use vtcode_core::update::UpdateStatus;

    let status = UpdateStatus {
        current_version: "0.33.1".to_string(),
        latest_version: Some("0.34.0".to_string()),
        update_available: true,
        download_url: Some("https://example.com/download".to_string()),
        release_notes: Some("New features".to_string()),
        last_checked: Some(chrono::Utc::now()),
    };

    // Test serialization
    let json = serde_json::to_string(&status);
    assert!(json.is_ok());

    // Test deserialization
    let json_str = json.unwrap();
    let deserialized: Result<UpdateStatus, _> = serde_json::from_str(&json_str);
    assert!(deserialized.is_ok());
}

#[tokio::test]
async fn test_update_verifier_creation() {
    use vtcode_core::update::UpdateVerifier;

    let config = UpdateConfig::default();
    let verifier = UpdateVerifier::new(config);
    assert!(verifier.is_ok());
}

#[tokio::test]
async fn test_update_downloader_creation() {
    use vtcode_core::update::UpdateDownloader;

    let config = UpdateConfig::default();
    let downloader = UpdateDownloader::new(config);
    assert!(downloader.is_ok());
}

#[tokio::test]
async fn test_update_installer_creation() {
    use vtcode_core::update::UpdateInstaller;

    let config = UpdateConfig::default();
    let installer = UpdateInstaller::new(config);
    assert!(installer.is_ok());
}

#[test]
fn test_current_version_constant() {
    use vtcode_core::update::CURRENT_VERSION;

    assert!(!CURRENT_VERSION.is_empty());
    assert!(CURRENT_VERSION.contains('.'));

    // Should be a valid semver
    let parts: Vec<&str> = CURRENT_VERSION.split('.').collect();
    assert!(parts.len() >= 3);
}

#[test]
fn test_github_repo_constants() {
    use vtcode_core::update::{GITHUB_REPO_NAME, GITHUB_REPO_OWNER};

    assert_eq!(GITHUB_REPO_OWNER, "vinhnx");
    assert_eq!(GITHUB_REPO_NAME, "vtcode");
}

#[test]
fn test_update_config_modification() {
    let mut config = UpdateConfig::default();

    // Test modifying configuration
    config.enabled = false;
    config.channel = UpdateChannel::Beta;
    config.frequency = UpdateFrequency::Weekly;
    config.auto_download = true;
    config.auto_install = true;
    config.max_backups = 10;

    assert!(!config.enabled);
    assert_eq!(config.channel, UpdateChannel::Beta);
    assert_eq!(config.frequency, UpdateFrequency::Weekly);
    assert!(config.auto_download);
    assert!(config.auto_install);
    assert_eq!(config.max_backups, 10);
}

#[tokio::test]
async fn test_update_manager_config_access() {
    let config = UpdateConfig::default();
    let manager = UpdateManager::new(config.clone()).unwrap();

    // Test that we can access the configuration
    let manager_config = manager.config();
    assert_eq!(manager_config.enabled, config.enabled);
    assert_eq!(manager_config.channel, config.channel);
}

#[tokio::test]
async fn test_update_manager_config_update() {
    let config = UpdateConfig::default();
    let mut manager = UpdateManager::new(config).unwrap();

    // Create a new configuration
    let mut new_config = UpdateConfig::default();
    new_config.channel = UpdateChannel::Beta;
    new_config.frequency = UpdateFrequency::Weekly;

    // Update the manager's configuration
    let result = manager.set_config(new_config.clone());
    assert!(result.is_ok());

    // Verify the configuration was updated
    assert_eq!(manager.config().channel, UpdateChannel::Beta);
    assert_eq!(manager.config().frequency, UpdateFrequency::Weekly);
}
