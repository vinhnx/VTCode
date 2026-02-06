//! Agent teams configuration schema and parsing.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentTeamsConfig {
    /// Enable agent teams (experimental)
    #[serde(default)]
    pub enabled: bool,

    /// Maximum number of teammates in a team
    #[serde(default = "default_max_teammates")]
    pub max_teammates: usize,

    /// Default model for agent team subagents
    #[serde(default)]
    pub default_model: Option<String>,
}

impl Default for AgentTeamsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_teammates: default_max_teammates(),
            default_model: None,
        }
    }
}

fn default_max_teammates() -> usize {
    4
}

#[cfg(test)]
mod tests {
    use super::AgentTeamsConfig;

    #[test]
    fn defaults() {
        let config = AgentTeamsConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_teammates, 4);
    }
}
