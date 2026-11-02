use std::path::{Path, PathBuf};

use crate::updater::{DesktopInstallOutcome, Updater, default_install_dir, existing_install_dir, install_desktop_app};
use anyhow::{Context, Result, bail};
use crossterm::style::Stylize;

/// Environment override for the VT Code.app location.
const DESKTOP_APP_ENV: &str = "VTCODE_DESKTOP_APP";

/// Resolve the desktop bundle to open: an explicit path wins, then the
/// environment override, then the standard install locations. Always returns
/// an absolute path: `open` resolves relative bundle paths unreliably and can
/// launch a different same-named install instead.
pub(crate) fn resolve_desktop_app(app_path: Option<PathBuf>) -> Result<PathBuf> {
    let bundle = if let Some(path) = app_path {
        validated_bundle(&path).with_context(|| format!("not a macOS app bundle: {}", path.display()))?
    } else if let Some(from_env) = std::env::var_os(DESKTOP_APP_ENV) {
        let path = PathBuf::from(from_env);
        validated_bundle(&path)
            .with_context(|| format!("{DESKTOP_APP_ENV} is not a macOS app bundle: {}", path.display()))?
    } else {
        scan_locations(&standard_locations())?
    };
    vtcode_commons::paths::canonicalize(&bundle).with_context(|| format!("failed to resolve {}", bundle.display()))
}

/// First valid bundle wins; otherwise report every recovery path.
fn scan_locations(roots: &[PathBuf]) -> Result<PathBuf> {
    for candidate in roots {
        if validated_bundle(candidate).is_ok() {
            return Ok(candidate.clone());
        }
    }
    bail!(
        "VT Code.app not found; install it with `vtcode desktop --install`, pass its path (`vtcode desktop <path>`), set {DESKTOP_APP_ENV}, or build it with scripts/package-desktop-macos.sh"
    )
}

fn standard_locations() -> Vec<PathBuf> {
    let mut locations = Vec::with_capacity(2);
    let user_applications = vtcode_commons::paths::expand_tilde("~/Applications/VT Code.app");
    if user_applications != Path::new("~/Applications/VT Code.app") {
        locations.push(user_applications);
    }
    locations.push(PathBuf::from("/Applications/VT Code.app"));
    locations
}

/// A bundle only counts when it has the executable layout the launcher needs.
fn validated_bundle(path: &Path) -> Result<PathBuf> {
    if path.is_dir() && path.join("Contents/Info.plist").is_file() {
        Ok(path.to_path_buf())
    } else {
        bail!("not a macOS app bundle: {}", path.display())
    }
}

pub(crate) async fn handle_desktop_command(app_path: Option<PathBuf>, install: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("the VT Code desktop app is only available on macOS");
    }
    if install {
        return handle_desktop_install().await;
    }
    let app = resolve_desktop_app(app_path)?;
    // Address the bundle path directly (mirrors scripts/open-desktop-macos.sh):
    // display-name lookup is ambiguous when several development builds share
    // one bundle identifier, and -n always opens a fresh instance.
    let status = std::process::Command::new("open")
        .arg("-n")
        .arg(&app)
        .status()
        .with_context(|| format!("failed to launch {}", app.display()))?;
    if !status.success() {
        bail!("failed to open {}", app.display());
    }
    println!("Opened VT Code Desktop: {}", app.display());
    Ok(())
}

/// Download and install the desktop app from the prebuilt release asset.
/// Updates install in place; fresh installs go to `~/Applications` (like
/// `vtcode update --with-desktop`, this flag never prompts).
async fn handle_desktop_install() -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let updater = Updater::new(current_version).context("Failed to initialize updater")?;
    let install_dir = match existing_install_dir() {
        Some(dir) => dir,
        None => default_install_dir()?,
    };
    let outcome = install_desktop_app(&updater, &install_dir).await?;
    print_desktop_install_outcome(outcome);
    Ok(())
}

