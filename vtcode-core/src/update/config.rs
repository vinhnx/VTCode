//! Configuration for the self-update mechanism

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Update channel selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Stable releases only
    Stable,
    /// Beta releases (pre-releases)
    Beta,
    /// Development builds (nightly)
    Nightly,
}

impl Default for UpdateChannel {
    fn default() -> Self {
        Self::Stable
    }
}

impl std::fmt::Display for UpdateChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stable => write!(f, "stable"),
            Self::Beta => write!(f, "beta"),
            Self::Nightly => write!(f, "nightly"),
        }
    }
}

/// Update frequency configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateFrequency {
    /// Check on every launch
    Always,
    /// Check once per day
    Daily,
    /// Check once per week
    Weekly,
    /// Never check automatically
    Never,
}

impl Default for UpdateFrequency {
    fn default() -> Self {
        Self::Daily
    }
}

/// Configuration for the update system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Whether automatic updates are enabled
    pub enabled: bool,

    /// Update channel to use
    pub channel: UpdateChannel,

    /// How often to check for updates
    pub frequency: UpdateFrequency,

    /// Whether to automatically download updates
    pub auto_download: bool,

    /// Whether to automatically install updates
    pub auto_install: bool,

    /// Directory for storing update files
    pub update_dir: PathBuf,

    /// Directory for storing backups
    pub backup_dir: PathBuf,

    /// Maximum number of backups to keep
    pub max_backups: usize,

    /// Timeout for download operations (in seconds)
    pub download_timeout_secs: u64,

    /// Whether to verify signatures
    pub verify_signatures: bool,

    /// Whether to verify checksums
    pub verify_checksums: bool,

    /// GitHub API token for authenticated requests (optional)
    pub github_token: Option<String>,

    /// Custom GitHub API base URL (for enterprise)
    pub github_api_base: Option<String>,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let vtcode_dir = home_dir.join(".vtcode");

        Self {
            enabled: false,
            channel: UpdateChannel::default(),
            frequency: UpdateFrequency::default(),
            auto_download: false,
            auto_install: false,
            update_dir: vtcode_dir.join("updates"),
            backup_dir: vtcode_dir.join("backups"),
            max_backups: 3,
            download_timeout_secs: 300,
            verify_signatures: true,
            verify_checksums: true,
            github_token: None,
            github_api_base: None,
        }
    }
}

impl UpdateConfig {
    /// Load configuration from environment variables and defaults
    pub fn from_env() -> Result<Self> {
        let mut config = Self::default();

        // Check environment variables
        if let Ok(val) = std::env::var("VTCODE_UPDATE_ENABLED") {
            config.enabled = val.parse().unwrap_or(false);
        }

        if let Ok(val) = std::env::var("VTCODE_UPDATE_CHANNEL") {
            config.channel = match val.to_lowercase().as_str() {
                "beta" => UpdateChannel::Beta,
                "nightly" => UpdateChannel::Nightly,
                _ => UpdateChannel::Stable,
            };
        }

        if let Ok(val) = std::env::var("VTCODE_UPDATE_FREQUENCY") {
            config.frequency = match val.to_lowercase().as_str() {
                "always" => UpdateFrequency::Always,
                "weekly" => UpdateFrequency::Weekly,
                "never" => UpdateFrequency::Never,
                _ => UpdateFrequency::Daily,
            };
        }

        if let Ok(val) = std::env::var("VTCODE_UPDATE_AUTO_DOWNLOAD") {
            config.auto_download = val.parse().unwrap_or(false);
        }

        if let Ok(val) = std::env::var("VTCODE_UPDATE_AUTO_INSTALL") {
            config.auto_install = val.parse().unwrap_or(false);
        }

        if let Ok(val) = std::env::var("VTCODE_UPDATE_DIR") {
            config.update_dir = PathBuf::from(val);
        }

        if let Ok(val) = std::env::var("VTCODE_UPDATE_BACKUP_DIR") {
            config.backup_dir = PathBuf::from(val);
        }

        if let Ok(val) = std::env::var("VTCODE_UPDATE_MAX_BACKUPS") {
            config.max_backups = val.parse().unwrap_or(3);
        }

        if let Ok(val) = std::env::var("GITHUB_TOKEN") {
            config.github_token = Some(val);
        }

        if let Ok(val) = std::env::var("GITHUB_API_BASE") {
            config.github_api_base = Some(val);
        }

        Ok(config)
    }

    /// Ensure required directories exist
    pub fn ensure_directories(&self) -> Result<()> {
        std::fs::create_dir_all(&self.update_dir)?;
        std::fs::create_dir_all(&self.backup_dir)?;
        Ok(())
    }

    /// Get the GitHub API base URL
    pub fn github_api_base(&self) -> &str {
        self.github_api_base
            .as_deref()
            .unwrap_or("https://api.github.com")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_channel_display() {
        assert_eq!(UpdateChannel::Stable.to_string(), "stable");
        assert_eq!(UpdateChannel::Beta.to_string(), "beta");
        assert_eq!(UpdateChannel::Nightly.to_string(), "nightly");
    }

    #[test]
    fn test_default_config() {
        let config = UpdateConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.channel, UpdateChannel::Stable);
        assert_eq!(config.frequency, UpdateFrequency::Daily);
        assert!(!config.auto_download);
        assert!(!config.auto_install);
        assert_eq!(config.max_backups, 3);
    }
}
