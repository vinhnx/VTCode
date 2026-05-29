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
use crate::constants::defaults;
use crate::models::Provider;

/// API key sources for different providers
///
/// Retained for backward compatibility. New code should use [`get_api_key`] directly —
/// the struct is no longer consumed by the key resolution logic.
#[derive(Debug, Clone, Default)]
pub struct ApiKeySources {
    pub gemini_env: String,
    pub anthropic_env: String,
    pub openai_env: String,
    pub openrouter_env: String,
    pub deepseek_env: String,
    pub zai_env: String,
    pub ollama_env: String,
    pub lmstudio_env: String,
    pub gemini_config: Option<String>,
    pub anthropic_config: Option<String>,
    pub openai_config: Option<String>,
    pub openrouter_config: Option<String>,
    pub deepseek_config: Option<String>,
    pub zai_config: Option<String>,
    pub ollama_config: Option<String>,
    pub lmstudio_config: Option<String>,
}

pub fn api_key_env_var(provider: &str) -> String {
    let trimmed = provider.trim();
    if trimmed.is_empty() {
        return defaults::DEFAULT_API_KEY_ENV.to_owned();
    }

    if trimmed.eq_ignore_ascii_case("codex") {
        return String::new();
    }

    if let Ok(resolved) = Provider::from_str(trimmed)
        && resolved.uses_managed_auth()
    {
        return String::new();
    }

    Provider::from_str(trimmed)
        .map(|resolved| resolved.default_api_key_env().to_owned())
        .unwrap_or_else(|_| format!("{}_API_KEY", trimmed.to_ascii_uppercase()))
}

