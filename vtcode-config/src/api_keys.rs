//! API key management module for secure retrieval from environment variables,
//! .env files, and configuration files.
//!
//! This module provides a unified interface for retrieving API keys for different providers,
//! prioritizing security by checking environment variables first, then .env files, and finally
//! falling back to configuration file values.

use anyhow::Result;
use std::env;
use std::str::FromStr;

use crate::auth::CustomApiKeyStorage;
use crate::models::Provider;

/// API key sources for different providers
#[derive(Debug, Clone)]
pub struct ApiKeySources {
    /// Gemini API key environment variable name
    pub gemini_env: String,
    /// Anthropic API key environment variable name
    pub anthropic_env: String,
    /// OpenAI API key environment variable name
    pub openai_env: String,
    /// OpenRouter API key environment variable name
    pub openrouter_env: String,
    /// DeepSeek API key environment variable name
    pub deepseek_env: String,
    /// Z.AI API key environment variable name
    pub zai_env: String,
    /// Ollama API key environment variable name
    pub ollama_env: String,
    /// LM Studio API key environment variable name
    pub lmstudio_env: String,
    /// Gemini API key from configuration file
    pub gemini_config: Option<String>,
    /// Anthropic API key from configuration file
    pub anthropic_config: Option<String>,
    /// OpenAI API key from configuration file
    pub openai_config: Option<String>,
    /// OpenRouter API key from configuration file
    pub openrouter_config: Option<String>,
    /// DeepSeek API key from configuration file
    pub deepseek_config: Option<String>,
    /// Z.AI API key from configuration file
    pub zai_config: Option<String>,
    /// Ollama API key from configuration file
    pub ollama_config: Option<String>,
    /// LM Studio API key from configuration file
    pub lmstudio_config: Option<String>,
}

impl Default for ApiKeySources {
    fn default() -> Self {
        Self {
            gemini_env: "GEMINI_API_KEY".to_string(),
            anthropic_env: "ANTHROPIC_API_KEY".to_string(),
            openai_env: "OPENAI_API_KEY".to_string(),
            openrouter_env: "OPENROUTER_API_KEY".to_string(),
            deepseek_env: "DEEPSEEK_API_KEY".to_string(),
            zai_env: "ZAI_API_KEY".to_string(),
            ollama_env: "OLLAMA_API_KEY".to_string(),
            lmstudio_env: "LMSTUDIO_API_KEY".to_string(),
            gemini_config: None,
            anthropic_config: None,
            openai_config: None,
            openrouter_config: None,
            deepseek_config: None,
            zai_config: None,
            ollama_config: None,
            lmstudio_config: None,
        }
    }
}

impl ApiKeySources {
    /// Create API key sources for a specific provider with automatic environment variable inference
    pub fn for_provider(_provider: &str) -> Self {
        Self::default()
    }
}

fn inferred_api_key_env(provider: &str) -> &'static str {
    Provider::from_str(provider)
        .map(|resolved| resolved.default_api_key_env())
        .unwrap_or("GEMINI_API_KEY")
}

#[cfg(test)]
mod test_env_overrides {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    static OVERRIDES: LazyLock<Mutex<HashMap<String, Option<String>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    pub(super) fn get(key: &str) -> Option<Option<String>> {
        OVERRIDES.lock().ok().and_then(|map| map.get(key).cloned())
    }

    pub(super) fn set(key: &str, value: Option<&str>) {
        if let Ok(mut map) = OVERRIDES.lock() {
            map.insert(key.to_string(), value.map(ToString::to_string));
        }
    }

    pub(super) fn restore(key: &str, previous: Option<Option<String>>) {
        if let Ok(mut map) = OVERRIDES.lock() {
            match previous {
                Some(value) => {
                    map.insert(key.to_string(), value);
                }
                None => {
                    map.remove(key);
                }
            }
        }
    }
}

fn read_env_var(key: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(override_value) = test_env_overrides::get(key) {
        return override_value;
    }

    env::var(key).ok()
}

/// Load environment variables from .env file
///
/// This function attempts to load environment variables from a .env file
/// in the current directory. It logs a warning if the file exists but cannot
/// be loaded, but doesn't fail if the file doesn't exist.
pub fn load_dotenv() -> Result<()> {
    match dotenvy::dotenv() {
        Ok(path) => {
            // Only print in verbose mode to avoid polluting stdout/stderr in scripts
            if read_env_var("VTCODE_VERBOSE").is_some() || read_env_var("RUST_LOG").is_some() {
                tracing::info!("Loaded environment variables from: {}", path.display());
            }
            Ok(())
        }
        Err(dotenvy::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            // .env file doesn't exist, which is fine
            Ok(())
        }
        Err(e) => {
            tracing::warn!("Failed to load .env file: {}", e);
            Ok(())
        }
    }
}

