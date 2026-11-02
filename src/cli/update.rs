use crate::updater::{
    DesktopInstallOutcome, InstallOutcome, UpdateExecutionStrategy, UpdateInfo, Updater, default_install_dir,
    desktop_macos_major, desktop_supported, existing_install_dir, install_desktop_app, installed_desktop_version,
    parse_highlights,
};
use anyhow::{Context, Result, anyhow};
use crossterm::style::Stylize;
use semver::Version;
use std::env;
use std::io::IsTerminal;
use std::process::Command;
use vtcode_config::update::{ReleaseChannel, UpdateConfig};
use vtcode_core::ui::user_confirmation::UserConfirmation;

/// How the update command handles the macOS desktop app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DesktopChoice {
    /// Ask when interactive; print a hint and skip otherwise.
    #[default]
    Prompt,
    /// Install or update without prompting.
    Install,
    /// Skip without prompting.
    Skip,
}

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
    /// How to handle the macOS desktop app
    pub desktop: DesktopChoice,
}

/// Whether `--with-desktop` was passed with a mode that never installs (the flag
/// would otherwise be silently ignored).
fn desktop_install_flag_unused(options: &UpdateCommandOptions) -> bool {
    options.desktop == DesktopChoice::Install
        && (options.check_only
            || options.list
            || options.pin.is_some()
            || options.unpin
            || options.channel.is_some()
            || options.show_config)
}

/// Handle the update command
pub async fn handle_update_command(options: UpdateCommandOptions) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    if desktop_install_flag_unused(&options) {
        println!("{} --with-desktop only applies when installing; it has no effect with this mode.", "!".yellow());
    }

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

    println!("{} VT Code v{}", "Checking for updates...".cyan(), current_version);

    let mut updater = Updater::new(current_version).context("Failed to initialize updater")?;

    // Show pin status
    if let Some(pinned) = updater.pinned_version() {
        println!("{} Version pinned to {} (use --unpin to remove)", "ℹ".blue(), pinned.to_string().cyan());
    }

    match updater.check_for_updates().await {
        Ok(Some(update_info)) => handle_update_available(&updater, &update_info, options).await,
        Ok(None) => {
            println!("{} You're already on the latest version (v{})", "✓".green(), current_version);
            println!("{} vtcode init   # scaffold project if needed", "→".cyan());

            if options.force {
                println!("{} Force mode enabled, attempting reinstall...", "→".cyan());
                install_update(&updater, None, true, options.desktop).await?;
            } else if !options.check_only {
                maybe_offer_desktop_install(&updater, Some(updater.current_version().clone()), options.desktop).await?;
            }

            Ok(())
        }
        Err(err) => {
            tracing::error!("Failed to check for updates: {:#}", err);
            Err(err)
        }
    }
}

/// Show current update configuration
fn handle_show_config() -> Result<()> {
    let config = UpdateConfig::load().unwrap_or_default();

    println!("{}", "Update Configuration".bold());
    println!("{} Config file: {}", "→".cyan(), UpdateConfig::config_path()?.display());
    println!();
    println!("{} Channel: {}", "→".cyan(), config.channel.to_string().green());
    println!(
        "{} Pinned: {}",
        "→".cyan(),
        if let Some(pin) = config.pinned_version() {
            let reason = config
                .pin
                .as_ref()
                .and_then(|p| p.reason.as_deref())
                .unwrap_or("no reason specified");
            format!("{} ({})", pin.to_string().cyan(), reason)
        } else {
            "No".to_string()
        }
    );
    println!("{} Check interval: {} hours", "→".cyan(), config.check_interval_hours);
    println!("{} Download timeout: {} seconds", "→".cyan(), config.download_timeout_secs);

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

    let pinned = updater
        .pinned_version()
        .ok_or_else(|| {
            anyhow!("is_pinned() returned true but pinned_version() returned None — updater state is inconsistent")
        })?
        .clone();
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
            anyhow::bail!("Invalid channel '{channel_str}'. Must be one of: stable, beta, nightly");
        }
    };

    let mut config = UpdateConfig::load().unwrap_or_default();
    config.channel = channel.clone();
    config.save()?;

    println!("{} Release channel set to {}", "✓".green(), channel.to_string().green());
    println!("{} Future updates will follow the {} channel", "→".cyan(), channel.to_string().green());

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
        let is_current = version_info.version == *updater.current_version();
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
            println!("   {}", format!("Published: {published}").dim());
        }

        if i < versions.len() - 1 {
            println!();
        }
    }

    println!("{}", "─".repeat(60));
    println!("{} Current version: v{}", "→".cyan(), current_version.green());
    println!("{} Use --pin VERSION to pin to a specific version", "→".cyan());

    Ok(())
}

