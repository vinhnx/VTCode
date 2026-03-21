use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

use crate::openrouter_oauth::OpenRouterOAuthConfig;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AuthConfig {
    #[serde(default)]
    pub openrouter: OpenRouterOAuthConfig,
    #[serde(default)]
    pub openai: OpenAIAuthConfig,
    #[serde(default)]
    pub copilot: CopilotAuthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default)]
pub struct CopilotAuthConfig {
    pub command: Option<String>,
    pub host: Option<String>,
    #[serde(default = "default_copilot_available_tools")]
    pub available_tools: Vec<String>,
    #[serde(default)]
    pub excluded_tools: Vec<String>,
    #[serde(default = "default_vtcode_tool_allowlist")]
    pub vtcode_tool_allowlist: Vec<String>,
    pub startup_timeout_secs: u64,
    pub auth_timeout_secs: u64,
}

impl Default for CopilotAuthConfig {
    fn default() -> Self {
        Self {
            command: None,
            host: None,
            available_tools: default_copilot_available_tools(),
            excluded_tools: Vec::new(),
            vtcode_tool_allowlist: default_vtcode_tool_allowlist(),
            startup_timeout_secs: 20,
            auth_timeout_secs: 300,
        }
    }
}

fn default_copilot_available_tools() -> Vec<String> {
    ["view", "glob", "grep"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn default_vtcode_tool_allowlist() -> Vec<String> {
    [
        "unified_search",
        "unified_file",
        "unified_exec",
        "apply_patch",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OpenAIPreferredMethod {
    #[default]
    Auto,
    ApiKey,
    Chatgpt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default)]
pub struct OpenAIAuthConfig {
    pub preferred_method: OpenAIPreferredMethod,
    pub callback_port: u16,
    pub auto_refresh: bool,
    pub flow_timeout_secs: u64,
}

impl Default for OpenAIAuthConfig {
    fn default() -> Self {
        Self {
            preferred_method: OpenAIPreferredMethod::Auto,
            callback_port: 1455,
            auto_refresh: true,
            flow_timeout_secs: 300,
        }
    }
}

impl OpenAIPreferredMethod {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ApiKey => "api_key",
            Self::Chatgpt => "chatgpt",
        }
    }
}

impl OpenAIAuthConfig {
    #[must_use]
    pub fn callback_url(&self) -> String {
        format!("http://localhost:{}/callback", self.callback_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_defaults_match_expected_values() {
        let config = OpenAIAuthConfig::default();
        assert_eq!(config.preferred_method, OpenAIPreferredMethod::Auto);
        assert_eq!(config.callback_port, 1455);
        assert!(config.auto_refresh);
        assert_eq!(config.flow_timeout_secs, 300);
    }

    #[test]
    fn copilot_defaults_match_expected_values() {
        let config = CopilotAuthConfig::default();
        assert!(config.command.is_none());
        assert!(config.host.is_none());
        assert_eq!(config.available_tools, vec!["view", "glob", "grep"]);
        assert!(config.excluded_tools.is_empty());
        assert_eq!(
            config.vtcode_tool_allowlist,
            vec![
                "unified_search",
                "unified_file",
                "unified_exec",
                "apply_patch"
            ]
        );
        assert_eq!(config.startup_timeout_secs, 20);
        assert_eq!(config.auth_timeout_secs, 300);
    }
}
