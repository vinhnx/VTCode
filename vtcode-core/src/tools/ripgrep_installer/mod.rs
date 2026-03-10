//! Ripgrep availability detection and explicit installation management.
//!
//! This module handles detecting if ripgrep is available and installing it on
//! demand through `vtcode dependencies install ripgrep`.

mod platform;
mod state;

use crate::tools::ripgrep_binary::RIPGREP_INSTALL_COMMAND;
use anyhow::{Result, anyhow};
use std::process::Command;

use self::platform::install_with_smart_detection;
use self::state::{InstallLockGuard, InstallationCache};

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

    /// Attempt to install ripgrep for the current platform.
    /// Uses smart installer detection to try available tools first.
    pub fn install() -> Result<()> {
        debug_log("Installation attempt started");

        if matches!(Self::check(), RipgrepStatus::Available { .. }) {
            debug_log("ripgrep already available; skipping installation");
            return Ok(());
        }

        if std::env::var("VTCODE_RIPGREP_NO_INSTALL").is_ok() {
            debug_log("Auto-install disabled via VTCODE_RIPGREP_NO_INSTALL");
            return Err(anyhow!(
                "Ripgrep auto-installation disabled via VTCODE_RIPGREP_NO_INSTALL"
            ));
        }

        if InstallLockGuard::is_install_in_progress() {
            debug_log("Another installation is already in progress, skipping");
            return Err(anyhow!("Ripgrep installation already in progress"));
        }

        let _lock = InstallLockGuard::acquire()?;
        debug_log("Installation lock acquired");

        if !InstallationCache::is_stale()
            && let Ok(cache) = InstallationCache::load()
            && cache.status == "failed"
        {
            let reason = cache.failure_reason.as_deref().unwrap_or("unknown reason");
            debug_log(&format!("Cache shows previous failure: {}", reason));
            return Err(anyhow!(
                "Previous installation attempt failed ({}). Not retrying for 24 hours.",
                reason
            ));
        }

        let result = install_with_smart_detection();

        match result {
            Ok(()) => match Self::check() {
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
            },
            Err(err) => {
                let msg = err.to_string();
                debug_log(&format!("Installation failed: {}", msg));
                InstallationCache::mark_failed("all-methods", &msg);
                Err(anyhow!(msg))
            }
        }
    }

    /// Check if ripgrep is available, installing it if the caller explicitly requests that flow.
    pub fn ensure_available() -> Result<Self> {
        match Self::check() {
            status @ RipgrepStatus::Available { .. } => Ok(status),
            RipgrepStatus::NotFound => {
                eprintln!("Ripgrep not found. Run `{RIPGREP_INSTALL_COMMAND}` to install it.");
                Self::install()?;
                Ok(Self::check())
            }
            status => Ok(status),
        }
    }
}

pub(super) fn debug_log(message: &str) {
    if std::env::var("VTCODE_DEBUG_RIPGREP").is_ok() {
        eprintln!("[DEBUG ripgrep] {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ripgrep_status_check() {
        let status = RipgrepStatus::check();
        println!("Ripgrep status: {:?}", status);
    }
}
