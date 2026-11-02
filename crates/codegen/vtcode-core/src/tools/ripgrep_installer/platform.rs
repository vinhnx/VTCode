use anyhow::{Context, Result, anyhow};
use std::process::Command;

use super::debug_log;

/// Actionable manual install steps shown whenever automatic installation fails.
pub(super) fn manual_install_guide() -> &'static str {
    "Install ripgrep manually and rerun `vtcode dependencies status ripgrep`:\n\
     - Ubuntu/Debian: sudo apt update && sudo apt install -y ripgrep\n\
     - macOS: brew install ripgrep\n\
     - Windows: choco install ripgrep (or: scoop install ripgrep)\n\
     - Any OS with Rust: cargo install ripgrep --locked\n\
     - Or download a release from https://github.com/BurntSushi/ripgrep/releases"
}

pub(super) fn install_with_smart_detection() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut first_error: Option<String> = None;
        if command_exists("brew") {
            tracing::info!(method = "homebrew", "installing ripgrep");
            debug_log("Attempting installation via Homebrew");
            match install_via_homebrew() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    debug_log(&format!("Homebrew installation failed: {err:#}, trying fallback"));
                    first_error = Some(format!("brew: {err:#}"));
                }
            }
        }
        if command_exists("cargo") {
            tracing::info!(method = "cargo", "installing ripgrep");
            debug_log("Attempting installation via Cargo");
            match install_via_cargo() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    return Err(anyhow!(
                        "Automatic ripgrep installation failed ({}; cargo: {err:#}).\n{}",
                        first_error.as_deref().unwrap_or("brew not attempted"),
                        manual_install_guide()
                    ));
                }
            }
        }
        if let Some(first) = first_error {
            return Err(anyhow!("Automatic ripgrep installation failed ({first}).\n{}", manual_install_guide()));
        }
        Err(anyhow!("No supported installer found (need Homebrew or Cargo).\n{}", manual_install_guide()))
    }

    #[cfg(target_os = "linux")]
    {
        let mut apt_error: Option<String> = None;
        if command_exists("apt") || command_exists("apt-get") {
            tracing::info!(method = "apt", "installing ripgrep");
            debug_log("Attempting installation via APT");
            match install_via_apt() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    debug_log(&format!("APT installation failed: {err:#}, trying fallback"));
                    apt_error = Some(format!("{err:#}"));
                }
            }
        }
        if command_exists("cargo") {
            tracing::info!(method = "cargo", "installing ripgrep");
            debug_log("Attempting installation via Cargo (fallback)");
            match install_via_cargo() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    return Err(anyhow!(
                        "Automatic ripgrep installation failed (apt: {}; cargo: {err:#}).\n{}",
                        apt_error.as_deref().unwrap_or("not attempted"),
                        manual_install_guide()
                    ));
                }
            }
        }
        if let Some(apt_err) = apt_error {
            return Err(anyhow!(
                "Automatic ripgrep installation via apt failed: {apt_err}\n{}",
                manual_install_guide()
            ));
        }
        Err(anyhow!("No supported installer found (need APT or Cargo).\n{}", manual_install_guide()))
    }

    #[cfg(target_os = "windows")]
    {
        let mut failures: Vec<String> = Vec::new();
        if command_exists("cargo") {
            tracing::info!(method = "cargo", "installing ripgrep");
            debug_log("Attempting installation via Cargo");
            match install_via_cargo() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    debug_log(&format!("Cargo installation failed: {err:#}, trying fallback"));
                    failures.push(format!("cargo: {err:#}"));
                }
            }
        }
        if command_exists("choco") {
            tracing::info!(method = "chocolatey", "installing ripgrep");
            debug_log("Attempting installation via Chocolatey (fallback)");
            match install_via_chocolatey() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    debug_log(&format!("Chocolatey installation failed: {err:#}, trying Scoop"));
                    failures.push(format!("choco: {err:#}"));
                }
            }
        }
        if command_exists("scoop") {
            tracing::info!(method = "scoop", "installing ripgrep");
            debug_log("Attempting installation via Scoop (fallback)");
            match install_via_scoop() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    failures.push(format!("scoop: {err:#}"));
                    return Err(anyhow!(
                        "Automatic ripgrep installation failed ({}).\n{}",
                        failures.join("; "),
                        manual_install_guide()
                    ));
                }
            }
        }
        if failures.is_empty() {
            return Err(anyhow!(
                "No supported installer found (need Cargo, Chocolatey, or Scoop).\n{}",
                manual_install_guide()
            ));
        }
        Err(anyhow!(
            "Automatic ripgrep installation failed ({}).\n{}",
            failures.join("; "),
            manual_install_guide()
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(anyhow!("Unsupported platform for automatic ripgrep installation"))
    }
}

