use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

const DEFAULT_IMAGE: &str = "vtcode-computer-use-browser:1.64.1";
const DEFAULT_TURN_TIMEOUT_SECS: u64 = 300;
const DEFAULT_MAX_ACTIONS_PER_TURN: usize = 50;

/// Configuration for the opt-in computer-use runtime.
///
/// Approval stays per turn and per target regardless of these settings: the
/// run loop requests human review through the permission flow before any
/// control sequence, and automation policies cannot bypass it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ComputerUseConfig {
    /// Opt-in marker. The feature stays inert unless this is true.
    #[serde(default)]
    pub enabled: bool,
    /// Approved static build served to the browser target. Required before
    /// turns can run, but not before the switch can persist: Settings
    /// enables first and guides to the target second, and turns fail
    /// closed until a target is set. Must be absolute when present.
    /// Existence is verified at launch, not here.
    #[serde(default)]
    pub browser_build: Option<PathBuf>,
    /// Pinned browser container image (tag or digest, never `latest`).
    #[serde(default = "default_image")]
    pub image: String,
    /// Wall-clock budget for one approved turn, in seconds.
    #[serde(default = "default_turn_timeout_secs")]
    pub turn_timeout_secs: u64,
    /// Maximum actions executed in one approved turn.
    #[serde(default = "default_max_actions_per_turn")]
    pub max_actions_per_turn: usize,
    /// Ask OpenAI Decisions to review each proposed batch for prompt
    /// injection and task drift before execution. Only OpenAI providers
    /// whose route supports Decisions are reviewed; anything else
    /// proceeds on the human turn approval alone. Uses OpenAI API billing.
    #[serde(default)]
    pub decisions_review: bool,
}

impl Default for ComputerUseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            browser_build: None,
            image: default_image(),
            turn_timeout_secs: default_turn_timeout_secs(),
            max_actions_per_turn: default_max_actions_per_turn(),
            decisions_review: false,
        }
    }
}

impl ComputerUseConfig {
    /// Validate shapes and bounds without touching the filesystem.
    pub fn validate(&self) -> Result<()> {
        // Image pinning mirrors the container launcher
        // (`vtcode_computer_use::browser::container`), which re-validates at
        // launch and stays authoritative; this fails fast on config errors.
        if self.image.is_empty()
            || self.image.len() > 512
            || self.image.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            bail!("computer_use.image must be a non-empty image reference without whitespace");
        }
        if !has_image_pin(&self.image) || self.image.ends_with(":latest") {
            bail!("computer_use.image must be pinned with a tag or digest other than latest");
        }
        // Enabled without a target is a legitimate intermediate state (the
        // Settings switch persists first; turns fail closed until a target
        // is set), so only the shape of a present value is validated here.
        if let Some(build) = self.browser_build.as_ref()
            && (!build.is_absolute() || build.as_os_str().is_empty())
        {
            bail!("computer_use.browser_build must be an absolute directory");
        }
        if self.turn_timeout_secs == 0 || self.turn_timeout_secs > 3600 {
            bail!("computer_use.turn_timeout_secs must be between 1 and 3600");
        }
        if self.max_actions_per_turn == 0 || self.max_actions_per_turn > 1000 {
            bail!("computer_use.max_actions_per_turn must be between 1 and 1000");
        }
        Ok(())
    }
}

fn default_image() -> String {
    DEFAULT_IMAGE.to_string()
}

/// Whether the reference carries a tag or digest. A registry port colon
/// (`registry:5000/foo`) is not a tag: only a colon after the last slash
/// counts. Mirrors the launcher check in
/// `vtcode_computer_use::browser::container`.
fn has_image_pin(image: &str) -> bool {
    if image.contains('@') {
        return true;
    }
    match image.rsplit_once('/') {
        Some((_, last)) => last.contains(':'),
        None => image.contains(':'),
    }
}

const fn default_turn_timeout_secs() -> u64 {
    DEFAULT_TURN_TIMEOUT_SECS
}

const fn default_max_actions_per_turn() -> usize {
    DEFAULT_MAX_ACTIONS_PER_TURN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_computer_use_disabled() {
        let config = ComputerUseConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.browser_build, None);
        assert_eq!(config.image, "vtcode-computer-use-browser:1.64.1");
        assert_eq!(config.turn_timeout_secs, 300);
        assert_eq!(config.max_actions_per_turn, 50);
        assert!(!config.decisions_review);
        config.validate().expect("defaults should validate");
    }

    #[test]
    fn parses_decisions_review_flag() {
        let config: ComputerUseConfig = toml::from_str("decisions_review = true\n").expect("computer use config");
        assert!(config.decisions_review);
        let serialized = toml::to_string(&config).expect("serialize computer use");
        assert!(serialized.contains("decisions_review = true"));
        let roundtrip: ComputerUseConfig = toml::from_str(&serialized).expect("roundtrip computer use");
        assert!(roundtrip.decisions_review);
    }

    #[test]
    fn enabled_without_a_target_persists_and_relative_targets_reject() {
        // The Settings switch persists first; turns fail closed until a
        // target is set, so validation must not refuse the switch.
        let config = ComputerUseConfig { enabled: true, ..Default::default() };
        config.validate().expect("enabled without a target should validate");
        for enabled in [false, true] {
            let config = ComputerUseConfig {
                enabled,
                browser_build: Some(PathBuf::from("relative/build")),
                ..Default::default()
            };
            assert!(config.validate().is_err(), "enabled={enabled} accepted a relative build");
            let config = ComputerUseConfig {
                enabled,
                browser_build: Some(PathBuf::from("/absolute/build")),
                ..Default::default()
            };
            config.validate().expect("absolute build should validate");
        }
    }

    #[test]
    fn image_must_be_pinned() {
        for image in [
            "",
            "runner",
            "runner:latest",
            "run ner:1.0",
            "registry.invalid/runner:1.64\n1",
            "registry.invalid:5000/runner",
        ] {
            let config = ComputerUseConfig { image: image.to_string(), ..Default::default() };
            assert!(config.validate().is_err(), "accepted {image:?}");
        }
        for image in [
            "registry.invalid/runner:1.64.1",
            "registry.invalid/runner@sha256:abc",
            "registry.invalid:5000/runner:1.64.1",
        ] {
            let config = ComputerUseConfig { image: image.to_string(), ..Default::default() };
            assert!(config.validate().is_ok(), "rejected {image:?}");
        }
    }

    #[test]
    fn budgets_have_sane_bounds() {
        for (timeout, actions, valid) in [
            (0, 50, false),
            (1, 50, true),
            (3600, 50, true),
            (3601, 50, false),
            (300, 0, false),
            (300, 1, true),
            (300, 1000, true),
            (300, 1001, false),
        ] {
            let config = ComputerUseConfig {
                turn_timeout_secs: timeout,
                max_actions_per_turn: actions,
                ..Default::default()
            };
            assert_eq!(config.validate().is_ok(), valid, "timeout={timeout} actions={actions}");
        }
    }
}
