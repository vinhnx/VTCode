use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Floor for the HTTP timeout keys. Values in `1..MIN` are almost always a unit
/// mistake (a caller meaning milliseconds), so they are rejected at load time
/// rather than turned into reliably-failing clients.
const MIN_HTTP_TIMEOUT_SECONDS: u64 = 5;

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
    /// Maximum duration (in seconds) for streaming API responses.
    #[serde(default = "TimeoutsConfig::default_streaming_ceiling_seconds")]
    pub streaming_ceiling_seconds: u64,
    /// Maximum duration (in seconds) for an explicit long-running command wait.
    #[serde(default = "TimeoutsConfig::default_long_running_command_ceiling_seconds")]
    pub long_running_command_ceiling_seconds: u64,
    /// Percentage (0-100) of the ceiling after which the UI should warn.
    #[serde(default = "TimeoutsConfig::default_warning_threshold_percent")]
    pub warning_threshold_percent: u8,
    /// Adaptive timeout decay ratio (0.1-1.0). Lower relaxes faster back to ceiling.
    #[serde(default = "TimeoutsConfig::default_decay_ratio")]
    pub adaptive_decay_ratio: f64,
    /// Number of consecutive successes before relaxing adaptive ceiling.
    #[serde(default = "TimeoutsConfig::default_success_streak")]
    pub adaptive_success_streak: u32,
    /// Minimum timeout floor in milliseconds when applying adaptive clamps.
    #[serde(default = "TimeoutsConfig::default_min_floor_ms")]
    pub adaptive_min_floor_ms: u64,
    /// Timeout (in seconds) for the connect phase of outbound HTTP clients.
    /// `0` disables the limit. Also settable via `VTCODE_HTTP_CONNECT_TIMEOUT_SECS`.
    #[serde(default = "TimeoutsConfig::default_http_connect_seconds")]
    pub http_connect_seconds: u64,
    /// Timeout (in seconds) for a whole outbound HTTP request, including body.
    /// `0` disables the limit. Also settable via `VTCODE_HTTP_REQUEST_TIMEOUT_SECS`.
    #[serde(default = "TimeoutsConfig::default_http_request_seconds")]
    pub http_request_seconds: u64,
    /// Timeout (in seconds) for large HTTP transfers such as release downloads.
    /// `0` disables the limit. Also settable via `VTCODE_HTTP_DOWNLOAD_TIMEOUT_SECS`.
    #[serde(default = "TimeoutsConfig::default_http_download_seconds")]
    pub http_download_seconds: u64,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            default_ceiling_seconds: Self::default_default_ceiling_seconds(),
            pty_ceiling_seconds: Self::default_pty_ceiling_seconds(),
            mcp_ceiling_seconds: Self::default_mcp_ceiling_seconds(),
            streaming_ceiling_seconds: Self::default_streaming_ceiling_seconds(),
            long_running_command_ceiling_seconds: Self::default_long_running_command_ceiling_seconds(),
            warning_threshold_percent: Self::default_warning_threshold_percent(),
            adaptive_decay_ratio: Self::default_decay_ratio(),
            adaptive_success_streak: Self::default_success_streak(),
            adaptive_min_floor_ms: Self::default_min_floor_ms(),
            http_connect_seconds: Self::default_http_connect_seconds(),
            http_request_seconds: Self::default_http_request_seconds(),
            http_download_seconds: Self::default_http_download_seconds(),
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

    const fn default_streaming_ceiling_seconds() -> u64 {
        600
    }

    const fn default_long_running_command_ceiling_seconds() -> u64 {
        3_600
    }

    const fn default_warning_threshold_percent() -> u8 {
        80
    }

    const fn default_decay_ratio() -> f64 {
        0.875
    }

    const fn default_success_streak() -> u32 {
        5
    }

    const fn default_min_floor_ms() -> u64 {
        1_000
    }

    // The HTTP keys are the escape hatch for the "my connection is slower than
    // the author's" case, so they also honor an environment variable. Matching
    // `VTCODE_FULLSCREEN_SCROLL_SPEED`, the env var applies only when the TOML
    // key is absent, which keeps `vtcode.toml` authoritative when both are set.
    fn default_http_connect_seconds() -> u64 {
        env_seconds(crate::constants::env_vars::VTCODE_HTTP_CONNECT_TIMEOUT_SECS).unwrap_or(5)
    }

    fn default_http_request_seconds() -> u64 {
        env_seconds(crate::constants::env_vars::VTCODE_HTTP_REQUEST_TIMEOUT_SECS).unwrap_or(30)
    }

    fn default_http_download_seconds() -> u64 {
        env_seconds(crate::constants::env_vars::VTCODE_HTTP_DOWNLOAD_TIMEOUT_SECS).unwrap_or(300)
    }

    /// Convert the configured threshold into a fraction (0.0-1.0).
    pub fn warning_threshold_fraction(&self) -> f32 {
        f32::from(self.warning_threshold_percent) / 100.0
    }

    /// Resolve the outbound HTTP timeout defaults for
    /// [`vtcode_commons::http::install_http_timeouts`].
    pub fn http_timeouts(&self) -> vtcode_commons::http::HttpTimeouts {
        vtcode_commons::http::HttpTimeouts::from_seconds(
            self.http_connect_seconds,
            self.http_request_seconds,
            self.http_download_seconds,
        )
    }

    /// Normalize a ceiling value into an optional duration.
    pub fn ceiling_duration(&self, seconds: u64) -> Option<std::time::Duration> {
        if seconds == 0 {
            None
        } else {
            Some(std::time::Duration::from_secs(seconds))
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.warning_threshold_percent > 0 && self.warning_threshold_percent < 100,
            "timeouts.warning_threshold_percent must be between 1 and 99",
        );

        ensure!(
            (0.1..=1.0).contains(&self.adaptive_decay_ratio),
            "timeouts.adaptive_decay_ratio must be between 0.1 and 1.0"
        );
        ensure!(self.adaptive_success_streak > 0, "timeouts.adaptive_success_streak must be at least 1");
        ensure!(self.adaptive_min_floor_ms >= 100, "timeouts.adaptive_min_floor_ms must be at least 100ms");

        ensure!(
            self.default_ceiling_seconds == 0 || self.default_ceiling_seconds >= Self::MIN_CEILING_SECONDS,
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

        ensure!(
            self.streaming_ceiling_seconds == 0 || self.streaming_ceiling_seconds >= Self::MIN_CEILING_SECONDS,
            "timeouts.streaming_ceiling_seconds must be at least {} seconds (or 0 to disable)",
            Self::MIN_CEILING_SECONDS
        );

        ensure!(
            self.long_running_command_ceiling_seconds == 0
                || self.long_running_command_ceiling_seconds >= Self::MIN_CEILING_SECONDS,
            "timeouts.long_running_command_ceiling_seconds must be at least {} seconds (or 0 to disable)",
            Self::MIN_CEILING_SECONDS
        );

        for (name, seconds) in [
            ("http_connect_seconds", self.http_connect_seconds),
            ("http_request_seconds", self.http_request_seconds),
            ("http_download_seconds", self.http_download_seconds),
        ] {
            ensure!(
                seconds == 0 || seconds >= MIN_HTTP_TIMEOUT_SECONDS,
                "timeouts.{name} must be at least {MIN_HTTP_TIMEOUT_SECONDS} seconds (or 0 to disable)",
            );
        }

        Ok(())
    }
}

