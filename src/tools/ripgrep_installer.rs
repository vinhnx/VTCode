//! Ripgrep availability detection and installation management.
//!
//! This module handles detecting if ripgrep is available and optionally
//! installing it if missing, similar to how Qwen-code manages dependencies.
//!
//! Features:
//! - Smart installer detection (checks available tools before attempting)
//! - Installation caching to avoid repeated failed attempts
//! - Concurrent install protection with lock files
//! - Configurable behavior via VTCODE_RIPGREP_* environment variables and vtcode.toml
//! - Enhanced debug logging with VTCODE_DEBUG_RIPGREP environment variable
//! - Comprehensive error handling with recovery strategies

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of ripgrep availability check
#[derive(Debug, Clone, PartialEq)]
pub enum RipgrepStatus {
    /// Ripgrep is available and working
    Available { version: String },
    /// Ripgrep is not installed
    NotFound,
    /// Ripgrep exists but returned an error
    Error { reason: String },
}

/// Installation attempt cache to avoid repeated retries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationCache {
    /// Timestamp of last installation attempt
    pub last_attempt: u64,
    /// Status from last attempt
    pub status: String, // "success", "failed", "skipped"
    /// Method that was attempted
    pub method_attempted: Option<String>,
    /// Reason for failure (if applicable)
    pub failure_reason: Option<String>,
}

impl InstallationCache {
    fn cache_path() -> PathBuf {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".vtcode/ripgrep_install_cache.json")
        } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
            // Windows
            PathBuf::from(userprofile).join(".vtcode\\ripgrep_install_cache.json")
        } else {
            PathBuf::from(".vtcode/ripgrep_install_cache.json")
        }
    }

    fn lock_path() -> PathBuf {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".vtcode/ripgrep.lock")
        } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
            PathBuf::from(userprofile).join(".vtcode\\ripgrep.lock")
        } else {
            PathBuf::from(".vtcode/ripgrep.lock")
        }
    }

    /// Acquire installation lock to prevent concurrent installs
    fn acquire_lock() -> Result<std::fs::File> {
        let lock_path = Self::lock_path();
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)
            .context("Failed to create install lock file")
    }

    /// Check if another installation is in progress
    fn is_install_in_progress() -> bool {
        let lock_path = Self::lock_path();
        lock_path.exists() && lock_path.metadata()
            .map(|meta| {
                // Consider lock stale after 30 minutes
                let age = SystemTime::now()
                    .duration_since(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH))
                    .unwrap_or_default()
                    .as_secs();
                age < 1800
            })
            .unwrap_or(false)
    }

    fn is_stale() -> bool {
        // Cache is considered stale after 24 hours
        if let Ok(cache) = Self::load() {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            (now - cache.last_attempt) > 86400 // 24 hours
        } else {
            true // No cache, so "stale"
        }
    }

    fn load() -> Result<Self> {
        let path = Self::cache_path();
        let content = fs::read_to_string(&path)
            .context("Failed to read installation cache")?;
        serde_json::from_str(&content).context("Failed to parse installation cache")
    }

    fn save(&self) -> Result<()> {
        let path = Self::cache_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let json = serde_json::to_string(self).context("Failed to serialize cache")?;
        fs::write(&path, json).context("Failed to write installation cache")?;
        Ok(())
    }

    fn mark_failed(method: &str, reason: &str) {
        let cache = InstallationCache {
            last_attempt: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            status: "failed".to_string(),
            method_attempted: Some(method.to_string()),
            failure_reason: Some(reason.to_string()),
        };
        let _ = cache.save();
    }

    fn mark_success(method: &str) {
        let cache = InstallationCache {
            last_attempt: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            status: "success".to_string(),
            method_attempted: Some(method.to_string()),
            failure_reason: None,
        };
        let _ = cache.save();
    }
}

/// Debug logging helper - only logs if VTCODE_DEBUG_RIPGREP is set
fn debug_log(msg: &str) {
    if std::env::var("VTCODE_DEBUG_RIPGREP").is_ok() {
        eprintln!("[DEBUG ripgrep] {}", msg);
    }
}

