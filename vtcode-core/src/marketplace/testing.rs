//! Testing module for VT Code marketplace system
//!
//! This module provides tests and examples for using the marketplace system with actual sources.

use std::path::PathBuf;

use anyhow::Result;

use crate::marketplace::{
    MarketplaceConfig, MarketplaceSource, MarketplaceSystem, PluginManifest,
    config::{InstalledPlugin, MarketplaceSettings},
};

/// Test the marketplace system with sample configurations
pub async fn test_marketplace_system() -> Result<()> {
    println!("Testing VT Code marketplace system...");

    // Create a temporary directory for testing
    let temp_dir = tempfile::tempdir()?;
    let base_dir = temp_dir.path().to_path_buf();

    // Create a plugin runtime (using a dummy one for testing)
    let plugin_runtime = None; // In real usage, this would be a proper PluginRuntime

    // Create marketplace system
    let config = MarketplaceConfig::default();
    let marketplace_system = MarketplaceSystem::new(base_dir.clone(), config, plugin_runtime);

    // Initialize the system
    marketplace_system.initialize().await?;
    println!("✓ Marketplace system initialized");

    // Test adding a marketplace
    let test_marketplace = MarketplaceSource::Git {
        id: "test-marketplace".to_string(),
        url: "https://github.com/test/test-marketplace".to_string(),
        refspec: Some("main".to_string()),
    };

    marketplace_system
        .registry
        .add_marketplace(test_marketplace)
        .await?;
    println!("✓ Test marketplace added");

    // Test creating and installing a sample plugin
    let sample_plugin = PluginManifest {
        id: "test-plugin".to_string(),
        name: "Test Plugin".to_string(),
        version: "1.0.0".to_string(),
        description: "A test plugin for marketplace system".to_string(),
        entrypoint: PathBuf::from("bin/test-plugin"),
        capabilities: vec!["test".to_string()],
        source: "https://github.com/test/test-plugin".to_string(),
        trust_level: Some(crate::config::PluginTrustLevel::Sandbox),
        dependencies: vec![],
        author: "Test Author".to_string(),
        license: "MIT".to_string(),
        homepage: "https://github.com/test/test-plugin".to_string(),
        repository: "https://github.com/test/test-plugin".to_string(),
    };

    // Install the plugin
    marketplace_system
        .installer
        .install_plugin(&sample_plugin)
        .await?;
    println!("✓ Sample plugin installed");

    // Verify the plugin is installed
    let is_installed = marketplace_system
        .installer
        .is_installed("test-plugin")
        .await;
    assert!(is_installed, "Plugin should be installed");
    println!("✓ Plugin installation verified");

    // Test uninstalling the plugin
    marketplace_system
        .installer
        .uninstall_plugin("test-plugin")
        .await?;
    println!("✓ Sample plugin uninstalled");

    // Verify the plugin is uninstalled
    let is_installed_after = marketplace_system
        .installer
        .is_installed("test-plugin")
        .await;
    assert!(!is_installed_after, "Plugin should be uninstalled");
    println!("✓ Plugin uninstallation verified");

    println!("All marketplace system tests passed!");
    Ok(())
}

