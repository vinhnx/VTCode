use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn default_auth_timeout_ms() -> u64 {
    5_000
}

fn default_auth_refresh_interval_ms() -> u64 {
    300_000
}

/// Command-backed bearer token configuration for a custom provider.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CustomProviderCommandAuthConfig {
    /// Command to execute. Bare names are resolved via `PATH`.
    pub command: String,

    /// Optional command arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional working directory for the token command.
    #[serde(default)]
    pub cwd: Option<PathBuf>,

    /// Maximum time to wait for the command to complete successfully.
    #[serde(default = "default_auth_timeout_ms")]
    pub timeout_ms: u64,

    /// Maximum age for the cached token before rerunning the command.
    #[serde(default = "default_auth_refresh_interval_ms")]
    pub refresh_interval_ms: u64,
}

impl CustomProviderCommandAuthConfig {
    fn validate(&self, provider_name: &str) -> Result<(), String> {
        if self.command.trim().is_empty() {
            return Err(format!(
                "custom_providers[{provider_name}]: `auth.command` must not be empty"
            ));
        }

        if self.timeout_ms == 0 {
            return Err(format!(
                "custom_providers[{provider_name}]: `auth.timeout_ms` must be greater than 0"
            ));
        }

        if self.refresh_interval_ms == 0 {
            return Err(format!(
                "custom_providers[{provider_name}]: `auth.refresh_interval_ms` must be greater than 0"
            ));
        }

        Ok(())
    }
}

