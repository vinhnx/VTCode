//! Update configuration for VT Code auto-updater
//!
//! Manages release channel preferences, version pinning, and download mirrors.
//! Configuration stored in `~/.vtcode/update.toml`.

use crate::defaults::get_config_dir;
use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Release channel for VT Code updates
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    /// Stable releases (default)
    #[default]
    Stable,
    /// Beta releases (pre-release testing)
    Beta,
    /// Nightly builds (bleeding edge)
    Nightly,
}

impl std::fmt::Display for ReleaseChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stable => write!(f, "stable"),
            Self::Beta => write!(f, "beta"),
            Self::Nightly => write!(f, "nightly"),
        }
    }
}

/// Mirror configuration for download fallback
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MirrorConfig {
    /// Primary mirror URL (GitHub Releases by default)
    pub primary: Option<String>,
    /// Fallback mirrors in order of preference
    #[serde(default)]
    pub fallbacks: Vec<String>,
    /// Enable geographic mirror selection
    #[serde(default = "default_true")]
    pub geo_select: bool,
}

/// Version pinning configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct VersionPin {
    /// Pinned version (if set, auto-update will stay on this version)
    pub version: Option<Version>,
    /// Reason for pinning (user note)
    pub reason: Option<String>,
    /// Auto-unpin after successful update check (for temporary pins)
    #[serde(default)]
    pub auto_unpin: bool,
}

/// Update configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateConfig {
    /// Release channel to follow
    #[serde(default)]
    pub channel: ReleaseChannel,

    /// Pinned version (None = follow channel latest)
    #[serde(default)]
    pub pin: Option<VersionPin>,

    /// Mirror configuration
    #[serde(default)]
    pub mirrors: MirrorConfig,

    /// Auto-update check interval in hours (0 = disable)
    #[serde(default = "default_check_interval")]
    pub check_interval_hours: u64,

    /// Download timeout in seconds
    #[serde(default = "default_download_timeout")]
    pub download_timeout_secs: u64,

    /// Keep backup of previous version after update
    #[serde(default = "default_true")]
    pub keep_backup: bool,

    /// Auto-rollback on startup if new version fails
    #[serde(default)]
    pub auto_rollback: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            channel: ReleaseChannel::Stable,
            pin: None,
            mirrors: MirrorConfig::default(),
            check_interval_hours: default_check_interval(),
            download_timeout_secs: default_download_timeout(),
            keep_backup: true,
            auto_rollback: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_check_interval() -> u64 {
    24 // Check daily by default
}

fn default_download_timeout() -> u64 {
    300 // 5 minutes
}

impl UpdateConfig {
    /// Load update configuration from default location
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path().context("Failed to determine update config path")?;

        if !config_path.exists() {
            // Return defaults if config doesn't exist
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read update config: {}", config_path.display()))?;

        let config: UpdateConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse update config: {}", config_path.display()))?;

        Ok(config)
    }

    /// Save update configuration to default location
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path().context("Failed to determine update config path")?;

        // Ensure directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let content = toml::to_string_pretty(self).context("Failed to serialize update config")?;

        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write update config: {}", config_path.display()))?;

        Ok(())
    }

    /// Get the configuration file path
    pub fn config_path() -> Result<PathBuf> {
        let config_dir = get_config_dir().context("Failed to get config directory")?;
        Ok(config_dir.join("update.toml"))
    }

    /// Check if version is pinned
    pub fn is_pinned(&self) -> bool {
        self.pin.as_ref().is_some_and(|p| p.version.is_some())
    }

    /// Get pinned version if set
    pub fn pinned_version(&self) -> Option<&Version> {
        self.pin.as_ref().and_then(|p| p.version.as_ref())
    }

    /// Set version pin
    pub fn set_pin(&mut self, version: Version, reason: Option<String>, auto_unpin: bool) {
        self.pin = Some(VersionPin {
            version: Some(version),
            reason,
            auto_unpin,
        });
    }

    /// Clear version pin
    pub fn clear_pin(&mut self) {
        self.pin = None;
    }

    /// Check if update check is due based on interval
    pub fn is_check_due(&self, last_check: Option<std::time::SystemTime>) -> bool {
        if self.check_interval_hours == 0 {
            return false; // Checks disabled
        }

        let Some(last_check) = last_check else {
            return true; // Never checked before
        };

        let elapsed = std::time::SystemTime::now()
            .duration_since(last_check)
            .unwrap_or_default();

        elapsed >= std::time::Duration::from_secs(self.check_interval_hours * 3600)
    }
}

/// Create example update configuration
pub fn create_example_config() -> String {
    r#"# VT Code Update Configuration
# Location: ~/.vtcode/update.toml

# Release channel to follow
# Options: stable (default), beta, nightly
channel = "stable"

# Version pinning (optional)
# Uncomment to pin to a specific version
# [pin]
# version = "0.85.3"
# reason = "Waiting for bug fix in next release"
# auto_unpin = false

# Download mirrors (optional)
# [mirrors]
# primary = "https://github.com/vinhnx/vtcode/releases"
# fallbacks = [
#     "https://mirror.example.com/vtcode",
# ]
# geo_select = true

# Auto-update check interval in hours (0 = disable)
check_interval_hours = 24

# Download timeout in seconds
download_timeout_secs = 300

# Keep backup of previous version after update
keep_backup = true

# Auto-rollback on startup if new version fails
auto_rollback = false
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = UpdateConfig::default();
        assert_eq!(config.channel, ReleaseChannel::Stable);
        assert_eq!(config.check_interval_hours, 24);
        assert_eq!(config.download_timeout_secs, 300);
        assert!(config.keep_backup);
        assert!(!config.auto_rollback);
    }

    #[test]
    fn test_release_channel_display() {
        assert_eq!(ReleaseChannel::Stable.to_string(), "stable");
        assert_eq!(ReleaseChannel::Beta.to_string(), "beta");
        assert_eq!(ReleaseChannel::Nightly.to_string(), "nightly");
    }

    #[test]
    fn test_version_pin() {
        let mut config = UpdateConfig::default();
        let version = Version::parse("0.85.3").unwrap();
        config.set_pin(version.clone(), Some("Testing".to_string()), false);

        assert!(config.is_pinned());
        assert_eq!(config.pinned_version(), Some(&version));

        config.clear_pin();
        assert!(!config.is_pinned());
    }
}
