use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeoutsConfig {
    /// Maximum duration (in seconds) for standard, non-PTY tools.
    #[serde(default = "TimeoutsConfig::default_default_ceiling_seconds")]
    pub default_ceiling_seconds: u64,
    /// Maximum duration (in seconds) for PTY-backed commands.
    #[serde(default = "TimeoutsConfig::default_pty_ceiling_seconds")]
    pub pty_ceiling_seconds: u64,
    /// Maximum duration (in seconds) for MCP calls.
    #[serde(default = "TimeoutsConfig::default_mcp_ceiling_seconds")]
    pub mcp_ceiling_seconds: u64,
    /// Percentage (0-100) of the ceiling after which the UI should warn.
    #[serde(default = "TimeoutsConfig::default_warning_threshold_percent")]
    pub warning_threshold_percent: u8,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            default_ceiling_seconds: Self::default_default_ceiling_seconds(),
            pty_ceiling_seconds: Self::default_pty_ceiling_seconds(),
            mcp_ceiling_seconds: Self::default_mcp_ceiling_seconds(),
            warning_threshold_percent: Self::default_warning_threshold_percent(),
        }
    }
}

impl TimeoutsConfig {
    const MIN_CEILING_SECONDS: u64 = 15;

    const fn default_default_ceiling_seconds() -> u64 {
        180
    }

    const fn default_pty_ceiling_seconds() -> u64 {
        300
    }

    const fn default_mcp_ceiling_seconds() -> u64 {
        120
    }

    const fn default_warning_threshold_percent() -> u8 {
        80
    }

    /// Convert the configured threshold into a fraction (0.0-1.0).
    pub fn warning_threshold_fraction(&self) -> f32 {
        f32::from(self.warning_threshold_percent) / 100.0
    }

    /// Normalize a ceiling value into an optional duration.
    pub fn ceiling_duration(&self, seconds: u64) -> Option<std::time::Duration> {
        if seconds == 0 {
            None
        } else {
            Some(std::time::Duration::from_secs(seconds))
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.warning_threshold_percent > 0 && self.warning_threshold_percent < 100,
            "timeouts.warning_threshold_percent must be between 1 and 99",
        );

        ensure!(
            self.default_ceiling_seconds == 0
                || self.default_ceiling_seconds >= Self::MIN_CEILING_SECONDS,
            "timeouts.default_ceiling_seconds must be at least {} seconds (or 0 to disable)",
            Self::MIN_CEILING_SECONDS
        );

        ensure!(
            self.pty_ceiling_seconds == 0 || self.pty_ceiling_seconds >= Self::MIN_CEILING_SECONDS,
            "timeouts.pty_ceiling_seconds must be at least {} seconds (or 0 to disable)",
            Self::MIN_CEILING_SECONDS
        );

        ensure!(
            self.mcp_ceiling_seconds == 0 || self.mcp_ceiling_seconds >= Self::MIN_CEILING_SECONDS,
            "timeouts.mcp_ceiling_seconds must be at least {} seconds (or 0 to disable)",
            Self::MIN_CEILING_SECONDS
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TimeoutsConfig;

    #[test]
    fn default_values_are_safe() {
        let config = TimeoutsConfig::default();
        assert_eq!(config.default_ceiling_seconds, 180);
        assert_eq!(config.pty_ceiling_seconds, 300);
        assert_eq!(config.mcp_ceiling_seconds, 120);
        assert_eq!(config.warning_threshold_percent, 80);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn zero_ceiling_disables_limit() {
        let mut config = TimeoutsConfig::default();
        config.default_ceiling_seconds = 0;
        assert!(config.validate().is_ok());
        assert!(
            config
                .ceiling_duration(config.default_ceiling_seconds)
                .is_none()
        );
    }

    #[test]
    fn warning_threshold_bounds_are_enforced() {
        let mut config = TimeoutsConfig::default();
        config.warning_threshold_percent = 0;
        assert!(config.validate().is_err());

        config.warning_threshold_percent = 100;
        assert!(config.validate().is_err());
    }
}