/// Configuration for a user-defined OpenAI-compatible provider endpoint.
///
/// Allows users to define multiple named custom endpoints (e.g., corporate
/// proxies) with distinct display names, so they can toggle between them
/// and clearly see which endpoint is active.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CustomProviderConfig {
    /// Stable provider key used for routing and persistence (e.g., "mycorp").
    /// Must be lowercase alphanumeric with optional hyphens/underscores.
    pub name: String,

    /// Human-friendly label shown in the TUI header, footer, and model picker
    /// (e.g., "MyCorporateName").
    pub display_name: String,

    /// Base URL of the OpenAI-compatible API endpoint
    /// (e.g., `<https://llm.corp.example/v1>`).
    pub base_url: String,

    /// Environment variable name that holds the API key for this endpoint
    /// (e.g., "MYCORP_API_KEY").
    #[serde(default)]
    pub api_key_env: String,

    /// Optional command-backed bearer token configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<CustomProviderCommandAuthConfig>,

    /// Default model to use with this endpoint (e.g., "gpt-5-mini").
    ///
    /// When [`models`](Self::models) is empty, this single model is what the
    /// `/model` picker offers for this provider. When [`models`](Self::models)
    /// is non-empty, this field is used as the default selection but the
    /// picker lists every entry in [`models`](Self::models).
    #[serde(default)]
    pub model: String,

    /// Optional list of additional model identifiers offered by the provider.
    ///
    /// Useful for OpenAI-compatible aggregators such as Atlas Cloud that
    /// expose many models behind a single endpoint. When set, the `/model`
    /// picker shows one entry per model. When empty, the picker falls back to
    /// the single [`model`](Self::model) field.
    #[serde(default)]
    pub models: Vec<String>,
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

    pub fn uses_command_auth(&self) -> bool {
        self.auth.is_some()
    }

    /// Return the list of models the `/model` picker should offer for this
    /// provider.
    ///
    /// If `models` is non-empty, every entry is returned (trimmed). Otherwise
    /// the single `model` field is returned as a one-element list. An empty
    /// `model` field with no `models` list yields an empty result.
    pub fn effective_models(&self) -> Vec<String> {
        if !self.models.is_empty() {
            return self
                .models
                .iter()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect();
        }
        let trimmed = self.model.trim();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_string()]
        }
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

        if let Some(auth) = &self.auth {
            auth.validate(&self.name)?;
            if !self.api_key_env.trim().is_empty() {
                return Err(format!(
                    "custom_providers[{}]: `auth` cannot be combined with `api_key_env`",
                    self.name
                ));
            }
        }

        if self.models.iter().any(|m| m.trim().is_empty()) {
            return Err(format!(
                "custom_providers[{}]: `models` entries must not be empty",
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
    use std::path::PathBuf;

    use super::{
        CustomProviderCommandAuthConfig, CustomProviderConfig, default_auth_refresh_interval_ms,
        default_auth_timeout_ms,
    };

    #[test]
    fn validate_accepts_lowercase_provider_name() {
        let config = CustomProviderConfig {
            name: "mycorp".to_string(),
            display_name: "MyCorp".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            api_key_env: String::new(),
            auth: None,
            model: "gpt-5-mini".to_string(),
            models: Vec::new(),
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
            auth: None,
            model: "gpt-5-mini".to_string(),
            models: Vec::new(),
        };

        let err = config.validate().expect_err("invalid name should fail");
        assert!(err.contains("must use lowercase letters, digits, hyphens, or underscores"));
    }

    #[test]
    fn validate_rejects_auth_and_api_key_env_together() {
        let config = CustomProviderConfig {
            name: "mycorp".to_string(),
            display_name: "MyCorp".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            api_key_env: "MYCORP_API_KEY".to_string(),
            auth: Some(CustomProviderCommandAuthConfig {
                command: "print-token".to_string(),
                args: Vec::new(),
                cwd: None,
                timeout_ms: default_auth_timeout_ms(),
                refresh_interval_ms: default_auth_refresh_interval_ms(),
            }),
            model: "gpt-5-mini".to_string(),
            models: Vec::new(),
        };

        let err = config.validate().expect_err("conflicting auth should fail");
        assert!(err.contains("`auth` cannot be combined with `api_key_env`"));
    }

    #[test]
    fn validate_accepts_command_auth_without_static_env_key() {
        let config = CustomProviderConfig {
            name: "mycorp".to_string(),
            display_name: "MyCorp".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            api_key_env: String::new(),
            auth: Some(CustomProviderCommandAuthConfig {
                command: "print-token".to_string(),
                args: vec!["--json".to_string()],
                cwd: Some(PathBuf::from("/tmp")),
                timeout_ms: 1_000,
                refresh_interval_ms: 60_000,
            }),
            model: "gpt-5-mini".to_string(),
            models: Vec::new(),
        };

        assert!(config.validate().is_ok());
        assert!(config.uses_command_auth());
    }

    #[test]
    fn validate_rejects_empty_model_entry_in_models_list() {
        let config = CustomProviderConfig {
            name: "mycorp".to_string(),
            display_name: "MyCorp".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            api_key_env: "MYCORP_API_KEY".to_string(),
            auth: None,
            model: "gpt-5-mini".to_string(),
            models: vec!["valid-model".to_string(), "   ".to_string()],
        };

        let err = config
            .validate()
            .expect_err("blank models entry should fail");
        assert!(err.contains("`models` entries must not be empty"));
    }

    #[test]
    fn effective_models_uses_models_list_when_present() {
        let config = CustomProviderConfig {
            name: "atlascloud".to_string(),
            display_name: "Atlas Cloud".to_string(),
            base_url: "https://api.atlascloud.ai/v1".to_string(),
            api_key_env: "ATLASCLOUD_API_KEY".to_string(),
            auth: None,
            model: "deepseek-ai/deepseek-v4-flash".to_string(),
            models: vec![
                "deepseek-ai/deepseek-v4-flash".to_string(),
                "deepseek-ai/deepseek-v4-pro".to_string(),
                "deepseek-ai/DeepSeek-V3-0324".to_string(),
                "qwen/qwen3.6-35b-a3b".to_string(),
                "moonshotai/kimi-k2.6".to_string(),
            ],
        };

        assert_eq!(
            config.effective_models(),
            vec![
                "deepseek-ai/deepseek-v4-flash".to_string(),
                "deepseek-ai/deepseek-v4-pro".to_string(),
                "deepseek-ai/DeepSeek-V3-0324".to_string(),
                "qwen/qwen3.6-35b-a3b".to_string(),
                "moonshotai/kimi-k2.6".to_string(),
            ]
        );
    }

    #[test]
    fn effective_models_falls_back_to_single_model_field() {
        let config = CustomProviderConfig {
            model: "gpt-5-mini".to_string(),
            ..CustomProviderConfig::default()
        };

        assert_eq!(config.effective_models(), vec!["gpt-5-mini".to_string()]);
    }
}
