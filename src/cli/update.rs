use crate::updater::{InstallOutcome, UpdateInfo, Updater};
use anyhow::{Context, Result};
use crossterm::style::Stylize;
use std::env;
use vtcode_config::update::{ReleaseChannel, UpdateConfig};
use vtcode_core::ui::user_confirmation::UserConfirmation;

/// Options for the update command
#[derive(Debug, Clone)]
pub struct UpdateCommandOptions {
    /// Check for updates without installing
    pub check_only: bool,
    /// Force reinstall latest version
    pub force: bool,
    /// List available versions
    pub list: bool,
    /// Number of versions to list
    pub limit: usize,
    /// Pin to a specific version
    pub pin: Option<String>,
    /// Unpin version
    pub unpin: bool,
    /// Set release channel
    pub channel: Option<String>,
    /// Show update configuration
    pub show_config: bool,
}

/// Handle the update command
pub async fn handle_update_command(options: UpdateCommandOptions) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    // Handle configuration operations first (don't need updater)
    if options.show_config {
        return handle_show_config();
    }

    if options.unpin {
        return handle_unpin();
    }

    if let Some(channel) = options.channel {
        return handle_set_channel(channel);
    }

    if options.list {
        return handle_list_versions(options.limit).await;
    }

    if let Some(version_str) = options.pin {
        return handle_pin_version(version_str);
    }

    println!(
        "{} VT Code v{}",
        "Checking for updates...".cyan(),
        current_version
    );

    let updater = Updater::new(current_version).context("Failed to initialize updater")?;

    // Show pin status
    if let Some(pinned) = updater.pinned_version() {
        println!(
            "{} Version pinned to {} (use --unpin to remove)",
            "ℹ".blue(),
            pinned.to_string().cyan()
        );
    }

    match updater.check_for_updates().await {
        Ok(Some(update_info)) => handle_update_available(&updater, &update_info, options).await,
        Ok(None) => {
            println!(
                "{} You're already on the latest version (v{})",
                "✓".green(),
                current_version
            );

            if options.force {
                println!("{} Force mode enabled, attempting reinstall...", "→".cyan());
                install_update(&updater, true).await?;
            }

            Ok(())
        }
        Err(err) => {
            eprintln!("{} Failed to check for updates: {:#}", "✗".red(), err);
            Err(err)
        }
    }
}

/// Show current update configuration
fn handle_show_config() -> Result<()> {
    let config = UpdateConfig::load().unwrap_or_default();

    println!("{}", "Update Configuration".bold());
    println!(
        "{} Config file: {}",
        "→".cyan(),
        UpdateConfig::config_path()?.display()
    );
    println!();
    println!(
        "{} Channel: {}",
        "→".cyan(),
        config.channel.to_string().green()
    );
    println!(
        "{} Pinned: {}",
        "→".cyan(),
        if let Some(pin) = config.pinned_version() {
            format!("{} ({}", pin.to_string().cyan(), pin)
        } else {
            "No".to_string()
        }
    );
    println!(
        "{} Check interval: {} hours",
        "→".cyan(),
        config.check_interval_hours
    );
    println!(
        "{} Download timeout: {} seconds",
        "→".cyan(),
        config.download_timeout_secs
    );
    println!(
        "{} Keep backup: {}",
        "→".cyan(),
        if config.keep_backup {
            "Yes".to_string().green()
        } else {
            "No".to_string().yellow()
        }
    );
    println!(
        "{} Auto-rollback: {}",
        "→".cyan(),
        if config.auto_rollback {
            "Yes".to_string().green()
        } else {
            "No".to_string().yellow()
        }
    );

    Ok(())
}

/// Unpin version
fn handle_unpin() -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let mut updater = Updater::new(current_version)?;

    if !updater.is_pinned() {
        println!("{} Version is not pinned", "ℹ".blue());
        return Ok(());
    }

    let pinned = updater.pinned_version().unwrap().clone();
    updater.unpin_version()?;

    println!("{} Unpinned from v{}", "✓".green(), pinned);
    println!(
        "{} You will now receive updates from the {} channel",
        "→".cyan(),
        updater.config().channel.to_string().green()
    );

    Ok(())
}