fn command_exists(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "where", cmd])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
fn install_via_homebrew() -> Result<()> {
    let output = Command::new("brew")
        .arg("install")
        .arg("ripgrep")
        .output()
        .context("Failed to execute brew install ripgrep")?;

    if output.status.success() {
        tracing::info!(method = "homebrew", "ripgrep installed successfully");
        Ok(())
    } else {
        Err(anyhow!("Homebrew installation failed: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

#[cfg(target_os = "linux")]
fn install_via_apt() -> Result<()> {
    use std::io::IsTerminal;

    // Check if we're already running as root (skip sudo if so)
    let is_root = nix::unistd::getuid().is_root();

    if is_root {
        // Running as root, no sudo needed
        tracing::info!(method = "apt", "running as root, skipping sudo");
        let _ = Command::new("apt").args(["update"]).output();

        let output = Command::new("apt")
            .args(["install", "-y", "ripgrep"])
            .output()
            .context("Failed to execute apt install ripgrep")?;

        if output.status.success() {
            tracing::info!(method = "apt", "ripgrep installed successfully");
            Ok(())
        } else {
            Err(anyhow!("apt installation failed: {}", String::from_utf8_lossy(&output.stderr).trim()))
        }
    } else {
        // Need sudo - log a warning about elevated privileges
        tracing::warn!(method = "apt", "using sudo to install ripgrep - this requires elevated privileges");

        // Check if passwordless sudo is available
        let sudo_ok = Command::new("sudo")
            .args(["-n", "true"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if !sudo_ok {
            // Passwordless sudo is unavailable. When attached to a terminal,
            // run sudo interactively so the user gets a password prompt
            // instead of a hard failure (issue #802).
            if std::io::stdin().is_terminal() {
                tracing::info!(method = "apt", "requesting sudo password interactively");
                debug_log("Passwordless sudo unavailable, trying interactive sudo");
                return install_via_apt_interactive();
            }
            return Err(anyhow!(
                "sudo requires a password but no terminal is available for a prompt. Run `sudo apt update && sudo apt install -y ripgrep` manually"
            ));
        }

        let _ = Command::new("sudo").args(["apt", "update"]).output();

        let output = Command::new("sudo")
            .args(["apt", "install", "-y", "ripgrep"])
            .output()
            .context("Failed to execute apt install ripgrep")?;

        if output.status.success() {
            tracing::info!(method = "apt", "ripgrep installed successfully");
            Ok(())
        } else {
            Err(anyhow!("apt installation failed: {}", String::from_utf8_lossy(&output.stderr).trim()))
        }
    }
}

/// Run `apt install` through an interactive sudo prompt. Uses `.status()`
/// (inherited stdio) so the sudo password prompt reaches the terminal,
/// unlike `.output()` which captures stdio and cannot prompt.
#[cfg(target_os = "linux")]
fn install_via_apt_interactive() -> Result<()> {
    // Best-effort refresh like the non-interactive paths: a failing
    // third-party repo must not block the install itself.
    let _ = Command::new("sudo").args(["apt", "update"]).status();

    let install_status = Command::new("sudo")
        .args(["apt", "install", "-y", "ripgrep"])
        .status()
        .context("Failed to execute sudo apt install ripgrep")?;

    if install_status.success() {
        tracing::info!(method = "apt", "ripgrep installed successfully via interactive sudo");
        Ok(())
    } else {
        Err(anyhow!("interactive sudo apt install failed with status {install_status}"))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn install_via_cargo() -> Result<()> {
    let output = Command::new("cargo")
        .args(["install", "ripgrep"])
        .output()
        .context("Failed to execute cargo install ripgrep")?;

    if output.status.success() {
        tracing::info!(method = "cargo", "ripgrep installed successfully");
        Ok(())
    } else {
        Err(anyhow!("cargo installation failed: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

#[cfg(target_os = "windows")]
fn install_via_chocolatey() -> Result<()> {
    let output = Command::new("choco")
        .args(["install", "-y", "ripgrep"])
        .output()
        .context("Failed to execute choco install ripgrep")?;

    if output.status.success() {
        tracing::info!(method = "chocolatey", "ripgrep installed successfully");
        Ok(())
    } else {
        Err(anyhow!("Chocolatey installation failed: {}", String::from_utf8_lossy(&output.stderr)))
    }
}

#[cfg(target_os = "windows")]
fn install_via_scoop() -> Result<()> {
    let output = Command::new("scoop")
        .args(["install", "ripgrep"])
        .output()
        .context("Failed to execute scoop install ripgrep")?;

    if output.status.success() {
        tracing::info!(method = "scoop", "ripgrep installed successfully");
        Ok(())
    } else {
        Err(anyhow!("Scoop installation failed: {}", String::from_utf8_lossy(&output.stderr)))
    }
}
