//! Runtime color output policy helpers.
//!
//! This module centralizes color enable/disable decisions for CLI and
//! transcript-style output paths. By default it follows the NO_COLOR
//! environment variable with strict "present and non-empty" semantics.

use once_cell::sync::Lazy;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Source that determined the active runtime color policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorOutputPolicySource {
    /// Default runtime behavior (auto detect + env hints).
    DefaultAuto,
    /// Disabled due to NO_COLOR environment variable.
    NoColorEnv,
    /// Disabled due to explicit `--no-color`.
    CliNoColor,
    /// Disabled due to explicit `--color never`.
    CliColorNever,
    /// Enabled due to explicit `--color always`.
    CliColorAlways,
    /// Enabled or disabled by explicit config override.
    ConfigOverride,
}

/// Runtime color output policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorOutputPolicy {
    pub enabled: bool,
    pub source: ColorOutputPolicySource,
}

const SOURCE_DEFAULT_AUTO: u8 = 0;
const SOURCE_NO_COLOR_ENV: u8 = 1;
const SOURCE_CLI_NO_COLOR: u8 = 2;
const SOURCE_CLI_COLOR_NEVER: u8 = 3;
const SOURCE_CLI_COLOR_ALWAYS: u8 = 4;
const SOURCE_CONFIG_OVERRIDE: u8 = 5;

static POLICY_ENABLED: AtomicBool = AtomicBool::new(true);
static POLICY_SOURCE: AtomicU8 = AtomicU8::new(SOURCE_DEFAULT_AUTO);

static INIT_FROM_ENV: Lazy<()> = Lazy::new(|| {
    let default_policy = detect_policy_from_env();
    set_color_output_policy(default_policy);
});

fn detect_policy_from_env() -> ColorOutputPolicy {
    if no_color_env_active() {
        ColorOutputPolicy {
            enabled: false,
            source: ColorOutputPolicySource::NoColorEnv,
        }
    } else {
        ColorOutputPolicy {
            enabled: true,
            source: ColorOutputPolicySource::DefaultAuto,
        }
    }
}

fn encode_source(source: ColorOutputPolicySource) -> u8 {
    match source {
        ColorOutputPolicySource::DefaultAuto => SOURCE_DEFAULT_AUTO,
        ColorOutputPolicySource::NoColorEnv => SOURCE_NO_COLOR_ENV,
        ColorOutputPolicySource::CliNoColor => SOURCE_CLI_NO_COLOR,
        ColorOutputPolicySource::CliColorNever => SOURCE_CLI_COLOR_NEVER,
        ColorOutputPolicySource::CliColorAlways => SOURCE_CLI_COLOR_ALWAYS,
        ColorOutputPolicySource::ConfigOverride => SOURCE_CONFIG_OVERRIDE,
    }
}

fn decode_source(value: u8) -> ColorOutputPolicySource {
    match value {
        SOURCE_NO_COLOR_ENV => ColorOutputPolicySource::NoColorEnv,
        SOURCE_CLI_NO_COLOR => ColorOutputPolicySource::CliNoColor,
        SOURCE_CLI_COLOR_NEVER => ColorOutputPolicySource::CliColorNever,
        SOURCE_CLI_COLOR_ALWAYS => ColorOutputPolicySource::CliColorAlways,
        SOURCE_CONFIG_OVERRIDE => ColorOutputPolicySource::ConfigOverride,
        _ => ColorOutputPolicySource::DefaultAuto,
    }
}

fn no_color_env_active_from(value: Option<OsString>) -> bool {
    value.map(|v| !v.is_empty()).unwrap_or(false)
}

/// Returns true when NO_COLOR is present and non-empty.
pub fn no_color_env_active() -> bool {
    no_color_env_active_from(std::env::var_os("NO_COLOR"))
}

/// Read the current runtime color policy.
pub fn current_color_output_policy() -> ColorOutputPolicy {
    Lazy::force(&INIT_FROM_ENV);
    ColorOutputPolicy {
        enabled: POLICY_ENABLED.load(Ordering::Relaxed),
        source: decode_source(POLICY_SOURCE.load(Ordering::Relaxed)),
    }
}

/// Replace the current runtime color policy.
pub fn set_color_output_policy(policy: ColorOutputPolicy) {
    POLICY_ENABLED.store(policy.enabled, Ordering::Relaxed);
    POLICY_SOURCE.store(encode_source(policy.source), Ordering::Relaxed);
}

/// Reset runtime color policy from environment defaults.
pub fn reset_color_output_policy_from_env() {
    set_color_output_policy(detect_policy_from_env());
}

/// Returns true when runtime color output is enabled.
pub fn color_output_enabled() -> bool {
    current_color_output_policy().enabled
}

#[cfg(test)]
mod tests {
    use super::no_color_env_active_from;
    use std::ffi::OsString;

    #[test]
    fn no_color_requires_non_empty_value() {
        assert!(!no_color_env_active_from(None));
        assert!(!no_color_env_active_from(Some(OsString::from(""))));
        assert!(no_color_env_active_from(Some(OsString::from("1"))));
    }
}
