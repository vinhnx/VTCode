//! Reasoning effort level definitions shared across VT Code crates.
//!
//! This module provides the [`ReasoningEffortLevel`] enum and associated
//! constants used for configuring model reasoning depth. These types live
//! in `vtcode-commons` so that both `vtcode-config` and `vtcode-llm`
//! can reference them without circular dependencies.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

/// Reasoning effort level string constants.
pub mod constants {
    pub const NONE: &str = "none";
    pub const MINIMAL: &str = "minimal";
    pub const LOW: &str = "low";
    pub const MEDIUM: &str = "medium";
    pub const HIGH: &str = "high";
    pub const XHIGH: &str = "xhigh";
    pub const MAX: &str = "max";
    /// Every value [`super::ReasoningEffortLevel::parse`] accepts, in
    /// ascending order. This is the single source list: tool schemas that
    /// accept a reasoning effort override use it verbatim.
    pub const PARSEABLE_LEVELS: &[&str] = &[NONE, MINIMAL, LOW, MEDIUM, HIGH, XHIGH, MAX];
    /// Effort levels offered for configuration and selection: every
    /// [`PARSEABLE_LEVELS`] entry except the leading `none`, which means
    /// "send no reasoning configuration" rather than an effort level.
    pub const ALLOWED_LEVELS: &[&str] = match PARSEABLE_LEVELS.split_first() {
        Some((_none, levels)) => levels,
        None => &[],
    };
    pub const LABEL_LOW: &str = "Low";
    pub const LABEL_MEDIUM: &str = "Medium";
    pub const LABEL_HIGH: &str = "High";
    pub const DESCRIPTION_LOW: &str = "Fast responses with lightweight reasoning.";
    pub const DESCRIPTION_MEDIUM: &str = "Balanced depth and speed. (Note: Mapped to high on some models)";
    pub const DESCRIPTION_HIGH: &str = "Deep reasoning for complex problems.";
}

/// Supported reasoning effort levels configured via vtcode.toml
/// These map to different provider-specific parameters:
/// - For Gemini 3 Pro: Maps to thinking_level (low, high) - medium coming soon
/// - For other models: Maps to provider-specific reasoning parameters
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ReasoningEffortLevel {
    /// No reasoning configuration - for models that don't support configurable reasoning
    None,
    /// Minimal reasoning effort - maps to low thinking level for Gemini 3 Pro
    Minimal,
    /// Low reasoning effort - maps to low thinking level for Gemini 3 Pro
    Low,
    /// Medium reasoning effort - Note: Not fully available for Gemini 3 Pro yet, defaults to high
    #[default]
    Medium,
    /// High reasoning effort - maps to high thinking level for Gemini 3 Pro
    High,
    /// Extra high reasoning effort - for GPT-5.6/6-Astra, Claude adaptive,
    /// Grok-4.6+, and Muse Spark long-running tasks
    XHigh,
    /// Maximum reasoning effort - for GPT-5.6/6-Astra, Claude adaptive,
    /// DeepSeek-V4, Kimi-K3, and GLM-5.x; aliased elsewhere
    Max,
    /// Forward-compatible catch-all for unrecognized effort level values
    Unknown,
}

impl ReasoningEffortLevel {
    /// Return the textual representation expected by downstream APIs
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => constants::NONE,
            Self::Minimal => constants::MINIMAL,
            Self::Low => constants::LOW,
            Self::Medium => constants::MEDIUM,
            Self::High => constants::HIGH,
            Self::XHigh => constants::XHIGH,
            Self::Max => constants::MAX,
            Self::Unknown => "unknown",
        }
    }

    /// Attempt to parse an effort level from user configuration input
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case(constants::NONE) {
            Some(Self::None)
        } else if normalized.eq_ignore_ascii_case(constants::MINIMAL) {
            Some(Self::Minimal)
        } else if normalized.eq_ignore_ascii_case(constants::LOW) {
            Some(Self::Low)
        } else if normalized.eq_ignore_ascii_case(constants::MEDIUM) {
            Some(Self::Medium)
        } else if normalized.eq_ignore_ascii_case(constants::HIGH) {
            Some(Self::High)
        } else if normalized.eq_ignore_ascii_case(constants::XHIGH) {
            Some(Self::XHigh)
        } else if normalized.eq_ignore_ascii_case(constants::MAX) {
            Some(Self::Max)
        } else {
            None
        }
    }

    /// Enumerate the allowed configuration values for validation and messaging
    pub fn allowed_values() -> &'static [&'static str] {
        constants::ALLOWED_LEVELS
    }
}

impl fmt::Display for ReasoningEffortLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReasoningEffortLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if let Some(parsed) = Self::parse(&raw) {
            Ok(parsed)
        } else {
            Ok(Self::Unknown)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reasoning_effort_parse_and_allowed_values_include_max() {
        assert_eq!(ReasoningEffortLevel::parse("max"), Some(ReasoningEffortLevel::Max));
        assert_eq!(ReasoningEffortLevel::Max.as_str(), "max");
        assert!(ReasoningEffortLevel::allowed_values().contains(&"max"));
    }

    /// Every named level, with an exhaustive match so a new variant fails to
    /// compile here until it is added to the level lists.
    fn named_levels() -> Vec<ReasoningEffortLevel> {
        use ReasoningEffortLevel::*;
        let levels = vec![None, Minimal, Low, Medium, High, XHigh, Max];
        for level in &levels {
            match level {
                None | Minimal | Low | Medium | High | XHigh | Max | Unknown => {}
            }
        }
        levels
    }

    #[test]
    fn parseable_levels_match_the_parser_in_both_directions() {
        for value in constants::PARSEABLE_LEVELS {
            let parsed = ReasoningEffortLevel::parse(value).unwrap_or_else(|| panic!("{value} must parse"));
            assert_eq!(parsed.as_str(), *value);
            // The derived serde wire form must agree with `as_str()` too:
            // config and provider payloads round-trip through both paths.
            assert_eq!(
                serde_json::to_value(parsed).expect("serializes"),
                serde_json::json!(*value),
                "serde wire form drifted from as_str() for {parsed:?}"
            );
        }
        let named = named_levels();
        assert_eq!(named.len(), constants::PARSEABLE_LEVELS.len());
        for level in named {
            assert!(constants::PARSEABLE_LEVELS.contains(&level.as_str()), "{level} missing from PARSEABLE_LEVELS");
        }
        assert_eq!(ReasoningEffortLevel::parse("unknown"), None);
    }

    #[test]
    fn allowed_levels_are_parseable_levels_without_none() {
        assert_eq!(constants::PARSEABLE_LEVELS[0], constants::NONE);
        assert_eq!(constants::ALLOWED_LEVELS, &constants::PARSEABLE_LEVELS[1..]);
    }
}
