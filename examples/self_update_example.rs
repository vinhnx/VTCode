//! Example demonstrating the self-update functionality
//!
//! This example shows how to:
//! - Check for available updates
//! - Download and install updates
//! - Configure update settings
//! - Manage backups and rollbacks
//!
//! Run with: cargo run --example self_update_example

use anyhow::Result;
use vtcode_core::update::{UpdateChannel, UpdateConfig, UpdateFrequency, UpdateManager};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt().with_env_filter("info").init();

    println!("VTCode Self-Update Example\n");

    // Example 1: Check for updates with default configuration
    println!("=== Example 1: Check for Updates ===");
    check_for_updates().await?;

    // Example 2: Configure update settings
    println!("\n=== Example 2: Configure Update Settings ===");
    configure_updates().await?;

    // Example 3: List available backups
    println!("\n=== Example 3: List Backups ===");
    list_backups().await?;

    // Example 4: Custom update configuration
    println!("\n=== Example 4: Custom Configuration ===");
    custom_configuration().await?;

    Ok(())
}

/// Example 1: Check for available updates
async fn check_for_updates() -> Result<()> {
    // Create update manager with default configuration
    let config = UpdateConfig::from_env()?;
    let manager = UpdateManager::new(config)?;

    println!("Checking for updates...");

    // Check for updates
    match manager.check_for_updates().await {
        Ok(status) => {
            println!("Current version: {}", status.current_version);

            if let Some(latest) = &status.latest_version {
                println!("Latest version:  {}", latest);
            }

            if status.update_available {
                println!("\nAn update is available!");

                if let Some(notes) = &status.release_notes {
                    println!("\nRelease notes:");
                    println!("{}", notes);
                }

                if let Some(url) = &status.download_url {
                    println!("\nDownload URL: {}", url);
                }
            } else {
                println!("\nYou are running the latest version.");
            }
        }
        Err(e) => {
            eprintln!("Failed to check for updates: {}", e);
        }
    }

    Ok(())
}

/// Example 2: Configure update settings
async fn configure_updates() -> Result<()> {
    let mut config = UpdateConfig::from_env()?;

    println!("Current configuration:");
    println!("  Enabled: {}", config.enabled);
    println!("  Channel: {}", config.channel);
    println!("  Frequency: {:?}", config.frequency);
    println!("  Auto-download: {}", config.auto_download);
    println!("  Auto-install: {}", config.auto_install);

    // Modify configuration
    config.channel = UpdateChannel::Beta;
    config.frequency = UpdateFrequency::Weekly;
    config.auto_download = true;

    println!("\nUpdated configuration:");
    println!("  Channel: {}", config.channel);
    println!("  Frequency: {:?}", config.frequency);
    println!("  Auto-download: {}", config.auto_download);

    Ok(())
}

/// Example 3: List available backups
async fn list_backups() -> Result<()> {
    let config = UpdateConfig::from_env()?;

    if !config.backup_dir.exists() {
        println!("No backup directory found.");
        return Ok(());
    }

    let backups: Vec<_> = std::fs::read_dir(&config.backup_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("vtcode_backup_")
        })
        .collect();

    if backups.is_empty() {
        println!("No backups found.");
        return Ok(());
    }

    println!("Available backups:");
    for backup in backups {
        let path = backup.path();
        let metadata = std::fs::metadata(&path)?;
        let size = metadata.len();
        let modified = metadata.modified()?;

        println!(
            "  {} ({} bytes, modified: {:?})",
            path.display(),
            size,
            modified
        );
    }

    Ok(())
}

/// Example 4: Custom update configuration
async fn custom_configuration() -> Result<()> {
    // Create a custom configuration
    let mut config = UpdateConfig::default();

    // Configure for beta channel with daily checks
    config.enabled = true;
    config.channel = UpdateChannel::Beta;
    config.frequency = UpdateFrequency::Daily;
    config.auto_download = true;
    config.auto_install = false; // Require manual installation
    config.max_backups = 5; // Keep more backups

    println!("Custom configuration:");
    println!("  Enabled: {}", config.enabled);
    println!("  Channel: {}", config.channel);
    println!("  Frequency: {:?}", config.frequency);
    println!("  Auto-download: {}", config.auto_download);
    println!("  Auto-install: {}", config.auto_install);
    println!("  Max backups: {}", config.max_backups);

    // Create manager with custom configuration
    let manager = UpdateManager::new(config)?;

    println!("\nUpdate manager created with custom configuration.");
    println!("Update directory: {:?}", manager.config().update_dir);
    println!("Backup directory: {:?}", manager.config().backup_dir);

    Ok(())
}

/// Example 5: Perform a complete update workflow (commented out for safety)
#[allow(dead_code)]
async fn complete_update_workflow() -> Result<()> {
    let config = UpdateConfig::from_env()?;
    let mut manager = UpdateManager::new(config)?;

    // Step 1: Check for updates
    println!("Step 1: Checking for updates...");
    let status = manager.check_for_updates().await?;

    if !status.update_available {
        println!("No update available.");
        return Ok(());
    }

    println!(
        "Update available: {}",
        status.latest_version.as_ref().unwrap()
    );

    // Step 2: Perform update
    println!("\nStep 2: Downloading and installing update...");
    let result = manager.perform_update().await?;

    if result.success {
        println!("\nUpdate successful!");
        println!(
            "Updated from {} to {}",
            result.old_version, result.new_version
        );

        if let Some(backup) = result.backup_path {
            println!("Backup created at: {:?}", backup);
        }

        if result.requires_restart {
            println!("\nPlease restart vtcode to use the new version.");
        }
    } else {
        println!("\nUpdate failed.");
    }

    Ok(())
}

/// Example 6: Rollback to a previous version (commented out for safety)
#[allow(dead_code)]
async fn rollback_example() -> Result<()> {
    let config = UpdateConfig::from_env()?;
    let manager = UpdateManager::new(config)?;

    // Find the most recent backup
    let backups: Vec<_> = std::fs::read_dir(&manager.config().backup_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("vtcode_backup_")
        })
        .collect();

    if backups.is_empty() {
        println!("No backups available for rollback.");
        return Ok(());
    }

    // Get the most recent backup
    let most_recent = backups
        .iter()
        .max_by_key(|entry| {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
        .unwrap();

    let backup_path = most_recent.path();
    println!("Rolling back to: {:?}", backup_path);

    // Perform rollback
    manager.rollback_to_backup(&backup_path)?;

    println!("Rollback completed successfully!");

    Ok(())
}
