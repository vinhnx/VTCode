use serde::{Deserialize, Serialize};

/// Configuration for a user-defined OpenAI-compatible provider endpoint.
///
/// Allows users to define multiple named custom endpoints (e.g., corporate
/// proxies) with distinct display names, so they can toggle between them
/// and clearly see which endpoint is active.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomProviderConfig {
    /// Stable provider key used for routing and persistence (e.g., "mycorp").
    /// Must be lowercase alphanumeric with optional hyphens/underscores.
    pub name: String,

    /// Human-friendly label shown in the TUI header, footer, and model picker
    /// (e.g., "MyCorporateName").
    pub display_name: String,

    /// Base URL of the OpenAI-compatible API endpoint
    /// (e.g., "https://llm.corp.example/v1").
    pub base_url: String,

    /// Environment variable name that holds the API key for this endpoint
    /// (e.g., "MYCORP_API_KEY").
    #[serde(default)]
    pub api_key_env: String,

    /// Default model to use with this endpoint (e.g., "gpt-5-mini").
    #[serde(default)]
    pub model: String,
}

impl CustomProviderConfig {
    /// Resolve the API key environment variable used for this provider.
    ///
    /// Falls back to a derived `NAME_API_KEY`-style variable when the config
    /// does not set `api_key_env`.
    pub fn resolved_api_key_env(&self) -> String {
        if !self.api_key_env.trim().is_empty() {
            return self.api_key_env.clone();
        }

        let mut key = String::new();
        for ch in self.name.chars() {
            if ch.is_ascii_alphanumeric() {
                key.push(ch.to_ascii_uppercase());
            } else if !key.ends_with('_') {
                key.push('_');
            }
        }
        if !key.ends_with("_API_KEY") {
            if !key.ends_with('_') {
                key.push('_');
            }
            key.push_str("API_KEY");
        }
        key
    }

    /// Validate that required fields are present and the name doesn't collide
    /// with built-in provider keys.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("custom_providers: `name` must not be empty".to_string());
        }

        if !is_valid_provider_name(&self.name) {
            return Err(format!(
                "custom_providers[{}]: `name` must use lowercase letters, digits, hyphens, or underscores",
                self.name
            ));
        }

        if self.display_name.trim().is_empty() {
            return Err(format!(
                "custom_providers[{}]: `display_name` must not be empty",
                self.name
            ));
        }

        if self.base_url.trim().is_empty() {
            return Err(format!(
                "custom_providers[{}]: `base_url` must not be empty",
                self.name
            ));
        }

        let reserved = [
            "openai",
            "anthropic",
            "gemini",
            "copilot",
            "deepseek",
            "openrouter",
            "ollama",
            "lmstudio",
            "moonshot",
            "zai",
            "minimax",
            "huggingface",
            "openresponses",
        ];
        let lower = self.name.to_lowercase();
        if reserved.contains(&lower.as_str()) {
            return Err(format!(
                "custom_providers[{}]: name collides with built-in provider",
                self.name
            ));
        }

        Ok(())
    }
}

fn is_valid_provider_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    let Some(last) = bytes.last() else {
        return false;
    };

    let is_valid_char = |ch: u8| matches!(ch, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_');
    let is_alphanumeric = |ch: u8| matches!(ch, b'a'..=b'z' | b'0'..=b'9');

    is_alphanumeric(*first) && is_alphanumeric(*last) && bytes.iter().copied().all(is_valid_char)
}

#[cfg(test)]
mod tests {
    use super::CustomProviderConfig;

    #[test]
    fn validate_accepts_lowercase_provider_name() {
        let config = CustomProviderConfig {
            name: "mycorp".to_string(),
            display_name: "MyCorp".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            api_key_env: String::new(),
            model: "gpt-5-mini".to_string(),
        };

        assert!(config.validate().is_ok());
        assert_eq!(config.resolved_api_key_env(), "MYCORP_API_KEY");
    }

    #[test]
    fn validate_rejects_invalid_provider_name() {
        let config = CustomProviderConfig {
            name: "My Corp".to_string(),
            display_name: "My Corp".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            api_key_env: String::new(),
            model: "gpt-5-mini".to_string(),
        };

        let err = config.validate().expect_err("invalid name should fail");
        assert!(err.contains("must use lowercase letters, digits, hyphens, or underscores"));
    }
}