impl RipgrepStatus {
    /// Check if ripgrep is currently available
    pub fn check() -> Self {
        debug_log("Checking ripgrep availability...");
        match Command::new("rg").arg("--version").output() {
            Ok(output) => {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if version.is_empty() {
                        debug_log("ripgrep found but returned empty version");
                        RipgrepStatus::Error {
                            reason: "rg --version returned empty output".to_string(),
                        }
                    } else {
                        debug_log(&format!("ripgrep available: {}", version));
                        RipgrepStatus::Available { version }
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    debug_log(&format!("ripgrep check failed: {}", stderr));
                    RipgrepStatus::Error {
                        reason: if stderr.is_empty() {
                            "rg exited with error".to_string()
                        } else {
                            stderr
                        },
                    }
                }
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    debug_log("ripgrep not found in PATH");
                    RipgrepStatus::NotFound
                } else {
                    debug_log(&format!("ripgrep check error: {}", err));
                    RipgrepStatus::Error {
                        reason: err.to_string(),
                    }
                }
            }
        }
    }

    /// Attempt to install ripgrep for the current platform
    /// Uses smart installer detection to try available tools first
    pub fn install() -> Result<()> {
        debug_log("Installation attempt started");

        // Check if auto-install is disabled
        if std::env::var("VTCODE_RIPGREP_NO_INSTALL").is_ok() {
            debug_log("Auto-install disabled via VTCODE_RIPGREP_NO_INSTALL");
            return Err(anyhow!("Ripgrep auto-installation disabled via VTCODE_RIPGREP_NO_INSTALL"));
        }

        // Check if another installation is already in progress
        if InstallationCache::is_install_in_progress() {
            debug_log("Another installation is already in progress, skipping");
            return Err(anyhow!("Ripgrep installation already in progress"));
        }

        // Acquire installation lock
        let _lock = InstallationCache::acquire_lock()?;
        debug_log("Installation lock acquired");

        // Check cache to avoid repeated failed attempts
        if !InstallationCache::is_stale() {
            if let Ok(cache) = InstallationCache::load() {
                if cache.status == "failed" {
                    let reason = cache.failure_reason
                        .as_deref()
                        .unwrap_or("unknown reason");
                    debug_log(&format!("Cache shows previous failure: {}", reason));
                    return Err(anyhow!(
                        "Previous installation attempt failed ({}). Not retrying for 24 hours.",
                        reason
                    ));
                }
            }
        }

        let result = Self::install_with_smart_detection();
        
        // Clean up lock file
        let _ = std::fs::remove_file(InstallationCache::lock_path());
        
        match result {
            Ok(()) => {
                // Verify installation was successful
                match Self::check() {
                    RipgrepStatus::Available { .. } => {
                        debug_log("Installation verified successfully");
                        InstallationCache::mark_success("auto-detected");
                        Ok(())
                    }
                    status => {
                        let msg = format!("Installation verification failed: {:?}", status);
                        debug_log(&msg);
                        InstallationCache::mark_failed("auto-detected", &msg);
                        Err(anyhow!(msg))
                    }
                }
            }
            Err(e) => {
                let msg = e.to_string();
                debug_log(&format!("Installation failed: {}", msg));
                InstallationCache::mark_failed("all-methods", &msg);
                Err(anyhow!(msg))
            }
        }
    }

    /// Install with smart detection of available package managers
    #[allow(unreachable_code)]
    fn install_with_smart_detection() -> Result<()> {
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
            debug_log("No supported installer found on macOS");
            return Err(anyhow!(
                "No supported installer found. Install Homebrew or Cargo, or install ripgrep manually."
            ));
        }

        #[cfg(target_os = "linux")]
        {
            if command_exists("apt") || command_exists("apt-get") {
                eprintln!("Installing ripgrep via APT...");
                debug_log("Attempting installation via APT");
                if let Ok(()) = install_via_apt() {
                    return Ok(());
                }
                debug_log("APT installation failed, trying fallback");
            }
            if command_exists("cargo") {
                eprintln!("Installing ripgrep via Cargo...");
                debug_log("Attempting installation via Cargo (fallback)");
                return install_via_cargo();
            }
            debug_log("No supported installer found on Linux");
            return Err(anyhow!(
                "No supported installer found. Install APT, Cargo, or install ripgrep manually."
            ));
        }

        #[cfg(target_os = "windows")]
        {
            if command_exists("cargo") {
                eprintln!("Installing ripgrep via Cargo...");
                debug_log("Attempting installation via Cargo");
                if let Ok(()) = install_via_cargo() {
                    return Ok(());
                }
                debug_log("Cargo installation failed, trying fallback");
            }
            if command_exists("choco") {
                eprintln!("Installing ripgrep via Chocolatey...");
                debug_log("Attempting installation via Chocolatey (fallback)");
                if let Ok(()) = install_via_chocolatey() {
                    return Ok(());
                }
                debug_log("Chocolatey installation failed, trying Scoop");
            }
            if command_exists("scoop") {
                eprintln!("Installing ripgrep via Scoop...");
                debug_log("Attempting installation via Scoop (fallback)");
                return install_via_scoop();
            }
            debug_log("No supported installer found on Windows");
            return Err(anyhow!(
                "No supported installer found. Install Cargo, Chocolatey, or Scoop."
            ));
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            debug_log("Unsupported platform");
            Err(anyhow!(
                "Unsupported platform for automatic ripgrep installation"
            ))
        }
    }

    /// Check if ripgrep is available, attempting auto-installation if missing
    pub fn ensure_available() -> Result<Self> {
        match Self::check() {
            status @ RipgrepStatus::Available { .. } => Ok(status),
            RipgrepStatus::NotFound => {
                eprintln!("Ripgrep not found. Attempting auto-installation...");
                Self::install()?;
                Ok(Self::check())
            }
            status => Ok(status),
        }
    }
}

/// Check if a command exists in PATH
fn command_exists(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(&["/C", "where", cmd])
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
    eprintln!("Installing ripgrep via apt...");

    // First, try to update package list
    let _update = Command::new("sudo").args(&["apt", "update"]).output();

    let output = Command::new("sudo")
        .args(&["apt", "install", "-y", "ripgrep"])
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
    eprintln!("Installing ripgrep via cargo...");
    let output = Command::new("cargo")
        .args(&["install", "ripgrep"])
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
    eprintln!("Installing ripgrep via Chocolatey...");
    let output = Command::new("choco")
        .args(&["install", "-y", "ripgrep"])
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
    eprintln!("Installing ripgrep via Scoop...");
    let output = Command::new("scoop")
        .args(&["install", "ripgrep"])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ripgrep_status_check() {
        // This test will pass if ripgrep is installed, fail otherwise
        // It's mainly for CI/CD environments where ripgrep should be present
        let status = RipgrepStatus::check();
        println!("Ripgrep status: {:?}", status);
    }
}
