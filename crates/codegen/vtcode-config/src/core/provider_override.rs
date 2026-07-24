use serde::{Deserialize, Serialize};

/// Configuration for overriding a built-in provider's model list.
///
/// Allows users to extend built-in providers (e.g., `opencode-zen`,
/// `opencode-go`) with additional custom models, and optionally override
/// the provider's base URL or API key environment variable.
///
/// # Example
///
/// ```toml
/// [providers.opencode-zen]
/// models = [
///     "opencode/gpt-5.4",
///     "opencode/gpt-5.4-mini",
///     "my-custom-model",
/// ]
/// base_url = "https://custom-endpoint.example.com"
/// api_key_env = "MY_CUSTOM_KEY"
/// ```
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderOverrideConfig {
    /// Additional model identifiers to offer for this built-in provider.
    ///
    /// These models are appended to the provider's hardcoded model list
    /// and appear in the `/model` picker alongside built-in entries.
    #[serde(default)]
    pub models: Vec<String>,

    /// Optional base URL override for the provider endpoint.
    ///
    /// When set, custom models from this override are routed to the
    /// specified endpoint instead of the provider's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Optional environment variable name for the API key.
    ///
    /// When set, overrides the provider's default API key environment
    /// variable for models from this override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

impl ProviderOverrideConfig {
    /// Validate that all model entries are non-empty after trimming and
    /// that there are no duplicate model entries.
    pub(crate) fn validate(&self, provider_name: &str) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for model in &self.models {
            let trimmed = model.trim();
            if trimmed.is_empty() {
                return Err(format!("providers[{provider_name}]: `models` entries must not be empty"));
            }
            if !seen.insert(trimmed.to_lowercase()) {
                return Err(format!("providers[{provider_name}]: duplicate model `{trimmed}`"));
            }
        }
        if let Some(base_url) = &self.base_url
            && base_url.trim().is_empty()
        {
            return Err(format!("providers[{provider_name}]: `base_url` must not be empty"));
        }
        if let Some(api_key_env) = &self.api_key_env
            && api_key_env.trim().is_empty()
        {
            return Err(format!("providers[{provider_name}]: `api_key_env` must not be empty"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderOverrideConfig;

    #[test]
    fn default_config_is_empty() {
        let config = ProviderOverrideConfig::default();
        assert!(config.models.is_empty());
        assert!(config.base_url.is_none());
        assert!(config.api_key_env.is_none());
    }

    #[test]
    fn validate_accepts_valid_config() {
        let config = ProviderOverrideConfig {
            models: vec!["model-a".to_string(), "model-b".to_string()],
            base_url: Some("https://example.com".to_string()),
            api_key_env: Some("MY_KEY".to_string()),
        };
        assert!(config.validate("test-provider").is_ok());
    }

    #[test]
    fn validate_rejects_empty_model_entry() {
        let config = ProviderOverrideConfig {
            models: vec!["model-a".to_string(), "   ".to_string()],
            base_url: None,
            api_key_env: None,
        };
        let err = config.validate("test-provider").expect_err("blank model should fail");
        assert!(err.contains("`models` entries must not be empty"));
    }

    #[test]
    fn validate_rejects_empty_base_url() {
        let config = ProviderOverrideConfig {
            models: vec!["model-a".to_string()],
            base_url: Some("   ".to_string()),
            api_key_env: None,
        };
        let err = config.validate("test-provider").expect_err("blank base_url should fail");
        assert!(err.contains("`base_url` must not be empty"));
    }

    #[test]
    fn validate_rejects_empty_api_key_env() {
        let config = ProviderOverrideConfig {
            models: vec!["model-a".to_string()],
            base_url: None,
            api_key_env: Some("   ".to_string()),
        };
        let err = config.validate("test-provider").expect_err("blank api_key_env should fail");
        assert!(err.contains("`api_key_env` must not be empty"));
    }

    #[test]
    fn validate_rejects_duplicate_models() {
        let config = ProviderOverrideConfig {
            models: vec!["model-a".to_string(), "model-b".to_string(), "model-a".to_string()],
            base_url: None,
            api_key_env: None,
        };
        let err = config.validate("test-provider").expect_err("duplicate model should fail");
        assert!(err.contains("duplicate model"));
    }

    #[test]
    fn validate_rejects_duplicate_models_case_insensitive() {
        let config = ProviderOverrideConfig {
            models: vec!["Model-A".to_string(), "model-a".to_string()],
            base_url: None,
            api_key_env: None,
        };
        let err = config
            .validate("test-provider")
            .expect_err("case-insensitive duplicate should fail");
        assert!(err.contains("duplicate model"));
    }
}
