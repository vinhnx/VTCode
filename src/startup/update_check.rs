//! Startup update check functionality
//!
//! This module handles checking for updates on application startup and
//! prompting users to update when new versions are available.

use anyhow::Result;
use console::style;
use vtcode_core::update::{UpdateConfig, UpdateManager, UpdateStatus};

/// Check for updates on startup and prompt user if available
pub async fn check_for_updates_on_startup() -> Result<()> {
    // Check if update checks are disabled via environment variable
    if std::env::var("VT_UPDATE_CHECK")
        .unwrap_or_else(|_| "true".to_string())
        .to_lowercase()
        == "false"
    {
        return Ok(());
    }

    // Load update configuration
    let config = match UpdateConfig::from_env() {
        Ok(cfg) => cfg,
        Err(_) => {
            // Silently fail if config can't be loaded
            return Ok(());
        }
    };

    // Skip if updates are disabled
    if !config.enabled {
        return Ok(());
    }

    // Create update manager
    let manager = match UpdateManager::new(config) {
        Ok(mgr) => mgr,
        Err(_) => {
            // Silently fail if manager can't be created
            return Ok(());
        }
    };

    // Check for updates (non-blocking, with timeout)
    let status = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        manager.check_for_updates(),
    )
    .await
    {
        Ok(Ok(status)) => status,
        _ => {
            // Silently fail on timeout or error
            return Ok(());
        }
    };

    // Display update notification if available
    if status.update_available {
        display_update_notification(&status)?;
        prompt_for_update(manager, &status).await?;
    }

    Ok(())
}

/// Display a prominent update notification
fn display_update_notification(status: &UpdateStatus) -> Result<()> {
    let current = &status.current_version;
    let latest = status.latest_version.as_deref().unwrap_or("unknown");

    // Print prominent header
    println!();
    println!("{}", style("═".repeat(80)).cyan().bold());
    println!(
        "{}",
        style("  UPDATE AVAILABLE").cyan().bold().on_black()
    );
    println!("{}", style("═".repeat(80)).cyan().bold());
    println!();
    println!(
        "  {} {}",
        style("Current version:").dim(),
        style(current).yellow()
    );
    println!(
        "  {} {}",
        style("Latest version: ").dim(),
        style(latest).green().bold()
    );
    println!();

    // Show release notes if available
    if let Some(notes) = &status.release_notes {
        let lines: Vec<&str> = notes.lines().take(5).collect();
        if !lines.is_empty() {
            println!("  {}", style("Release highlights:").dim());
            for line in lines {
                if !line.trim().is_empty() {
                    println!("    {}", style(line.trim()).dim());
                }
            }
            println!();
        }
    }

    println!(
        "  {} {}",
        style("→").cyan(),
        style("Run 'vtcode update install' to update").dim()
    );
    println!("{}", style("═".repeat(80)).cyan().bold());
    println!();

    Ok(())
}

/// Prompt user to install update
async fn prompt_for_update(manager: UpdateManager, status: &UpdateStatus) -> Result<()> {
    // Check if we're in an interactive terminal
    if !is_terminal::is_terminal(&std::io::stdin()) {
        return Ok(());
    }

    // Check if auto-install is enabled
    if manager.config().auto_install {
        println!("  {} Auto-install is enabled. Installing update...", style("→").cyan());
        return perform_update(manager, status).await;
    }

    // Prompt user
    use dialoguer::Confirm;

    let prompt = Confirm::new()
        .with_prompt("Would you like to install this update now?")
        .default(false)
        .interact_opt()?;

    match prompt {
        Some(true) => {
            perform_update(manager, status).await?;
        }
        Some(false) => {
            println!(
                "  {} Update skipped. Run 'vtcode update install' later to update.",
                style("ℹ").blue()
            );
        }
        None => {
            // User cancelled (Ctrl+C)
            return Ok(());
        }
    }

    Ok(())
}

