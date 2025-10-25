//! CLI commands for self-update functionality

use crate::update::{UpdateChannel, UpdateConfig, UpdateFrequency, UpdateManager};
use anyhow::{Context, Result};
use clap::Subcommand;

#[derive(Debug, Clone, Subcommand)]
pub enum UpdateCommands {
    /// Check for available updates
    Check {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Install available updates
    Install {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,

        /// Force reinstall even if no update is available
        #[arg(short, long)]
        force: bool,
    },

    /// Configure update settings
    Config {
        /// Enable or disable automatic updates
        #[arg(long)]
        enabled: Option<bool>,

        /// Set update channel (stable, beta, nightly)
        #[arg(long)]
        channel: Option<String>,

        /// Set update frequency (always, daily, weekly, never)
        #[arg(long)]
        frequency: Option<String>,

        /// Enable or disable automatic downloads
        #[arg(long)]
        auto_download: Option<bool>,

        /// Enable or disable automatic installation
        #[arg(long)]
        auto_install: Option<bool>,
    },

    /// List available backups
    Backups,

    /// Rollback to a previous version
    Rollback {
        /// Backup file to rollback to
        backup: Option<String>,
    },

    /// Clean up old backups
    Cleanup,
}

/// Handle update-related commands
pub async fn handle_update_command(command: UpdateCommands) -> Result<()> {
    match command {
        UpdateCommands::Check { verbose } => handle_check_command(verbose).await,
        UpdateCommands::Install { yes, force } => handle_install_command(yes, force).await,
        UpdateCommands::Config {
            enabled,
            channel,
            frequency,
            auto_download,
            auto_install,
        } => handle_config_command(enabled, channel, frequency, auto_download, auto_install).await,
        UpdateCommands::Backups => handle_backups_command().await,
        UpdateCommands::Rollback { backup } => handle_rollback_command(backup).await,
        UpdateCommands::Cleanup => handle_cleanup_command().await,
    }
}

/// Check for available updates
async fn handle_check_command(verbose: bool) -> Result<()> {
    let config = UpdateConfig::from_env()?;
    let manager = UpdateManager::new(config)?;

    println!("Checking for updates...");

    let status = manager
        .check_for_updates()
        .await
        .context("Failed to check for updates")?;

    println!("\nCurrent version: {}", status.current_version);

    if let Some(latest) = &status.latest_version {
        println!("Latest version:  {}", latest);
    }

    if status.update_available {
        println!("\nAn update is available!");

        if verbose {
            if let Some(notes) = &status.release_notes {
                println!("\nRelease notes:");
                println!("{}", notes);
            }

            if let Some(url) = &status.download_url {
                println!("\nDownload URL: {}", url);
            }
        }

        println!("\nRun 'vtcode update install' to install the update.");
    } else {
        println!("\nYou are running the latest version.");
    }

    Ok(())
}

/// Install available updates
async fn handle_install_command(yes: bool, force: bool) -> Result<()> {
    let config = UpdateConfig::from_env()?;
    let mut manager = UpdateManager::new(config)?;

    // Check for updates first
    let status = manager
        .check_for_updates()
        .await
        .context("Failed to check for updates")?;

    if !status.update_available && !force {
        println!("No update available. You are running the latest version.");
        return Ok(());
    }

    if !yes {
        println!(
            "This will update vtcode from {} to {}.",
            status.current_version,
            status.latest_version.as_deref().unwrap_or("unknown")
        );
        println!("A backup will be created before updating.");
        print!("Continue? [y/N] ");

        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Update cancelled.");
            return Ok(());
        }
    }

    println!("Downloading and installing update...");

    let result = manager
        .perform_update()
        .await
        .context("Failed to perform update")?;

    if result.success {
        println!("\nUpdate installed successfully!");
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
        anyhow::bail!("Update installation failed");
    }

    Ok(())
}

/// Configure update settings
async fn handle_config_command(
    enabled: Option<bool>,
    channel: Option<String>,
    frequency: Option<String>,
    auto_download: Option<bool>,
    auto_install: Option<bool>,
) -> Result<()> {
    let mut config = UpdateConfig::from_env()?;

    let mut changed = false;

    if let Some(val) = enabled {
        config.enabled = val;
        changed = true;
        println!(
            "Automatic updates: {}",
            if val { "enabled" } else { "disabled" }
        );
    }

    if let Some(val) = channel {
        config.channel = match val.to_lowercase().as_str() {
            "stable" => UpdateChannel::Stable,
            "beta" => UpdateChannel::Beta,
            "nightly" => UpdateChannel::Nightly,
            _ => anyhow::bail!(
                "Invalid channel: {}. Use 'stable', 'beta', or 'nightly'.",
                val
            ),
        };
        changed = true;
        println!("Update channel: {}", config.channel);
    }

    if let Some(val) = frequency {
        config.frequency = match val.to_lowercase().as_str() {
            "always" => UpdateFrequency::Always,
            "daily" => UpdateFrequency::Daily,
            "weekly" => UpdateFrequency::Weekly,
            "never" => UpdateFrequency::Never,
            _ => anyhow::bail!(
                "Invalid frequency: {}. Use 'always', 'daily', 'weekly', or 'never'.",
                val
            ),
        };
        changed = true;
        println!("Update frequency: {:?}", config.frequency);
    }

    if let Some(val) = auto_download {
        config.auto_download = val;
        changed = true;
        println!(
            "Automatic downloads: {}",
            if val { "enabled" } else { "disabled" }
        );
    }

    if let Some(val) = auto_install {
        config.auto_install = val;
        changed = true;
        println!(
            "Automatic installation: {}",
            if val { "enabled" } else { "disabled" }
        );
    }

    if !changed {
        println!("Current update configuration:");
        println!("  Enabled: {}", config.enabled);
        println!("  Channel: {}", config.channel);
        println!("  Frequency: {:?}", config.frequency);
        println!("  Auto-download: {}", config.auto_download);
        println!("  Auto-install: {}", config.auto_install);
    }

    Ok(())
}

/// List available backups
async fn handle_backups_command() -> Result<()> {
    let config = UpdateConfig::from_env()?;
    let manager = UpdateManager::new(config)?;

    let backups = manager
        .config()
        .backup_dir
        .read_dir()
        .context("Failed to read backup directory")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("vtcode_backup_")
        })
        .collect::<Vec<_>>();

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

/// Rollback to a previous version
async fn handle_rollback_command(backup: Option<String>) -> Result<()> {
    let config = UpdateConfig::from_env()?;
    let manager = UpdateManager::new(config)?;

    let backup_path = if let Some(path) = backup {
        std::path::PathBuf::from(path)
    } else {
        // Find the most recent backup
        let backups = manager
            .config()
            .backup_dir
            .read_dir()
            .context("Failed to read backup directory")?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("vtcode_backup_")
            })
            .max_by_key(|entry| {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            });

        if let Some(backup) = backups {
            backup.path()
        } else {
            anyhow::bail!("No backups found");
        }
    };

    println!("Rolling back to: {:?}", backup_path);

    manager
        .rollback_to_backup(&backup_path)
        .context("Failed to rollback")?;

    println!("Rollback completed successfully!");
    println!("Please restart vtcode to use the restored version.");

    Ok(())
}

/// Clean up old backups
async fn handle_cleanup_command() -> Result<()> {
    let config = UpdateConfig::from_env()?;
    let manager = UpdateManager::new(config)?;

    println!("Cleaning up old backups...");

    manager
        .cleanup_old_backups()
        .context("Failed to cleanup backups")?;

    println!("Cleanup completed successfully!");

    Ok(())
}