/// Pin to a specific version
fn handle_pin_version(version_str: String) -> Result<()> {
    let version = Version::parse(&version_str).with_context(|| format!("Invalid version format: {version_str}"))?;

    let current_version = env!("CARGO_PKG_VERSION");
    let mut updater = Updater::new(current_version)?;

    updater.pin_version(version.clone(), None, false)?;

    println!("{} Pinned to v{}", "✓".green(), version);
    println!("{} Auto-updates disabled until unpinned", "→".cyan());
    println!("{} Use --unpin to remove version pin", "→".cyan());

    Ok(())
}

/// Handle when an update is available
async fn handle_update_available(updater: &Updater, update: &UpdateInfo, options: UpdateCommandOptions) -> Result<()> {
    println!("\n{} New version available: v{}", "●".cyan(), update.version);

    println!("\n{}", "Release notes:".bold());
    println!("{}", update.release_notes);

    if options.check_only {
        let guidance = updater.update_guidance();
        println!("\n{} Update command: {}", "→".cyan(), guidance.command().green());
        println!("{} Release page: {}", "→".cyan(), Updater::release_url(&update.version).cyan());
        return Ok(());
    }

    let proceed = UserConfirmation::confirm_action(
        &format!("Install VT Code v{} now? This will replace the current binary.", update.version),
        true,
    )
    .context("Failed to read confirmation input")?;

    if !proceed {
        println!("{} Update canceled.", "✗".yellow());
        println!("{} You can run this again with `vtcode update`.", "→".cyan());
        return Ok(());
    }

    install_update(updater, Some(update), options.force, options.desktop).await
}

fn run_update_command(command: &str, strategy: UpdateExecutionStrategy) -> Result<std::process::ExitStatus> {
    let mut cmd = match strategy {
        UpdateExecutionStrategy::Shell => {
            #[cfg(target_os = "windows")]
            {
                let mut c = Command::new("cmd");
                c.arg("/C").arg(command);
                c
            }
            #[cfg(not(target_os = "windows"))]
            {
                let mut c = Command::new("/bin/sh");
                c.arg("-lc").arg(command);
                c
            }
        }
        UpdateExecutionStrategy::PowerShell => {
            let mut c = if cfg!(target_os = "windows") {
                Command::new("powershell")
            } else {
                Command::new("pwsh")
            };
            c.arg("-NoLogo").arg("-NoProfile").arg("-Command").arg(command);
            c
        }
    };

    cmd.status().with_context(|| format!("failed to run update command: {command}"))
}

async fn print_installed_release_notes(version: &Version, release_notes: &str) {
    let parsed = parse_highlights(version, release_notes);
    println!("\n{}", format!("What's new in v{version}:").bold());
    if parsed.items.is_empty() {
        println!("{}", release_notes.trim());
    } else {
        for item in &parsed.items {
            println!(" • {item}");
        }
    }
    println!("{} {}", "→".cyan(), Updater::release_url(version).cyan());
}

