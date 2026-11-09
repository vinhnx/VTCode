//! Desktop app configuration
//!
//! Settings for the optional native macOS desktop app (VT Code.app), which
//! shares the CLI's providers, configuration, and sessions.

use serde::{Deserialize, Serialize};

/// Desktop app configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct DesktopConfig {
    /// Suggest installing the desktop app at startup when it is missing.
    /// Shown at most once per day; set to `false` to mute permanently.
    #[serde(default = "default_suggest_install")]
    pub suggest_install: bool,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self { suggest_install: default_suggest_install() }
    }
}

fn default_suggest_install() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_install_defaults_on() {
        assert!(DesktopConfig::default().suggest_install);
        // Missing key in a user file must also default on (opt-out).
        let parsed: DesktopConfig = toml::from_str("").expect("empty table parses");
        assert!(parsed.suggest_install);
    }

    #[test]
    fn suggest_install_opt_out_parses() {
        let parsed: DesktopConfig = toml::from_str("suggest-install = false").expect("opt-out parses");
        assert!(!parsed.suggest_install);
    }
}