/// Get API key for a specific provider with secure fallback mechanism
///
/// This function implements a secure retrieval mechanism that:
/// 1. First checks environment variables (highest priority for security)
/// 2. Then checks .env file values
/// 3. Falls back to configuration file values if neither above is set
/// 4. Supports all major providers: Gemini, Anthropic, OpenAI, and OpenRouter
/// 5. Automatically infers the correct environment variable based on provider
///
/// # Arguments
///
/// * `provider` - The provider name ("gemini", "anthropic", or "openai")
/// * `sources` - Configuration for where to look for API keys
///
/// # Returns
///
/// * `Ok(String)` - The API key if found
/// * `Err` - If no API key could be found for the provider
pub fn get_api_key(provider: &str, sources: &ApiKeySources) -> Result<String> {
    let normalized_provider = provider.to_lowercase();
    // Automatically infer the correct environment variable based on provider
    let inferred_env = inferred_api_key_env(&normalized_provider);

    // Try the inferred environment variable first
    if let Some(key) = read_env_var(inferred_env)
        && !key.is_empty()
    {
        return Ok(key);
    }

    // Try secure storage (keyring) for custom API keys
    if let Ok(Some(key)) = get_custom_api_key_from_keyring(&normalized_provider) {
        return Ok(key);
    }

    // Fall back to the provider-specific sources
    match normalized_provider.as_str() {
        "gemini" => get_gemini_api_key(sources),
        "anthropic" => get_anthropic_api_key(sources),
        "openai" => get_openai_api_key(sources),
        "deepseek" => get_deepseek_api_key(sources),
        "openrouter" => get_openrouter_api_key(sources),
        "zai" => get_zai_api_key(sources),
        "ollama" => get_ollama_api_key(sources),
        "lmstudio" => get_lmstudio_api_key(sources),
        "huggingface" => {
            read_env_var("HF_TOKEN").ok_or_else(|| anyhow::anyhow!("HF_TOKEN not set"))
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider)),
    }
}

/// Get custom API key from secure storage (keyring).
///
/// This function retrieves API keys that were stored securely via the model picker
/// or interactive configuration flows.
///
/// # Arguments
/// * `provider` - The provider name
///
/// # Returns
/// * `Ok(Some(String))` - The API key if found in keyring
/// * `Ok(None)` - If no key is stored for this provider
/// * `Err` - If there was an error accessing the keyring
fn get_custom_api_key_from_keyring(provider: &str) -> Result<Option<String>> {
    let storage = CustomApiKeyStorage::new(provider);
    // Use default storage mode (keyring)
    let mode = crate::auth::AuthCredentialsStoreMode::default();
    storage.load(mode)
}

/// Get API key for a specific environment variable with fallback
fn get_api_key_with_fallback(
    env_var: &str,
    config_value: Option<&String>,
    provider_name: &str,
) -> Result<String> {
    // First try environment variable (most secure)
    if let Some(key) = read_env_var(env_var)
        && !key.is_empty()
    {
        return Ok(key);
    }

    // Then try configuration file value
    if let Some(key) = config_value
        && !key.is_empty()
    {
        return Ok(key.clone());
    }

    // If neither worked, return an error
    Err(anyhow::anyhow!(
        "No API key found for {} provider. Set {} environment variable (or add to .env file) or configure in vtcode.toml",
        provider_name,
        env_var
    ))
}

fn get_optional_api_key_with_fallback(env_var: &str, config_value: Option<&String>) -> String {
    if let Some(key) = read_env_var(env_var)
        && !key.is_empty()
    {
        return key;
    }

    if let Some(key) = config_value
        && !key.is_empty()
    {
        return key.clone();
    }

    String::new()
}

/// Get Gemini API key with secure fallback
fn get_gemini_api_key(sources: &ApiKeySources) -> Result<String> {
    // Try primary Gemini environment variable
    if let Some(key) = read_env_var(&sources.gemini_env)
        && !key.is_empty()
    {
        return Ok(key);
    }

    // Try Google API key as fallback (for backward compatibility)
    if let Some(key) = read_env_var("GOOGLE_API_KEY")
        && !key.is_empty()
    {
        return Ok(key);
    }

    // Try configuration file value
    if let Some(key) = &sources.gemini_config
        && !key.is_empty()
    {
        return Ok(key.clone());
    }

    // If nothing worked, return an error
    Err(anyhow::anyhow!(
        "No API key found for Gemini provider. Set {} or GOOGLE_API_KEY environment variable (or add to .env file) or configure in vtcode.toml",
        sources.gemini_env
    ))
}