async fn install_update(
    updater: &Updater,
    update_info: Option<&UpdateInfo>,
    force: bool,
    desktop: DesktopChoice,
) -> Result<()> {
    let guidance = updater.update_guidance();
    if guidance.source.is_managed() {
        println!("{} Managed install detected ({}).", "!".yellow(), guidance.source.label());
        println!("{} Running: {}", "→".cyan(), guidance.command().green());

        // `run_update_command` shells out and blocks until the package
        // manager exits; run it off the async executor so it cannot stall
        // Tokio workers. See `# Blocking` docs in `src/agent/runloop/git.rs`.
        let display_command = guidance.action.display_command.to_string();
        let execution = guidance.action.execution;
        let status = tokio::task::spawn_blocking(move || run_update_command(&display_command, execution))
            .await
            .context("update command task panicked")?
            .with_context(|| format!("failed to run update command for {}", guidance.source.label()))?;

        if status.success() {
            println!("{} Update command completed successfully!", "✓".green());
            // The managed installer replaced the binary outside of
            // `install_update_reported`, so seed the cache here. Otherwise the
            // next `vtcode` launch reads a stale `latest_version` and only
            // shows release notes on its second start (N+1).
            if let Some(info) = update_info {
                crate::updater::seed_release_notes_for_installed_version(&info.version, &info.release_notes);
                print_installed_release_notes(&info.version, &info.release_notes).await;
            }
            println!("\n{} Restart VT Code to use the new version", "→".cyan());
            println!("{} vtcode init   # scaffold project if needed", "→".cyan());
            println!("{} vtcode        # launch TUI", "→".cyan());
            maybe_offer_desktop_install(updater, update_info.map(|info| info.version.clone()), desktop).await?;
        } else {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
            println!("{} Update command exited with status: {}", "!".yellow(), code);
            println!("{} You may need to run the command manually.", "→".cyan());
        }

        return Ok(());
    }

    println!("\n{} Installing update...", "→".cyan());

    let desktop_target = match updater.install_update(force).await? {
        InstallOutcome::Updated(version) => {
            let target = Version::parse(&version).ok();
            println!("{} {} installed successfully!", "✓".green(), version.bold());
            // `install_update_reported` already seeded the cache with the fresh
            // release metadata so the TUI shows notes on its first launch.
            // Echo the highlights here as well so they are visible right after
            // `vtcode update` succeeds, without waiting for a restart.
            if let Some(info) = update_info {
                print_installed_release_notes(&info.version, &info.release_notes).await;
            }
            println!("\n{} Restart VT Code to use the new version", "→".cyan());
            println!("{} vtcode init   # scaffold project if needed", "→".cyan());
            println!("{} vtcode        # launch TUI", "→".cyan());
            target
        }
        InstallOutcome::UpToDate(version) => {
            let target = Version::parse(&version).ok();
            println!("{} Already up to date ({})", "✓".green(), version.bold());
            target
        }
    };
    maybe_offer_desktop_install(updater, desktop_target, desktop).await?;

    Ok(())
}