pub fn resolve_api_key_env(provider: &str, configured_env: &str) -> String {
    let trimmed = configured_env.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV) {
        api_key_env_var(provider)
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod test_env_overrides {
    use hashbrown::HashMap;
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

/// Get API key for a specific provider.
///
/// Resolution order:
/// 1. Environment variable inferred from the provider name (e.g. `POOLSIDE_API_KEY`)
/// 2. Provider-specific fallbacks (OAuth tokens, alternate env vars, etc.)
/// 3. OS keyring / encrypted file storage
///
/// Adding a new built-in provider only requires:
/// - A `Provider` variant with `default_api_key_env()` returning the env var name
/// - (Optional) a match arm here only if the provider needs special fallback logic
pub fn get_api_key(provider: &str, _sources: &ApiKeySources) -> Result<String> {
    let normalized_provider = provider.to_lowercase();
    let inferred_env = api_key_env_var(&normalized_provider);

    // Generic path: read the inferred env var for any provider.
    if let Some(key) = read_env_var(&inferred_env)
        && !key.is_empty()
    {
        return Ok(key);
    }

    // Provider-specific fallback logic. Most providers are handled by the generic
    // env-var lookup above. Only providers with special behavior (alternate env vars,
    // OAuth tokens, optional keys, or managed-auth error messages) need a match arm.
    let provider_result = match normalized_provider.as_str() {
        // Gemini falls back to GOOGLE_API_KEY for backward compatibility
        "gemini" => {
            if let Some(key) = read_env_var("GOOGLE_API_KEY").filter(|k| !k.is_empty()) {
                return Ok(key);
            }
            Err(anyhow::anyhow!("GEMINI_API_KEY or GOOGLE_API_KEY not set"))
        }
        // OpenRouter tries OAuth token from encrypted storage first
        "openrouter" => {
            if let Ok(Some(token)) = crate::auth::load_oauth_token() {
                tracing::debug!("Using OAuth token for OpenRouter authentication");
                return Ok(token.api_key);
            }
            Err(anyhow::anyhow!("OPENROUTER_API_KEY not set"))
        }
        // Qwen has an alternate env var name
        "qwen" => {
            if let Some(key) = read_env_var("DASHSCOPE_API_KEY").filter(|k| !k.is_empty()) {
                return Ok(key);
            }
            Err(anyhow::anyhow!("QWEN_API_KEY or DASHSCOPE_API_KEY not set"))
        }
        // Ollama and LM Studio allow empty keys (local deployment)
        "ollama" | "lmstudio" => Ok(String::new()),
        // Managed-auth providers show a specific error message
        "copilot" => Err(anyhow::anyhow!(
            "GitHub Copilot authentication is managed by the official `copilot` CLI. Run `vtcode login copilot`."
        )),
        "codex" => Err(anyhow::anyhow!(
            "Codex authentication is managed by the official `codex app-server`. Run `vtcode login codex`."
        )),
        // All other providers: env var was already checked above, nothing more to do
        _ => {
            return Err(anyhow::anyhow!(
                "{} API key not found. Set {} environment variable or add to .env file.",
                normalized_provider,
                inferred_env,
            ));
        }
    };

    if provider_result.is_ok() {
        return provider_result;
    }

    // Try secure storage (keyring) only after env/config lookup fails.
    if let Ok(Some(key)) = get_custom_api_key_from_secure_storage(&normalized_provider) {
        return Ok(key);
    }

    provider_result
}

/// Get a custom API key from secure storage.
///
/// This function retrieves API keys that were stored securely via the model picker
/// or interactive configuration flows. When the OS keyring is unavailable, the
/// auth layer falls back to encrypted file storage automatically.
///
/// # Arguments
/// * `provider` - The provider name
///
/// # Returns
/// * `Ok(Some(String))` - The API key if found in secure storage
/// * `Ok(None)` - If no key is stored for this provider
/// * `Err` - If there was an error accessing secure storage
fn get_custom_api_key_from_secure_storage(provider: &str) -> Result<Option<String>> {
    let storage = CustomApiKeyStorage::new(provider);
    // The auth layer handles keyring-to-file fallback internally.
    let mode = crate::auth::AuthCredentialsStoreMode::default();
    storage.load(mode)
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

    fn default_sources() -> ApiKeySources {
        ApiKeySources::default()
    }

    #[test]
    fn gemini_reads_env_var() {
        with_override("GEMINI_API_KEY", Some("test-gemini-key"), || {
            let result = get_api_key("gemini", &default_sources());
            assert_eq!(result.unwrap(), "test-gemini-key");
        });
    }

    #[test]
    fn gemini_falls_back_to_google_api_key() {
        // Clear both GEMINI_API_KEY and set GOOGLE_API_KEY to verify fallback
        with_overrides(
            &[
                ("GEMINI_API_KEY", Some("gemini-primary")),
                ("GOOGLE_API_KEY", Some("google-fallback")),
            ],
            || {
                // With GEMINI_API_KEY set, it should be preferred
                let result = get_api_key("gemini", &default_sources());
                assert_eq!(result.unwrap(), "gemini-primary");
            },
        );
        with_overrides(
            &[
                ("GEMINI_API_KEY", None),
                ("GOOGLE_API_KEY", Some("google-fallback")),
            ],
            || {
                // Without GEMINI_API_KEY, it should fall back to GOOGLE_API_KEY
                let result = get_api_key("gemini", &default_sources());
                assert_eq!(result.unwrap(), "google-fallback");
            },
        );
    }

    #[test]
    fn anthropic_reads_env_var() {
        with_override("ANTHROPIC_API_KEY", Some("test-anthropic-key"), || {
            let result = get_api_key("anthropic", &default_sources());
            assert_eq!(result.unwrap(), "test-anthropic-key");
        });
    }

    #[test]
    fn openai_reads_env_var() {
        with_override("OPENAI_API_KEY", Some("test-openai-key"), || {
            let result = get_api_key("openai", &default_sources());
            assert_eq!(result.unwrap(), "test-openai-key");
        });
    }

    #[test]
    fn deepseek_reads_env_var() {
        with_override("DEEPSEEK_API_KEY", Some("test-deepseek-key"), || {
            let result = get_api_key("deepseek", &default_sources());
            assert_eq!(result.unwrap(), "test-deepseek-key");
        });
    }

    #[test]
    fn qwen_falls_back_to_dashscope() {
        with_overrides(
            &[
                ("QWEN_API_KEY", None),
                ("DASHSCOPE_API_KEY", Some("dashscope-key")),
            ],
            || {
                let result = get_api_key("qwen", &default_sources());
                assert_eq!(result.unwrap(), "dashscope-key");
            },
        );
    }

    #[test]
    fn ollama_allows_empty_key() {
        with_override("OLLAMA_API_KEY", None, || {
            let result = get_api_key("ollama", &default_sources());
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        });
    }

    #[test]
    fn lmstudio_allows_empty_key() {
        with_override("LMSTUDIO_API_KEY", None, || {
            let result = get_api_key("lmstudio", &default_sources());
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        });
    }

    #[test]
    fn ollama_reads_env_var_when_set() {
        with_override("OLLAMA_API_KEY", Some("test-ollama-key"), || {
            let result = get_api_key("ollama", &default_sources());
            assert_eq!(result.unwrap(), "test-ollama-key");
        });
    }

    #[test]
    fn copilot_returns_managed_auth_error() {
        let result = get_api_key("copilot", &default_sources());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("copilot"));
    }

    #[test]
    fn codex_returns_managed_auth_error() {
        let result = get_api_key("codex", &default_sources());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("codex"));
    }

    #[test]
    fn unknown_provider_returns_error_with_env_hint() {
        let result = get_api_key("someunknown", &default_sources());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("SOMEUNKNOWN_API_KEY"));
    }

    #[test]
    fn poolside_reads_env_var() {
        with_override("POOLSIDE_API_KEY", Some("test-poolside-key"), || {
            let result = get_api_key("poolside", &default_sources());
            assert_eq!(result.unwrap(), "test-poolside-key");
        });
    }

    #[test]
    fn poolside_returns_error_when_missing() {
        with_override("POOLSIDE_API_KEY", None, || {
            let result = get_api_key("poolside", &default_sources());
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("POOLSIDE_API_KEY"));
        });
    }

    #[test]
    fn api_key_env_var_uses_provider_defaults() {
        assert_eq!(api_key_env_var("codex"), "");
        assert_eq!(api_key_env_var("minimax"), "MINIMAX_API_KEY");
        assert_eq!(api_key_env_var("huggingface"), "HF_TOKEN");
        assert_eq!(api_key_env_var("poolside"), "POOLSIDE_API_KEY");
    }

    #[test]
    fn resolve_api_key_env_uses_provider_default_for_placeholder() {
        assert_eq!(
            resolve_api_key_env("minimax", defaults::DEFAULT_API_KEY_ENV),
            "MINIMAX_API_KEY"
        );
    }

    #[test]
    fn resolve_api_key_env_preserves_explicit_override() {
        assert_eq!(
            resolve_api_key_env("openai", "CUSTOM_OPENAI_KEY"),
            "CUSTOM_OPENAI_KEY"
        );
    }
}