/// Get Anthropic API key with secure fallback
fn get_anthropic_api_key(sources: &ApiKeySources) -> Result<String> {
    get_api_key_with_fallback(
        &sources.anthropic_env,
        sources.anthropic_config.as_ref(),
        "Anthropic",
    )
}

/// Get OpenAI API key with secure fallback
fn get_openai_api_key(sources: &ApiKeySources) -> Result<String> {
    get_api_key_with_fallback(
        &sources.openai_env,
        sources.openai_config.as_ref(),
        "OpenAI",
    )
}

/// Get OpenRouter API key with secure fallback
///
/// This function checks for credentials in the following order:
/// 1. OAuth token from encrypted storage (if OAuth is enabled)
/// 2. Environment variable (OPENROUTER_API_KEY)
/// 3. Configuration file value
fn get_openrouter_api_key(sources: &ApiKeySources) -> Result<String> {
    // First, try to load OAuth token from encrypted storage
    if let Ok(Some(token)) = crate::auth::load_oauth_token() {
        tracing::debug!("Using OAuth token for OpenRouter authentication");
        return Ok(token.api_key);
    }

    // Fall back to standard API key retrieval
    get_api_key_with_fallback(
        &sources.openrouter_env,
        sources.openrouter_config.as_ref(),
        "OpenRouter",
    )
}

/// Get DeepSeek API key with secure fallback
fn get_deepseek_api_key(sources: &ApiKeySources) -> Result<String> {
    get_api_key_with_fallback(
        &sources.deepseek_env,
        sources.deepseek_config.as_ref(),
        "DeepSeek",
    )
}

/// Get Z.AI API key with secure fallback
fn get_zai_api_key(sources: &ApiKeySources) -> Result<String> {
    get_api_key_with_fallback(&sources.zai_env, sources.zai_config.as_ref(), "Z.AI")
}

/// Get Ollama API key with secure fallback
fn get_ollama_api_key(sources: &ApiKeySources) -> Result<String> {
    // For Ollama we allow running without credentials when connecting to a local deployment.
    // Cloud variants still rely on environment/config values when present.
    Ok(get_optional_api_key_with_fallback(
        &sources.ollama_env,
        sources.ollama_config.as_ref(),
    ))
}