/// Test marketplace configuration system
pub async fn test_marketplace_config() -> Result<()> {
    println!("Testing marketplace configuration system...");

    // Create a temporary directory for testing
    let temp_dir = tempfile::tempdir()?;
    let config_path = temp_dir.path().join("marketplace-config-test.toml");

    // Create initial settings
    let mut settings = MarketplaceSettings::default();
    settings.auto_update.marketplaces = true;
    settings.security.default_trust_level = crate::config::PluginTrustLevel::Trusted;

    // Add a test marketplace
    let marketplace = MarketplaceSource::GitHub {
        id: "test-gh".to_string(),
        owner: "test".to_string(),
        repo: "test-repo".to_string(),
        refspec: Some("main".to_string()),
    };
    settings.add_marketplace(marketplace);

    // Add a test plugin
    let plugin = InstalledPlugin {
        id: "config-test-plugin".to_string(),
        name: "Config Test Plugin".to_string(),
        version: "1.0.0".to_string(),
        source: "test-marketplace".to_string(),
        install_path: PathBuf::from("/tmp/test"),
        enabled: true,
        trust_level: crate::config::PluginTrustLevel::Sandbox,
        installed_at: "2023-01-01".to_string(),
    };
    settings.add_installed_plugin(plugin);

    // Save settings to file
    settings.save_to_file(&config_path).await?;
    println!("✓ Marketplace settings saved to file");

    // Load settings from file
    let loaded_settings = MarketplaceSettings::load_from_file(&config_path).await?;
    println!("✓ Marketplace settings loaded from file");

    // Verify loaded settings
    assert_eq!(
        settings.auto_update.marketplaces,
        loaded_settings.auto_update.marketplaces
    );
    assert_eq!(
        settings.security.default_trust_level,
        loaded_settings.security.default_trust_level
    );
    assert_eq!(
        settings.marketplaces.len(),
        loaded_settings.marketplaces.len()
    );
    assert_eq!(
        settings.installed_plugins.len(),
        loaded_settings.installed_plugins.len()
    );

    println!("✓ Configuration system tests passed!");
    Ok(())
}

/// Test plugin validation functionality
pub fn test_plugin_validation() -> Result<()> {
    println!("Testing plugin validation...");

    // Create a valid plugin manifest
    let valid_plugin = PluginManifest {
        id: "valid-plugin".to_string(),
        name: "Valid Plugin".to_string(),
        version: "1.0.0".to_string(),
        description: "A valid test plugin".to_string(),
        entrypoint: PathBuf::from("bin/valid-plugin"),
        capabilities: vec!["test".to_string()],
        source: "https://github.com/test/valid-plugin".to_string(),
        trust_level: Some(crate::config::PluginTrustLevel::Sandbox),
        dependencies: vec![],
        author: "Test Author".to_string(),
        license: "MIT".to_string(),
        homepage: "https://github.com/test/valid-plugin".to_string(),
        repository: "https://github.com/test/valid-plugin".to_string(),
    };

    // Create an invalid plugin manifest (missing required fields)
    let invalid_plugin = PluginManifest {
        id: "".to_string(),   // Invalid: empty ID
        name: "".to_string(), // Invalid: empty name
        version: "1.0.0".to_string(),
        description: "An invalid test plugin".to_string(),
        entrypoint: PathBuf::from(""),
        capabilities: vec![],
        source: "".to_string(), // Invalid: empty source
        trust_level: None,
        dependencies: vec![],
        author: "Test Author".to_string(),
        license: "MIT".to_string(),
        homepage: "https://github.com/test/invalid-plugin".to_string(),
        repository: "https://github.com/test/invalid-plugin".to_string(),
    };

    // Create a plugin installer to test validation
    let temp_dir = tempfile::tempdir()?;
    let installer = super::installer::PluginInstaller::new(temp_dir.path().to_path_buf(), None);

    // Test validation of valid plugin (should pass)
    let valid_result = installer.validate_manifest(&valid_plugin);
    assert!(valid_result.is_ok(), "Valid plugin should pass validation");
    println!("✓ Valid plugin validation passed");

    // Test validation of invalid plugin (should fail)
    let invalid_result = installer.validate_manifest(&invalid_plugin);
    assert!(
        invalid_result.is_err(),
        "Invalid plugin should fail validation"
    );
    println!("✓ Invalid plugin validation failed as expected");

    println!("✓ Plugin validation tests passed!");
    Ok(())
}

/// Run all marketplace tests
pub async fn run_all_tests() -> Result<()> {
    println!("Running VT Code marketplace system tests...\n");

    test_plugin_validation()?;
    println!();

    test_marketplace_config().await?;
    println!();

    test_marketplace_system().await?;
    println!();

    println!("All tests completed successfully!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_full_marketplace_workflow() {
        // This test runs the full marketplace workflow
        let result = run_all_tests().await;
        assert!(result.is_ok(), "Marketplace tests should pass");
    }
}
