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
}
