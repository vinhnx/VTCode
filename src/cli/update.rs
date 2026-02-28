use anyhow::{Context, Result};
use crossterm::style::Stylize;
use std::env;
use vtcode::updater::{InstallOutcome, UpdateInfo, Updater};
use vtcode_core::ui::user_confirmation::UserConfirmation;

/// Options for the update command
#[derive(Debug, Clone)]
pub struct UpdateCommandOptions {
    /// Check for updates without installing
    pub check_only: bool,
    /// Force reinstall latest version
    pub force: bool,
}

/// Handle the update command
pub async fn handle_update_command(options: UpdateCommandOptions) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    println!(
        "{} VT Code v{}",
        "Checking for updates...".cyan(),
        current_version
    );

    let updater = Updater::new(current_version).context("Failed to initialize updater")?;

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