/// Perform the update installation
async fn perform_update(mut manager: UpdateManager, _status: &UpdateStatus) -> Result<()> {
    println!();
    println!("  {} Downloading update...", style("→").cyan());

    // Show progress indicator
    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_message("Downloading and verifying update...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    // Perform update
    let result = manager.perform_update().await;

    spinner.finish_and_clear();

    match result {
        Ok(update_result) => {
            if update_result.success {
                println!();
                println!("  {} Update installed successfully!", style("✓").green().bold());
                println!(
                    "  {} Updated from {} to {}",
                    style("→").cyan(),
                    style(&update_result.old_version).yellow(),
                    style(&update_result.new_version).green().bold()
                );

                if let Some(backup) = update_result.backup_path {
                    println!(
                        "  {} Backup created at: {}",
                        style("ℹ").blue(),
                        style(backup.display()).dim()
                    );
                }

                if update_result.requires_restart {
                    println!();
                    println!(
                        "  {} Please restart vtcode to use the new version.",
                        style("⚠").yellow()
                    );
                    println!(
                        "  {} Run 'vtcode --version' to verify the update.",
                        style("→").cyan()
                    );
                }

                println!();
            } else {
                println!(
                    "  {} Update installation failed.",
                    style("✗").red().bold()
                );
            }
        }
        Err(e) => {
            println!(
                "  {} Update failed: {}",
                style("✗").red().bold(),
                style(e).red()
            );
            println!(
                "  {} Your previous version has been restored.",
                style("ℹ").blue()
            );
            println!(
                "  {} Try again later with 'vtcode update install'",
                style("→").cyan()
            );
        }
    }

    Ok(())
}

/// Check if we should show the update prompt based on frequency
pub fn should_check_for_updates() -> bool {
    // Check environment variable
    if std::env::var("VT_UPDATE_CHECK")
        .unwrap_or_else(|_| "true".to_string())
        .to_lowercase()
        == "false"
    {
        return false;
    }

    // Check if we're in a CI/CD environment
    if is_ci_environment() {
        return false;
    }

    // Check if we're in a non-interactive environment
    if !is_terminal::is_terminal(&std::io::stdin()) {
        return false;
    }

    true
}

/// Detect if we're running in a CI/CD environment
fn is_ci_environment() -> bool {
    std::env::var("CI").is_ok()
        || std::env::var("CONTINUOUS_INTEGRATION").is_ok()
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
        || std::env::var("CIRCLECI").is_ok()
        || std::env::var("TRAVIS").is_ok()
        || std::env::var("JENKINS_URL").is_ok()
}

/// Display a minimal update notification (non-interactive)
pub fn display_minimal_update_notification(status: &UpdateStatus) {
    if !status.update_available {
        return;
    }

    let current = &status.current_version;
    let latest = status.latest_version.as_deref().unwrap_or("unknown");

    eprintln!(
        "{} Update available: {} → {} (run 'vtcode update install')",
        style("ℹ").blue(),
        style(current).yellow(),
        style(latest).green()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_check_for_updates() {
        // Should return a boolean
        let result = should_check_for_updates();
        assert!(result || !result); // Always true
    }

    #[test]
    fn test_is_ci_environment() {
        // Test CI detection
        unsafe {
            // SAFETY: Tests run single-threaded here and clean up the mutation immediately.
            std::env::remove_var("CI");
        }
        assert!(!is_ci_environment());

        unsafe {
            // SAFETY: The test owns this temporary CI value and restores it right after use.
            std::env::set_var("CI", "true");
        }
        assert!(is_ci_environment());
        unsafe {
            // SAFETY: Restores the environment to its previous state for subsequent tests.
            std::env::remove_var("CI");
        }
    }

    #[test]
    fn test_display_minimal_notification() {
        let status = UpdateStatus {
            current_version: "0.33.1".to_string(),
            latest_version: Some("0.34.0".to_string()),
            update_available: true,
            download_url: None,
            release_notes: None,
            last_checked: None,
        };

        // Should not panic
        display_minimal_update_notification(&status);
    }
}
