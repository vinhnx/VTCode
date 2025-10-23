//! API key management module for secure retrieval from environment variables,
//! .env files, and configuration files.
//!
//! This module provides a unified interface for retrieving API keys for different providers,
//! prioritizing security by checking environment variables first, then .env files, and finally
//! falling back to configuration file values.

use anyhow::Result;
use std::env;

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
    /// xAI API key environment variable name
    pub xai_env: String,
    /// DeepSeek API key environment variable name
    pub deepseek_env: String,
    /// Z.AI API key environment variable name
    pub zai_env: String,
    /// Ollama API key environment variable name
    pub ollama_env: String,
    /// Gemini API key from configuration file
    pub gemini_config: Option<String>,
    /// Anthropic API key from configuration file
    pub anthropic_config: Option<String>,
    /// OpenAI API key from configuration file
    pub openai_config: Option<String>,
    /// OpenRouter API key from configuration file
    pub openrouter_config: Option<String>,
    /// xAI API key from configuration file
    pub xai_config: Option<String>,
    /// DeepSeek API key from configuration file
    pub deepseek_config: Option<String>,
    /// Z.AI API key from configuration file
    pub zai_config: Option<String>,
    /// Ollama API key from configuration file
    pub ollama_config: Option<String>,
}

impl Default for ApiKeySources {
    fn default() -> Self {
        Self {
            gemini_env: "GEMINI_API_KEY".to_string(),
            anthropic_env: "ANTHROPIC_API_KEY".to_string(),
            openai_env: "OPENAI_API_KEY".to_string(),
            openrouter_env: "OPENROUTER_API_KEY".to_string(),
            xai_env: "XAI_API_KEY".to_string(),
            deepseek_env: "DEEPSEEK_API_KEY".to_string(),
            zai_env: "ZAI_API_KEY".to_string(),
            ollama_env: "OLLAMA_API_KEY".to_string(),
            gemini_config: None,
            anthropic_config: None,
            openai_config: None,
            openrouter_config: None,
            xai_config: None,
            deepseek_config: None,
            zai_config: None,
            ollama_config: None,
        }
    }
}

impl ApiKeySources {
    /// Create API key sources for a specific provider with automatic environment variable inference
    pub fn for_provider(provider: &str) -> Self {
        let (primary_env, _fallback_envs) = match provider.to_lowercase().as_str() {
            "gemini" => ("GEMINI_API_KEY", vec!["GOOGLE_API_KEY"]),
            "anthropic" => ("ANTHROPIC_API_KEY", vec![]),
            "openai" => ("OPENAI_API_KEY", vec![]),
            "deepseek" => ("DEEPSEEK_API_KEY", vec![]),
            "openrouter" => ("OPENROUTER_API_KEY", vec![]),
            "xai" => ("XAI_API_KEY", vec![]),
            "zai" => ("ZAI_API_KEY", vec![]),
            "ollama" => ("OLLAMA_API_KEY", vec![]),
            _ => ("GEMINI_API_KEY", vec!["GOOGLE_API_KEY"]),
        };

        // For backward compatibility, we still set all env vars but prioritize the primary one
        Self {
            gemini_env: if provider == "gemini" {
                primary_env.to_string()
            } else {
                "GEMINI_API_KEY".to_string()
            },
            anthropic_env: if provider == "anthropic" {
                primary_env.to_string()
            } else {
                "ANTHROPIC_API_KEY".to_string()
            },
            openai_env: if provider == "openai" {
                primary_env.to_string()
            } else {
                "OPENAI_API_KEY".to_string()
            },
            openrouter_env: if provider == "openrouter" {
                primary_env.to_string()
            } else {
                "OPENROUTER_API_KEY".to_string()
            },
            xai_env: if provider == "xai" {
                primary_env.to_string()
            } else {
                "XAI_API_KEY".to_string()
            },
            deepseek_env: if provider == "deepseek" {
                primary_env.to_string()
            } else {
                "DEEPSEEK_API_KEY".to_string()
            },
            zai_env: if provider == "zai" {
                primary_env.to_string()
            } else {
                "ZAI_API_KEY".to_string()
            },
            ollama_env: if provider == "ollama" {
                primary_env.to_string()
            } else {
                "OLLAMA_API_KEY".to_string()
            },
            gemini_config: None,
            anthropic_config: None,
            openai_config: None,
            openrouter_config: None,
            xai_config: None,
            deepseek_config: None,
            zai_config: None,
            ollama_config: None,
        }
    }
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
            if std::env::var("VTCODE_VERBOSE").is_ok() || std::env::var("RUST_LOG").is_ok() {
                eprintln!("Loaded environment variables from: {}", path.display());
            }
            Ok(())
        }
        Err(dotenvy::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            // .env file doesn't exist, which is fine
            Ok(())
        }
        Err(e) => {
            eprintln!("Warning: Failed to load .env file: {}", e);
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
/// 4. Supports all major providers: Gemini, Anthropic, OpenAI, OpenRouter, and xAI
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
    // Automatically infer the correct environment variable based on provider
    let inferred_env = match provider.to_lowercase().as_str() {
        "gemini" => "GEMINI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "xai" => "XAI_API_KEY",
        "zai" => "ZAI_API_KEY",
        "ollama" => "OLLAMA_API_KEY",
        _ => "GEMINI_API_KEY",
    };

    // Try the inferred environment variable first
    if let Ok(key) = env::var(inferred_env)
        && !key.is_empty()
    {
        return Ok(key);
    }

    // Fall back to the provider-specific sources
    match provider.to_lowercase().as_str() {
        "gemini" => get_gemini_api_key(sources),
        "anthropic" => get_anthropic_api_key(sources),
        "openai" => get_openai_api_key(sources),
        "deepseek" => get_deepseek_api_key(sources),
        "openrouter" => get_openrouter_api_key(sources),
        "xai" => get_xai_api_key(sources),
        "zai" => get_zai_api_key(sources),
        "ollama" => get_ollama_api_key(sources),
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider)),
    }
}