/// Offer the macOS desktop app install/update after the CLI step.
///
/// `target` is the CLI version just installed (or already current); the offer
/// is skipped silently when the installed desktop app already matches it, so
/// up-to-date users are never nagged. A `None` target skips the pre-check and
/// installs the latest release's asset.
async fn maybe_offer_desktop_install(updater: &Updater, target: Option<Version>, desktop: DesktopChoice) -> Result<()> {
    if !desktop_supported() {
        if desktop == DesktopChoice::Install {
            anyhow::bail!("--with-desktop requires macOS; the desktop app is not available on this platform.");
        }
        return Ok(());
    }
    if desktop == DesktopChoice::Skip {
        return Ok(());
    }
    // A pinned CLI means "leave my versions alone": skip the prompt, while an
    // explicit --with-desktop still expresses intent and proceeds.
    if updater.is_pinned() && desktop == DesktopChoice::Prompt {
        return Ok(());
    }
    // The bundle cannot launch below macOS 15: fail an explicit request,
    // warn-and-skip a prompted one (mirrors the installer's best-effort).
    let macos_major = tokio::task::spawn_blocking(desktop_macos_major)
        .await
        .context("desktop OS check panicked")?;
    if !macos_major.is_some_and(|major| major >= 15) {
        if desktop == DesktopChoice::Install {
            anyhow::bail!("VT Code Desktop requires macOS 15 or newer.");
        }
        println!("{} Desktop app requires macOS 15 or newer; skipping.", "!".yellow());
        return Ok(());
    }

    if let Some(target) = target.as_ref() {
        let installed = installed_desktop_version().await;
        if let Some(current) = installed.as_ref().filter(|version| **version >= *target) {
            println!("{} Desktop app is already up to date (v{current})", "✓".green());
            return Ok(());
        }
    }

    let install = match desktop {
        DesktopChoice::Install => true,
        DesktopChoice::Skip => return Ok(()),
        DesktopChoice::Prompt => {
            if !std::io::stdin().is_terminal() {
                println!("{} Run `vtcode update --with-desktop` to also install the macOS desktop app.", "→".cyan());
                return Ok(());
            }
            let message = match target.as_ref() {
                Some(version) => format!("Install the VT Code desktop app (v{version})?"),
                None => "Install the VT Code desktop app?".to_string(),
            };
            UserConfirmation::confirm_action(&message, true).context("Failed to read confirmation input")?
        }
    };
    if !install {
        println!("{} Desktop install skipped.", "→".cyan());
        return Ok(());
    }

    // Updates reuse the existing install's directory; fresh installs ask
    // unless --with-desktop already declined prompting.
    let install_dir = match existing_install_dir() {
        Some(dir) => dir,
        None if desktop == DesktopChoice::Prompt => {
            let all_users =
                UserConfirmation::confirm_action("Install for all users (/Applications, may need sudo)?", false)
                    .context("Failed to read confirmation input")?;
            if all_users {
                std::path::PathBuf::from("/Applications")
            } else {
                default_install_dir()?
            }
        }
        None => default_install_dir()?,
    };

    // Prompt mode is best-effort (like the installer default): the CLI update
    // already succeeded, so a desktop failure warns instead of failing the
    // command. Explicit --with-desktop fails hard.
    let outcome = match install_desktop_app(updater, &install_dir).await {
        Ok(outcome) => outcome,
        Err(err) if desktop == DesktopChoice::Prompt => {
            println!("{} Desktop install failed: {err:#}", "!".yellow());
            println!("{} Re-run with --with-desktop to retry, or --without-desktop to skip.", "→".cyan());
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    match outcome {
        DesktopInstallOutcome::Installed(version) => {
            println!("{} Desktop app v{} installed successfully!", "✓".green(), version.bold());
            println!("{} Open it anytime with: vtcode desktop", "→".cyan());
        }
        DesktopInstallOutcome::UpToDate(version) => {
            println!("{} Desktop app is already up to date (v{})", "✓".green(), version);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(desktop: DesktopChoice) -> UpdateCommandOptions {
        UpdateCommandOptions {
            check_only: false,
            force: false,
            list: false,
            limit: 10,
            pin: None,
            unpin: false,
            channel: None,
            show_config: false,
            desktop,
        }
    }

    #[test]
    fn unused_flag_warning_covers_every_non_install_mode() {
        // --with-desktop with a real update: no warning.
        assert!(!desktop_install_flag_unused(&options(DesktopChoice::Install)));
        // Every mode that never installs warns.
        let mut check = options(DesktopChoice::Install);
        check.check_only = true;
        assert!(desktop_install_flag_unused(&check));
        let mut list = options(DesktopChoice::Install);
        list.list = true;
        assert!(desktop_install_flag_unused(&list));
        let mut pin = options(DesktopChoice::Install);
        pin.pin = Some("0.1.0".to_string());
        assert!(desktop_install_flag_unused(&pin));
        let mut unpin = options(DesktopChoice::Install);
        unpin.unpin = true;
        assert!(desktop_install_flag_unused(&unpin));
        let mut channel = options(DesktopChoice::Install);
        channel.channel = Some("beta".to_string());
        assert!(desktop_install_flag_unused(&channel));
        let mut show = options(DesktopChoice::Install);
        show.show_config = true;
        assert!(desktop_install_flag_unused(&show));
        // Prompt/Skip never warn: the hint only concerns an explicit request.
        let mut prompt_check = options(DesktopChoice::Prompt);
        prompt_check.check_only = true;
        assert!(!desktop_install_flag_unused(&prompt_check));
        let mut skip_check = options(DesktopChoice::Skip);
        skip_check.check_only = true;
        assert!(!desktop_install_flag_unused(&skip_check));
    }

    #[tokio::test]
    async fn skip_choice_short_circuits_before_any_prompt_or_gate() {
        let updater = Updater::new("0.0.0-test").expect("updater");
        maybe_offer_desktop_install(&updater, None, DesktopChoice::Skip)
            .await
            .expect("skip is a silent no-op");
    }

    /// Explicit `--with-desktop` fails hard off macOS instead of silently
    /// skipping (the contract that makes the flag trustworthy in scripts).
    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn explicit_install_bails_off_macos() {
        let updater = Updater::new("0.0.0-test").expect("updater");
        let error = maybe_offer_desktop_install(&updater, None, DesktopChoice::Install)
            .await
            .expect_err("must fail, not skip");
        assert!(error.to_string().contains("--with-desktop requires macOS"), "{error:#}");
    }
}
