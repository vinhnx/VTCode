use anyhow::{Context, Result, anyhow};
use std::process::Command;

use super::debug_log;

pub(super) fn install_with_smart_detection() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if command_exists("brew") {
            eprintln!("Installing ripgrep via Homebrew...");
            debug_log("Attempting installation via Homebrew");
            return install_via_homebrew();
        }
        if command_exists("cargo") {
            eprintln!("Installing ripgrep via Cargo...");
            debug_log("Attempting installation via Cargo");
            return install_via_cargo();
        }
        Err(anyhow!(
            "No supported installer found. Install Homebrew or Cargo, or install ripgrep manually."
        ))
    }

    #[cfg(target_os = "linux")]
    {
        if command_exists("apt") || command_exists("apt-get") {
            eprintln!("Installing ripgrep via APT...");
            debug_log("Attempting installation via APT");
            if install_via_apt().is_ok() {
                return Ok(());
            }
            debug_log("APT installation failed, trying fallback");
        }
        if command_exists("cargo") {
            eprintln!("Installing ripgrep via Cargo...");
            debug_log("Attempting installation via Cargo (fallback)");
            return install_via_cargo();
        }
        Err(anyhow!(
            "No supported installer found. Install APT, Cargo, or install ripgrep manually."
        ))
    }

    #[cfg(target_os = "windows")]
    {
        if command_exists("cargo") {
            eprintln!("Installing ripgrep via Cargo...");
            debug_log("Attempting installation via Cargo");
            if install_via_cargo().is_ok() {
                return Ok(());
            }
            debug_log("Cargo installation failed, trying fallback");
        }
        if command_exists("choco") {
            eprintln!("Installing ripgrep via Chocolatey...");
            debug_log("Attempting installation via Chocolatey (fallback)");
            if install_via_chocolatey().is_ok() {
                return Ok(());
            }
            debug_log("Chocolatey installation failed, trying Scoop");
        }
        if command_exists("scoop") {
            eprintln!("Installing ripgrep via Scoop...");
            debug_log("Attempting installation via Scoop (fallback)");
            return install_via_scoop();
        }
        return Err(anyhow!(
            "No supported installer found. Install Cargo, Chocolatey, or Scoop."
        ));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(anyhow!(
            "Unsupported platform for automatic ripgrep installation"
        ))
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
        eprintln!("✓ Ripgrep installed successfully via Homebrew");
        Ok(())
    } else {
        Err(anyhow!(
            "Homebrew installation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(target_os = "linux")]
fn install_via_apt() -> Result<()> {
    let _ = Command::new("sudo").args(["apt", "update"]).output();

    let output = Command::new("sudo")
        .args(["apt", "install", "-y", "ripgrep"])
        .output()
        .context("Failed to execute apt install ripgrep")?;

    if output.status.success() {
        eprintln!("✓ Ripgrep installed successfully via apt");
        Ok(())
    } else {
        Err(anyhow!(
            "apt installation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn install_via_cargo() -> Result<()> {
    let output = Command::new("cargo")
        .args(["install", "ripgrep"])
        .output()
        .context("Failed to execute cargo install ripgrep")?;

    if output.status.success() {
        eprintln!("✓ Ripgrep installed successfully via cargo");
        Ok(())
    } else {
        Err(anyhow!(
            "cargo installation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(target_os = "windows")]
fn install_via_chocolatey() -> Result<()> {
    let output = Command::new("choco")
        .args(["install", "-y", "ripgrep"])
        .output()
        .context("Failed to execute choco install ripgrep")?;

    if output.status.success() {
        eprintln!("✓ Ripgrep installed successfully via Chocolatey");
        Ok(())
    } else {
        Err(anyhow!(
            "Chocolatey installation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(target_os = "windows")]
fn install_via_scoop() -> Result<()> {
    let output = Command::new("scoop")
        .args(["install", "ripgrep"])
        .output()
        .context("Failed to execute scoop install ripgrep")?;

    if output.status.success() {
        eprintln!("✓ Ripgrep installed successfully via Scoop");
        Ok(())
    } else {
        Err(anyhow!(
            "Scoop installation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
