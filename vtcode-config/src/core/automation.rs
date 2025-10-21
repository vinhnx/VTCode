use crate::constants::{defaults, tools};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Automation-specific configuration toggles.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AutomationConfig {
    /// Full-auto execution safeguards.
    #[serde(default)]
    pub full_auto: FullAutoConfig,
}

/// Controls for running the agent without interactive approvals.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FullAutoConfig {
    /// Enable the runtime flag once the workspace is configured for autonomous runs.
    #[serde(default = "default_full_auto_enabled")]
    pub enabled: bool,

    /// Maximum number of autonomous agent turns before the exec runner pauses.
    #[serde(default = "default_full_auto_max_turns")]
    pub max_turns: usize,

    /// Allow-list of tools that may execute automatically.
    #[serde(default = "default_full_auto_allowed_tools")]
    pub allowed_tools: Vec<String>,

    /// Require presence of a profile/acknowledgement file before activation.
    #[serde(default = "default_require_profile_ack")]
    pub require_profile_ack: bool,

    /// Optional path to a profile describing acceptable behaviors.
    #[serde(default)]
    pub profile_path: Option<PathBuf>,
}

impl Default for FullAutoConfig {
    fn default() -> Self {
        Self {
            enabled: default_full_auto_enabled(),
            max_turns: default_full_auto_max_turns(),
            allowed_tools: default_full_auto_allowed_tools(),
            require_profile_ack: default_require_profile_ack(),
            profile_path: None,
        }
    }
}

fn default_full_auto_enabled() -> bool {
    false
}

fn default_full_auto_allowed_tools() -> Vec<String> {
    vec![
        tools::READ_FILE.to_string(),
        tools::LIST_FILES.to_string(),
        tools::GREP_FILE.to_string(),
        tools::SIMPLE_SEARCH.to_string(),
    ]
}

fn default_require_profile_ack() -> bool {
    true
}

fn default_full_auto_max_turns() -> usize {
    defaults::DEFAULT_FULL_AUTO_MAX_TURNS
}