/// Report a desktop install/update outcome (shared by `desktop --install`
/// and the update command's desktop step).
pub(crate) fn print_desktop_install_outcome(outcome: DesktopInstallOutcome) {
    match outcome {
        DesktopInstallOutcome::Installed { version, needs_gatekeeper_approval } => {
            println!("{} Desktop app v{} installed successfully!", "✓".green(), version.bold());
            println!("{} Open it anytime with: vtcode desktop", "→".cyan());
            if needs_gatekeeper_approval {
                println!("{} First launch needs one manual approval (this build is not notarized).", "!".yellow());
                println!(
                    "{} Right-click VT Code.app in Finder > Open > Open again; or System Settings > Privacy & Security > Open Anyway.",
                    "→".cyan()
                );
            }
        }
        DesktopInstallOutcome::UpToDate(version) => {
            println!("{} Desktop app is already up to date (v{})", "✓".green(), version);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal bundle layout the resolver accepts.
    fn write_bundle(root: &Path, name: &str) -> PathBuf {
        let app = root.join(name);
        std::fs::create_dir_all(app.join("Contents")).expect("bundle contents");
        std::fs::write(app.join("Contents/Info.plist"), "<plist/>").expect("info plist");
        app
    }

    #[test]
    fn explicit_path_wins_and_invalid_bundles_are_rejected() {
        let workspace = tempfile::tempdir().expect("workspace");
        let app = write_bundle(workspace.path(), "Custom.app");
        let resolved = resolve_desktop_app(Some(app)).expect("explicit bundle");
        assert!(resolved.is_absolute(), "{resolved:?}");
        assert_eq!(resolved.file_name().expect("bundle name").to_str(), Some("Custom.app"));

        let empty = workspace.path().join("Empty.app");
        std::fs::create_dir_all(&empty).expect("empty dir");
        resolve_desktop_app(Some(empty)).expect_err("dir without Info.plist must fail");
        resolve_desktop_app(Some(workspace.path().join("Missing.app"))).expect_err("missing path must fail");
    }

    #[test]
    fn environment_override_beats_standard_locations() {
        let env = vtcode_commons::env_lock::lock();
        let previous = std::env::var_os(DESKTOP_APP_ENV);
        let workspace = tempfile::tempdir().expect("workspace");
        let app = write_bundle(workspace.path(), "Env.app");
        env.set_var(DESKTOP_APP_ENV, &app);
        let resolved = resolve_desktop_app(None).expect("env bundle");
        if let Some(previous) = previous {
            env.set_var(DESKTOP_APP_ENV, previous);
        } else {
            env.remove_var(DESKTOP_APP_ENV);
        }
        assert!(resolved.is_absolute(), "{resolved:?}");
        assert_eq!(resolved.file_name().expect("bundle name").to_str(), Some("Env.app"));
    }

    #[test]
    fn scan_prefers_first_valid_bundle_and_reports_recovery_paths() {
        let workspace = tempfile::tempdir().expect("workspace");
        let first = write_bundle(workspace.path(), "First.app");
        let second = write_bundle(workspace.path(), "Second.app");
        let missing = workspace.path().join("Missing.app");
        // Asymmetric roots: a missing entry is skipped, the first valid wins.
        assert_eq!(scan_locations(&[missing.clone(), first.clone(), second]).expect("scan"), first);

        let error = scan_locations(&[missing]).expect_err("empty scan must fail");
        let message = format!("{error:#}");
        assert!(message.contains("vtcode desktop --install"), "{message}");
        assert!(message.contains("vtcode desktop <path>"), "{message}");
        assert!(message.contains(DESKTOP_APP_ENV), "{message}");
        assert!(message.contains("scripts/package-desktop-macos.sh"), "{message}");
    }

    /// `--install` fails hard off macOS instead of silently skipping.
    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn install_fails_off_macos() {
        let error = handle_desktop_command(None, true).await.expect_err("must fail, not install");
        assert!(error.to_string().contains("only available on macOS"), "{error:#}");
    }
}
