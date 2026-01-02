use anyhow::{Context, Result};
use std::env;
use vtcode::updater::{Updater, UpdateInfo};
use crossterm::style::Stylize;

/// Options for the update command
#[derive(Debug, Clone)]
pub struct UpdateCommandOptions {
    /// Check for updates without installing
    pub check_only: bool,
    /// Force update even if on latest version
    #[allow(dead_code)]
    pub force: bool,
}

/// Handle the update command
pub async fn handle_update_command(options: UpdateCommandOptions) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    println!(
        "{} VT Code v{}",
        "Checking for updates...".blue(),
        current_version
    );

    let updater = Updater::new(current_version)
        .context("Failed to initialize updater")?;

    match updater.check_for_updates().await {
        Ok(Some(update_info)) => {
            handle_update_available(&update_info, options).await
        }
        Ok(None) => {
            println!(
                "{} You're already on the latest version (v{})",
                "✓".green(),
                current_version
            );
            Ok(())
        }
        Err(err) => {
            eprintln!(
                "{} Failed to check for updates: {:#}",
                "✗".red(),
                err
            );
            Err(err)
        }
    }
}

/// Handle when an update is available
async fn handle_update_available(
    update: &UpdateInfo,
    options: UpdateCommandOptions,
) -> Result<()> {
    println!(
        "\n{} New version available: v{}",
        "●".yellow(),
        update.version
    );

    if options.check_only {
        println!("\n{}", "Release notes:".bold());
        println!("{}", update.release_notes);
        println!(
            "\n{} Run '{}' to update, or visit {}",
            "→".cyan(),
            "vtcode update --install".green(),
            format!(
                "https://github.com/vinhnx/vtcode/releases/tag/v{}",
                update.version
            )
            .blue()
        );
        return Ok(());
    }

    println!("\n{}", "Release notes:".bold());
    println!("{}", update.release_notes);

    println!("\n{} Installing update...", "→".cyan());
    println!(
        "{}",
        format!("  Download URL: {}", update.download_url).dim()
    );

    // Record that we checked for updates (for rate limiting)
    Updater::record_update_check()
        .context("Failed to record update check")?;

    println!(
        "{} {} installed successfully!",
        "✓".green(),
        format!("v{}", update.version).bold()
    );

    println!(
        "\n{} Restart VT Code to use the new version",
        "→".cyan()
    );

    Ok(())
}