/// Get LM Studio API key with secure fallback
fn get_lmstudio_api_key(sources: &ApiKeySources) -> Result<String> {
    Ok(get_optional_api_key_with_fallback(
        &sources.lmstudio_env,
        sources.lmstudio_config.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvOverrideGuard {
        key: &'static str,
        previous: Option<Option<String>>,
    }

    impl EnvOverrideGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = test_env_overrides::get(key);
            test_env_overrides::set(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvOverrideGuard {
        fn drop(&mut self) {
            test_env_overrides::restore(self.key, self.previous.clone());
        }
    }

    fn with_override<F>(key: &'static str, value: Option<&str>, f: F)
    where
        F: FnOnce(),
    {
        let _guard = EnvOverrideGuard::set(key, value);
        f();
    }

    fn with_overrides<F>(overrides: &[(&'static str, Option<&str>)], f: F)
    where
        F: FnOnce(),
    {
        let _guards: Vec<_> = overrides
            .iter()
            .map(|(key, value)| EnvOverrideGuard::set(key, *value))
            .collect();
        f();
    }

    #[test]
    fn test_get_gemini_api_key_from_env() {
        with_override("TEST_GEMINI_KEY", Some("test-gemini-key"), || {
            let sources = ApiKeySources {
                gemini_env: "TEST_GEMINI_KEY".to_string(),
                ..Default::default()
            };

            let result = get_gemini_api_key(&sources);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "test-gemini-key");
        });
    }

    #[test]
    fn test_get_anthropic_api_key_from_env() {
        with_override("TEST_ANTHROPIC_KEY", Some("test-anthropic-key"), || {
            let sources = ApiKeySources {
                anthropic_env: "TEST_ANTHROPIC_KEY".to_string(),
                ..Default::default()
            };

            let result = get_anthropic_api_key(&sources);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "test-anthropic-key");
        });
    }

    #[test]
    fn test_get_openai_api_key_from_env() {
        with_override("TEST_OPENAI_KEY", Some("test-openai-key"), || {
            let sources = ApiKeySources {
                openai_env: "TEST_OPENAI_KEY".to_string(),
                ..Default::default()
            };

            let result = get_openai_api_key(&sources);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "test-openai-key");
        });
    }

    #[test]
    fn test_get_deepseek_api_key_from_env() {
        with_override("TEST_DEEPSEEK_KEY", Some("test-deepseek-key"), || {
            let sources = ApiKeySources {
                deepseek_env: "TEST_DEEPSEEK_KEY".to_string(),
                ..Default::default()
            };

            let result = get_deepseek_api_key(&sources);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "test-deepseek-key");
        });
    }

    #[test]
    fn test_get_gemini_api_key_from_config() {
        with_overrides(
            &[
                ("TEST_GEMINI_CONFIG_KEY", None),
                ("GOOGLE_API_KEY", None),
                ("GEMINI_API_KEY", None),
            ],
            || {
                let sources = ApiKeySources {
                    gemini_env: "TEST_GEMINI_CONFIG_KEY".to_string(),
                    gemini_config: Some("config-gemini-key".to_string()),
                    ..Default::default()
                };

                let result = get_gemini_api_key(&sources);
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), "config-gemini-key");
            },
        );
    }

    #[test]
    fn test_get_api_key_with_fallback_prefers_env() {
        with_override("TEST_FALLBACK_KEY", Some("env-key"), || {
            let sources = ApiKeySources {
                openai_env: "TEST_FALLBACK_KEY".to_string(),
                openai_config: Some("config-key".to_string()),
                ..Default::default()
            };

            let result = get_openai_api_key(&sources);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "env-key"); // Should prefer env var
        });
    }

    #[test]
    fn test_get_api_key_fallback_to_config() {
        let sources = ApiKeySources {
            openai_env: "NONEXISTENT_ENV_VAR".to_string(),
            openai_config: Some("config-key".to_string()),
            ..Default::default()
        };

        let result = get_openai_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "config-key");
    }

    #[test]
    fn test_get_api_key_error_when_not_found() {
        let sources = ApiKeySources {
            openai_env: "NONEXISTENT_ENV_VAR".to_string(),
            ..Default::default()
        };

        let result = get_openai_api_key(&sources);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_ollama_api_key_missing_sources() {
        let sources = ApiKeySources {
            ollama_env: "NONEXISTENT_OLLAMA_ENV".to_string(),
            ..Default::default()
        };

        let result = get_ollama_api_key(&sources).expect("Ollama key retrieval should succeed");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_ollama_api_key_from_env() {
        with_override("TEST_OLLAMA_KEY", Some("test-ollama-key"), || {
            let sources = ApiKeySources {
                ollama_env: "TEST_OLLAMA_KEY".to_string(),
                ..Default::default()
            };

            let result = get_ollama_api_key(&sources);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "test-ollama-key");
        });
    }

    #[test]
    fn test_get_lmstudio_api_key_missing_sources() {
        let sources = ApiKeySources {
            lmstudio_env: "NONEXISTENT_LMSTUDIO_ENV".to_string(),
            ..Default::default()
        };

        let result =
            get_lmstudio_api_key(&sources).expect("LM Studio key retrieval should succeed");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_lmstudio_api_key_from_env() {
        with_override("TEST_LMSTUDIO_KEY", Some("test-lmstudio-key"), || {
            let sources = ApiKeySources {
                lmstudio_env: "TEST_LMSTUDIO_KEY".to_string(),
                ..Default::default()
            };

            let result = get_lmstudio_api_key(&sources);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "test-lmstudio-key");
        });
    }

    #[test]
    fn test_get_api_key_ollama_provider() {
        with_override(
            "TEST_OLLAMA_PROVIDER_KEY",
            Some("test-ollama-env-key"),
            || {
                let sources = ApiKeySources {
                    ollama_env: "TEST_OLLAMA_PROVIDER_KEY".to_string(),
                    ..Default::default()
                };
                let result = get_api_key("ollama", &sources);
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), "test-ollama-env-key");
            },
        );
    }

    #[test]
    fn test_get_api_key_lmstudio_provider() {
        with_override(
            "TEST_LMSTUDIO_PROVIDER_KEY",
            Some("test-lmstudio-env-key"),
            || {
                let sources = ApiKeySources {
                    lmstudio_env: "TEST_LMSTUDIO_PROVIDER_KEY".to_string(),
                    ..Default::default()
                };
                let result = get_api_key("lmstudio", &sources);
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), "test-lmstudio-env-key");
            },
        );
    }
}