/// Set release channel
fn handle_set_channel(channel_str: String) -> Result<()> {
    let channel = match channel_str.to_lowercase().as_str() {
        "stable" => ReleaseChannel::Stable,
        "beta" => ReleaseChannel::Beta,
        "nightly" => ReleaseChannel::Nightly,
        _ => {
            anyhow::bail!(
                "Invalid channel '{}'. Must be one of: stable, beta, nightly",
                channel_str
            );
        }
    };

    let mut config = UpdateConfig::load().unwrap_or_default();
    config.channel = channel.clone();
    config.save()?;

    println!(
        "{} Release channel set to {}",
        "✓".green(),
        channel.to_string().green()
    );
    println!(
        "{} Future updates will follow the {} channel",
        "→".cyan(),
        channel.to_string().green()
    );

    Ok(())
}

/// List available versions
async fn handle_list_versions(limit: usize) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let updater = Updater::new(current_version)?;

    println!("{} Fetching available versions...", "→".cyan());

    let versions = updater.list_versions(limit).await?;

    println!();
    println!("{}", "Available Versions".bold());
    println!("{}", "─".repeat(60));

    for (i, version_info) in versions.iter().enumerate() {
        let is_current = version_info.version.to_string() == current_version;
        let marker = if is_current {
            "●".green()
        } else if version_info.is_prerelease {
            "○".yellow()
        } else {
            "○".white()
        };

        let prerelease_tag = if version_info.is_prerelease {
            " (pre-release)".yellow().to_string()
        } else {
            "".to_string()
        };

        println!(
            "{} {:<12} {}{}",
            marker,
            format!("v{}", version_info.version),
            version_info.tag.clone().dim(),
            prerelease_tag
        );

        if let Some(published) = &version_info.published_at {
            println!("   {}", format!("Published: {}", published).dim());
        }

        if i < versions.len() - 1 {
            println!();
        }
    }

    println!("{}", "─".repeat(60));
    println!(
        "{} Current version: v{}",
        "→".cyan(),
        current_version.green()
    );
    println!(
        "{} Use --pin VERSION to pin to a specific version",
        "→".cyan()
    );

    Ok(())
}

/// Pin to a specific version
fn handle_pin_version(version_str: String) -> Result<()> {
    let version = semver::Version::parse(&version_str)
        .with_context(|| format!("Invalid version format: {}", version_str))?;

    let current_version = env!("CARGO_PKG_VERSION");
    let mut updater = Updater::new(current_version)?;

    updater.pin_version(version.clone(), None, false)?;

    println!("{} Pinned to v{}", "✓".green(), version);
    println!("{} Auto-updates disabled until unpinned", "→".cyan());
    println!("{} Use --unpin to remove version pin", "→".cyan());

    Ok(())
}

/// Handle when an update is available
async fn handle_update_available(
    updater: &Updater,
    update: &UpdateInfo,
    options: UpdateCommandOptions,
) -> Result<()> {
    println!(
        "\n{} New version available: v{}",
        "●".cyan(),
        update.version
    );

    println!("\n{}", "Release notes:".bold());
    println!("{}", update.release_notes);

    if options.check_only {
        let guidance = updater.update_guidance();
        println!(
            "\n{} Update command: {}",
            "→".cyan(),
            guidance.command.as_str().green()
        );
        println!(
            "{} Release page: {}",
            "→".cyan(),
            Updater::release_url(&update.version).cyan()
        );
        return Ok(());
    }

    let proceed = UserConfirmation::confirm_action(
        &format!(
            "Install VT Code v{} now? This will replace the current binary.",
            update.version
        ),
        true,
    )
    .context("Failed to read confirmation input")?;

    if !proceed {
        println!("{} Update canceled.", "✗".yellow());
        println!(
            "{} You can run this again with `vtcode update`.",
            "→".cyan()
        );
        return Ok(());
    }

    install_update(updater, options.force).await
}

async fn install_update(updater: &Updater, force: bool) -> Result<()> {
    let guidance = updater.update_guidance();
    if guidance.source.is_managed() {
        println!(
            "{} Managed install detected ({}).",
            "!".yellow(),
            guidance.source.label()
        );
        println!(
            "{} Update with: {}",
            "→".cyan(),
            guidance.command.as_str().green()
        );
        return Ok(());
    }

    println!("\n{} Installing update...", "→".cyan());

    match updater.install_update(force).await? {
        InstallOutcome::Updated(version) => {
            println!("{} {} installed successfully!", "✓".green(), version.bold());
            println!("\n{} Restart VT Code to use the new version", "→".cyan());
        }
        InstallOutcome::UpToDate(version) => {
            println!("{} Already up to date ({})", "✓".green(), version.bold());
        }
    }

    Ok(())
}