/// Parse a `VTCODE_*_TIMEOUT_SECS`-style environment override.
///
/// Empty, non-numeric, and zero values are ignored so a stray `=0` in a CI
/// environment cannot silently disable every HTTP deadline.
fn env_seconds(name: &str) -> Option<u64> {
    crate::env_helpers::read_env_var(name)
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
}

/// Resolve a user-supplied timeout into a bounded, non-zero value.
pub fn resolve_timeout(user_timeout: Option<u64>) -> u64 {
    use crate::constants::execution::{DEFAULT_TIMEOUT_SECS, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS};

    match user_timeout {
        None | Some(0) => DEFAULT_TIMEOUT_SECS,
        Some(value) if value < MIN_TIMEOUT_SECS => MIN_TIMEOUT_SECS,
        Some(value) if value > MAX_TIMEOUT_SECS => MAX_TIMEOUT_SECS,
        Some(value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::MIN_HTTP_TIMEOUT_SECONDS;
    use super::TimeoutsConfig;
    use super::resolve_timeout;
    use crate::constants::execution::{DEFAULT_TIMEOUT_SECS, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS};

    #[test]
    fn default_values_are_safe() {
        let config = TimeoutsConfig::default();
        assert_eq!(config.default_ceiling_seconds, 180);
        assert_eq!(config.pty_ceiling_seconds, 300);
        assert_eq!(config.mcp_ceiling_seconds, 120);
        assert_eq!(config.streaming_ceiling_seconds, 600);
        assert_eq!(config.long_running_command_ceiling_seconds, 3_600);
        assert_eq!(config.warning_threshold_percent, 80);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn zero_ceiling_disables_limit() {
        let config = TimeoutsConfig { default_ceiling_seconds: 0, ..Default::default() };
        assert!(config.validate().is_ok());
        assert!(config.ceiling_duration(config.default_ceiling_seconds).is_none());
    }

    #[test]
    fn warning_threshold_bounds_are_enforced() {
        let config_low = TimeoutsConfig { warning_threshold_percent: 0, ..Default::default() };
        assert!(config_low.validate().is_err());

        let config_high = TimeoutsConfig {
            warning_threshold_percent: 100,
            ..Default::default()
        };
        assert!(config_high.validate().is_err());
    }

    #[test]
    fn resolve_timeout_applies_bounds() {
        assert_eq!(resolve_timeout(None), DEFAULT_TIMEOUT_SECS);
        assert_eq!(resolve_timeout(Some(0)), DEFAULT_TIMEOUT_SECS);
        assert_eq!(resolve_timeout(Some(1)), MIN_TIMEOUT_SECS);
        assert_eq!(resolve_timeout(Some(MAX_TIMEOUT_SECS + 1)), MAX_TIMEOUT_SECS);
        assert_eq!(resolve_timeout(Some(120)), 120);
    }

    #[test]
    fn http_timeout_defaults_match_commons_http_defaults() {
        let config = TimeoutsConfig::default();
        assert_eq!(config.http_connect_seconds, 5);
        assert_eq!(config.http_request_seconds, 30);
        assert_eq!(config.http_download_seconds, 300);

        let resolved = config.http_timeouts();
        let defaults = vtcode_commons::http::HttpTimeouts::default();
        assert_eq!(resolved, defaults);
    }

    #[test]
    fn http_timeouts_map_zero_to_disabled() {
        let config = TimeoutsConfig {
            http_connect_seconds: 0,
            http_request_seconds: 0,
            http_download_seconds: 0,
            ..Default::default()
        };
        assert!(config.validate().is_ok());

        let resolved = config.http_timeouts();
        assert_eq!(resolved.connect, None);
        assert_eq!(resolved.request, None);
        assert_eq!(resolved.download, None);
    }

    #[test]
    fn http_timeouts_reject_sub_second_typos_but_allow_minimum() {
        for seconds in 1..MIN_HTTP_TIMEOUT_SECONDS {
            let config = TimeoutsConfig {
                http_request_seconds: seconds,
                ..Default::default()
            };
            assert!(config.validate().is_err(), "{seconds}s must be rejected as a likely unit mistake");
        }

        let config = TimeoutsConfig {
            http_request_seconds: MIN_HTTP_TIMEOUT_SECONDS,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn http_env_overrides_back_the_defaults() {
        // `#[serde(default)]` resolves through the env-aware fns, so an unset
        // TOML key can still be steered from the environment.
        let defaults = TimeoutsConfig::default();
        assert_eq!(defaults.http_request_seconds, 30);

        crate::env_helpers::test_env_overrides::set("VTCODE_HTTP_REQUEST_TIMEOUT_SECS", Some("120"));
        crate::env_helpers::test_env_overrides::set("VTCODE_HTTP_CONNECT_TIMEOUT_SECS", Some("15"));
        // A zero override must not silently disable the deadline.
        crate::env_helpers::test_env_overrides::set("VTCODE_HTTP_DOWNLOAD_TIMEOUT_SECS", Some("0"));

        let overridden = TimeoutsConfig::default();
        crate::env_helpers::test_env_overrides::set("VTCODE_HTTP_REQUEST_TIMEOUT_SECS", None);
        crate::env_helpers::test_env_overrides::set("VTCODE_HTTP_CONNECT_TIMEOUT_SECS", None);
        crate::env_helpers::test_env_overrides::set("VTCODE_HTTP_DOWNLOAD_TIMEOUT_SECS", None);

        assert_eq!(overridden.http_request_seconds, 120);
        assert_eq!(overridden.http_connect_seconds, 15);
        assert_eq!(overridden.http_download_seconds, 300, "a zero env override is ignored");
    }
}