/// Get API key for a specific environment variable with fallback
fn get_api_key_with_fallback(
    env_var: &str,
    config_value: Option<&String>,
    provider_name: &str,
) -> Result<String> {
    // First try environment variable (most secure)
    if let Ok(key) = env::var(env_var)
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

/// Get Gemini API key with secure fallback
fn get_gemini_api_key(sources: &ApiKeySources) -> Result<String> {
    // Try primary Gemini environment variable
    if let Ok(key) = env::var(&sources.gemini_env)
        && !key.is_empty()
    {
        return Ok(key);
    }

    // Try Google API key as fallback (for backward compatibility)
    if let Ok(key) = env::var("GOOGLE_API_KEY")
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
fn get_openrouter_api_key(sources: &ApiKeySources) -> Result<String> {
    get_api_key_with_fallback(
        &sources.openrouter_env,
        sources.openrouter_config.as_ref(),
        "OpenRouter",
    )
}

/// Get xAI API key with secure fallback
fn get_xai_api_key(sources: &ApiKeySources) -> Result<String> {
    get_api_key_with_fallback(&sources.xai_env, sources.xai_config.as_ref(), "xAI")
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
    get_api_key_with_fallback(&sources.ollama_env, sources.ollama_config.as_ref(), "Ollama")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_get_gemini_api_key_from_env() {
        // Set environment variable
        unsafe {
            env::set_var("TEST_GEMINI_KEY", "test-gemini-key");
        }

        let sources = ApiKeySources {
            gemini_env: "TEST_GEMINI_KEY".to_string(),
            ..Default::default()
        };

        let result = get_gemini_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-gemini-key");

        // Clean up
        unsafe {
            env::remove_var("TEST_GEMINI_KEY");
        }
    }

    #[test]
    fn test_get_anthropic_api_key_from_env() {
        // Set environment variable
        unsafe {
            env::set_var("TEST_ANTHROPIC_KEY", "test-anthropic-key");
        }

        let sources = ApiKeySources {
            anthropic_env: "TEST_ANTHROPIC_KEY".to_string(),
            ..Default::default()
        };

        let result = get_anthropic_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-anthropic-key");

        // Clean up
        unsafe {
            env::remove_var("TEST_ANTHROPIC_KEY");
        }
    }

    #[test]
    fn test_get_openai_api_key_from_env() {
        // Set environment variable
        unsafe {
            env::set_var("TEST_OPENAI_KEY", "test-openai-key");
        }

        let sources = ApiKeySources {
            openai_env: "TEST_OPENAI_KEY".to_string(),
            ..Default::default()
        };

        let result = get_openai_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-openai-key");

        // Clean up
        unsafe {
            env::remove_var("TEST_OPENAI_KEY");
        }
    }

    #[test]
    fn test_get_deepseek_api_key_from_env() {
        unsafe {
            env::set_var("TEST_DEEPSEEK_KEY", "test-deepseek-key");
        }

        let sources = ApiKeySources {
            deepseek_env: "TEST_DEEPSEEK_KEY".to_string(),
            ..Default::default()
        };

        let result = get_deepseek_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-deepseek-key");

        unsafe {
            env::remove_var("TEST_DEEPSEEK_KEY");
        }
    }

    #[test]
    fn test_get_xai_api_key_from_env() {
        unsafe {
            env::set_var("TEST_XAI_KEY", "test-xai-key");
        }

        let sources = ApiKeySources {
            xai_env: "TEST_XAI_KEY".to_string(),
            ..Default::default()
        };

        let result = get_xai_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-xai-key");

        unsafe {
            env::remove_var("TEST_XAI_KEY");
        }
    }

    #[test]
    fn test_get_gemini_api_key_from_config() {
        let sources = ApiKeySources {
            gemini_config: Some("config-gemini-key".to_string()),
            ..Default::default()
        };

        let result = get_gemini_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "config-gemini-key");
    }

    #[test]
    fn test_get_api_key_with_fallback_prefers_env() {
        // Set environment variable
        unsafe {
            env::set_var("TEST_FALLBACK_KEY", "env-key");
        }

        let sources = ApiKeySources {
            openai_env: "TEST_FALLBACK_KEY".to_string(),
            openai_config: Some("config-key".to_string()),
            ..Default::default()
        };

        let result = get_openai_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "env-key"); // Should prefer env var

        // Clean up
        unsafe {
            env::remove_var("TEST_FALLBACK_KEY");
        }
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
    fn test_get_ollama_api_key_from_env() {
        // Set environment variable
        unsafe {
            env::set_var("TEST_OLLAMA_KEY", "test-ollama-key");
        }

        let sources = ApiKeySources {
            ollama_env: "TEST_OLLAMA_KEY".to_string(),
            ..Default::default()
        };

        let result = get_ollama_api_key(&sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-ollama-key");

        // Clean up
        unsafe {
            env::remove_var("TEST_OLLAMA_KEY");
        }
    }

    #[test]
    fn test_get_api_key_ollama_provider() {
        // Set environment variable
        unsafe {
            env::set_var("OLLAMA_API_KEY", "test-ollama-env-key");
        }

        let sources = ApiKeySources::default();
        let result = get_api_key("ollama", &sources);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-ollama-env-key");

        // Clean up
        unsafe {
            env::remove_var("OLLAMA_API_KEY");
        }
    }
}
