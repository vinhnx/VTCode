//! API key management module for secure retrieval from environment variables,
//! .env files, and configuration files.
//!
//! This module provides a unified interface for retrieving API keys for different providers,
//! prioritizing security by checking environment variables first, then .env files, and finally
//! falling back to configuration file values.

use anyhow::Result;
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

fn read_env_var(key: &str) -> Option<String> {
    crate::env_helpers::read_env_var(key)
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
        "ollama" | "lmstudio" | "llamacpp" | "llama.cpp" | "llama-cpp" => Ok(String::new()),
        // Managed-auth providers show a specific error message
        "copilot" => Err(anyhow::anyhow!(
            "GitHub Copilot authentication is managed by the official `copilot` CLI. Run `vtcode login copilot`."
        )),
        "codex" => Err(anyhow::anyhow!(
            "Codex authentication is managed by the official `codex app-server`. Run `vtcode login codex`."
        )),
        // All other providers: env var was already checked above, nothing more to do
        _ => Err(anyhow::anyhow!(
            "{normalized_provider} API key not found. Export {inferred_env} in your shell, or paste it with `/model` (it is stored in your OS keyring, not a workspace .env).",
        )),
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

/// Where a provider's credential was discovered.
///
/// Used by the first-run wizard and model picker to show *why* a provider is
/// ready without re-prompting for a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    /// Process environment variable — covers shell exports (e.g. `~/.zshrc`)
    /// and values loaded from a workspace `.env` by `load_dotenv()`.
    Env,
    /// OS keyring / encrypted file storage (`CustomApiKeyStorage`).
    SecureStorage,
    /// Active OAuth session (OpenRouter or OpenAI ChatGPT).
    OAuth,
    /// Auth is managed by an external CLI (e.g. GitHub Copilot via `copilot`).
    ManagedAuth,
    /// Local server — no key required (Ollama, LM Studio, llama.cpp).
    Local,
}

impl CredentialSource {
    /// One-line, user-facing description of where the credential came from.
    pub fn describe(self, provider: Provider) -> &'static str {
        match self {
            CredentialSource::Env => "found in environment",
            CredentialSource::SecureStorage => "stored in OS keyring",
            CredentialSource::OAuth => "OAuth session active",
            CredentialSource::ManagedAuth => "managed by external CLI",
            CredentialSource::Local => {
                if provider.is_local() {
                    "local — no key required"
                } else {
                    "ready"
                }
            }
        }
    }
}

/// A provider with a discoverable credential — ready to use without prompting
/// the user to paste a key.
#[derive(Debug, Clone, Copy)]
pub struct DiscoveredProvider {
    pub provider: Provider,
    pub source: CredentialSource,
    /// The specific environment variable that satisfied discovery, when
    /// `source == Env`. Carries the *alternate* name (e.g. `GOOGLE_API_KEY`)
    /// when discovery used the alternate rather than the primary env var, so
    /// the UI can tell the user exactly what was read — e.g. "Found
    /// GOOGLE_API_KEY in your environment" instead of a generic "found in
    /// environment". `None` for non-env sources and for providers with no env
    /// var (local / managed-auth).
    pub env_var: Option<&'static str>,
}

/// Determine whether a single built-in provider has a usable credential right
/// now, and the full detail of where it came from. Returns `None` when no
/// credential is found.
///
/// Mirrors the resolution order of [`get_api_key`]: env var (including
/// provider-specific alternate env vars) → OAuth session → secure storage.
/// Local and managed-auth providers are always considered ready.
///
/// Prefer this over [`provider_credential_source`] when you need to surface
/// *which* env var was read (e.g. the first-run wizard and `api_key_hint`).
pub fn provider_credential_detail(provider: Provider) -> Option<DiscoveredProvider> {
    if provider.is_local() {
        return Some(DiscoveredProvider {
            provider,
            source: CredentialSource::Local,
            env_var: None,
        });
    }
    if provider.uses_managed_auth() {
        return Some(DiscoveredProvider {
            provider,
            source: CredentialSource::ManagedAuth,
            env_var: None,
        });
    }

    // OAuth-backed providers: an active session counts as ready.
    if matches!(provider, Provider::OpenRouter) && crate::auth::load_oauth_token().ok().flatten().is_some() {
        return Some(DiscoveredProvider {
            provider,
            source: CredentialSource::OAuth,
            env_var: None,
        });
    }
    if matches!(provider, Provider::OpenAI) && crate::auth::load_openai_chatgpt_session().ok().flatten().is_some() {
        return Some(DiscoveredProvider {
            provider,
            source: CredentialSource::OAuth,
            env_var: None,
        });
    }

    // Primary env var for the provider.
    let env_key = provider.default_api_key_env();
    if !env_key.is_empty() && env_value_present(env_key) {
        return Some(DiscoveredProvider {
            provider,
            source: CredentialSource::Env,
            env_var: Some(env_key),
        });
    }

    // Provider-specific alternate env vars (kept in sync with get_api_key).
    let alt = alternate_env_var(provider);
    if let Some(alt_key) = alt
        && env_value_present(alt_key)
    {
        return Some(DiscoveredProvider {
            provider,
            source: CredentialSource::Env,
            env_var: Some(alt_key),
        });
    }

    // Secure storage (OS keyring with encrypted-file fallback).
    if has_stored_credential(provider) {
        return Some(DiscoveredProvider {
            provider,
            source: CredentialSource::SecureStorage,
            env_var: None,
        });
    }

    None
}

/// Thin wrapper over [`provider_credential_detail`] that returns only the
/// credential source. Kept for backward compatibility with callers that don't
/// need the env-var detail.
pub fn provider_credential_source(provider: Provider) -> Option<CredentialSource> {
    provider_credential_detail(provider).map(|detail| detail.source)
}

/// Scan all built-in providers and return those with a discoverable credential.
///
/// "Discoverable" means the provider can be used right now without the user
/// pasting a key: the env var is set (shell export or loaded `.env`), a key is
/// in secure storage, an OAuth session is active, auth is managed by an
/// external CLI, or the provider is local and needs no key.
///
/// Results follow `Provider::all_providers()` order. This does not consult
/// `vtcode.toml` custom providers — the first-run wizard runs before a config
/// exists. Runtime custom-provider auth is handled by `resolve_runtime_provider_auth`.
pub fn discover_available_providers() -> Vec<DiscoveredProvider> {
    Provider::all_providers()
        .into_iter()
        .filter_map(provider_credential_detail)
        .collect()
}

/// Look up a provider in a discovery snapshot.
pub fn find_discovered(discovered: &[DiscoveredProvider], provider: Provider) -> Option<&DiscoveredProvider> {
    discovered.iter().find(|entry| entry.provider == provider)
}

/// Check whether any provider in the slice has an OAuth session or managed auth.
///
/// Used by secret-management UIs to decide whether to show the generic
/// `secret add/delete` hints or the OAuth-specific `login` hint.
pub fn has_oauth_or_managed_auth(discovered: &[DiscoveredProvider]) -> bool {
    discovered
        .iter()
        .any(|entry| matches!(entry.source, CredentialSource::OAuth | CredentialSource::ManagedAuth))
}

fn env_value_present(env_key: &str) -> bool {
    matches!(read_env_var(env_key), Some(value) if !value.trim().is_empty())
}

/// Alternate env var names that `get_api_key` accepts for a provider.
fn alternate_env_var(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::Gemini => Some("GOOGLE_API_KEY"),
        Provider::Qwen => Some("DASHSCOPE_API_KEY"),
        _ => None,
    }
}

fn has_stored_credential(provider: Provider) -> bool {
    get_custom_api_key_from_secure_storage(provider.as_ref())
        .ok()
        .flatten()
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialise all env-override tests so that one test's Drop restore cannot
    // overwrite another test's set.
    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct EnvOverrideGuard {
        key: &'static str,
        previous: Option<Option<String>>,
    }

    impl EnvOverrideGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = crate::env_helpers::test_env_overrides::get(key);
            crate::env_helpers::test_env_overrides::set(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvOverrideGuard {
        fn drop(&mut self) {
            crate::env_helpers::test_env_overrides::restore(self.key, self.previous.clone());
        }
    }

    fn with_override<F>(key: &'static str, value: Option<&str>, f: F)
    where
        F: FnOnce(),
    {
        let _lock = ENV_TEST_LOCK.lock().expect("env test lock poisoned");
        let _guard = EnvOverrideGuard::set(key, value);
        f();
    }

    fn with_overrides<F>(overrides: &[(&'static str, Option<&str>)], f: F)
    where
        F: FnOnce(),
    {
        let _lock = ENV_TEST_LOCK.lock().expect("env test lock poisoned");
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
        with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("google-fallback"))], || {
            // Without GEMINI_API_KEY, it should fall back to GOOGLE_API_KEY
            let result = get_api_key("gemini", &default_sources());
            assert_eq!(result.unwrap(), "google-fallback");
        });
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
        with_overrides(&[("QWEN_API_KEY", None), ("DASHSCOPE_API_KEY", Some("dashscope-key"))], || {
            let result = get_api_key("qwen", &default_sources());
            assert_eq!(result.unwrap(), "dashscope-key");
        });
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
        assert_eq!(resolve_api_key_env("minimax", defaults::DEFAULT_API_KEY_ENV), "MINIMAX_API_KEY");
    }

    #[test]
    fn resolve_api_key_env_preserves_explicit_override() {
        assert_eq!(resolve_api_key_env("openai", "CUSTOM_OPENAI_KEY"), "CUSTOM_OPENAI_KEY");
    }

    #[test]
    fn local_providers_are_always_discovered() {
        // Local providers need no key and should be discoverable with empty env.
        with_overrides(
            &[
                ("OLLAMA_API_KEY", None),
                ("LMSTUDIO_API_KEY", None),
                ("LLAMACPP_API_KEY", None),
            ],
            || {
                assert_eq!(provider_credential_source(Provider::Ollama), Some(CredentialSource::Local));
                assert_eq!(provider_credential_source(Provider::LmStudio), Some(CredentialSource::Local));
                assert_eq!(provider_credential_source(Provider::LlamaCpp), Some(CredentialSource::Local));
            },
        );
    }

    #[test]
    fn copilot_is_managed_auth_discovered() {
        assert_eq!(provider_credential_source(Provider::Copilot), Some(CredentialSource::ManagedAuth));
    }

    #[test]
    fn env_var_makes_provider_discovered() {
        with_override("OPENROUTER_API_KEY", Some("or-test-key"), || {
            assert_eq!(provider_credential_source(Provider::OpenRouter), Some(CredentialSource::Env));
        });
    }

    #[test]
    fn missing_env_var_leaves_provider_undiscovered() {
        with_override("OPENROUTER_API_KEY", None, || {
            assert_eq!(provider_credential_source(Provider::OpenRouter), None);
        });
    }

    #[test]
    fn gemini_alt_env_var_is_discovered() {
        with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("g-key"))], || {
            assert_eq!(provider_credential_source(Provider::Gemini), Some(CredentialSource::Env));
        });
    }

    #[test]
    fn qwen_alt_env_var_is_discovered() {
        with_overrides(&[("QWEN_API_KEY", None), ("DASHSCOPE_API_KEY", Some("ds-key"))], || {
            assert_eq!(provider_credential_source(Provider::Qwen), Some(CredentialSource::Env));
        });
    }

    #[test]
    fn credential_detail_surfaces_primary_env_var_name() {
        with_override("OPENROUTER_API_KEY", Some("or-key"), || {
            let detail = provider_credential_detail(Provider::OpenRouter).expect("OpenRouter discovered");
            assert_eq!(detail.source, CredentialSource::Env);
            assert_eq!(detail.env_var, Some("OPENROUTER_API_KEY"));
        });
    }

    #[test]
    fn credential_detail_surfaces_alternate_env_var_name() {
        // When only the alternate GOOGLE_API_KEY is set, the detail must report
        // *that* name (not the primary GEMINI_API_KEY) so the UI can tell the
        // user exactly which variable was read.
        with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("g-key"))], || {
            let detail = provider_credential_detail(Provider::Gemini).expect("Gemini discovered");
            assert_eq!(detail.source, CredentialSource::Env);
            assert_eq!(detail.env_var, Some("GOOGLE_API_KEY"));
        });
    }

    #[test]
    fn credential_detail_env_var_is_none_for_non_env_sources() {
        // Local and managed-auth providers are discovered without an env var.
        assert_eq!(
            provider_credential_detail(Provider::Ollama).map(|d| d.env_var),
            Some(None),
            "local providers must report env_var = None"
        );
        assert_eq!(
            provider_credential_detail(Provider::Copilot).map(|d| d.env_var),
            Some(None),
            "managed-auth providers must report env_var = None"
        );
    }

    #[test]
    fn credential_detail_returns_none_when_no_credential() {
        with_overrides(
            &[
                ("OPENROUTER_API_KEY", None),
                ("OPENAI_API_KEY", None),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                // OpenRouter has no env var, no OAuth token in tests, no keyring
                // entry in tests → not discovered.
                assert!(provider_credential_detail(Provider::OpenRouter).is_none());
            },
        );
    }

    #[test]
    fn discover_available_providers_carries_env_var_detail() {
        with_overrides(
            &[
                ("OPENROUTER_API_KEY", Some("or-key")),
                ("GEMINI_API_KEY", None),
                ("GOOGLE_API_KEY", Some("g-key")),
                ("OPENAI_API_KEY", None),
                ("ANTHROPIC_API_KEY", None),
            ],
            || {
                let discovered = discover_available_providers();
                let or = find_discovered(&discovered, Provider::OpenRouter).unwrap();
                assert_eq!(or.source, CredentialSource::Env);
                assert_eq!(or.env_var, Some("OPENROUTER_API_KEY"));
                let gemini = find_discovered(&discovered, Provider::Gemini).unwrap();
                assert_eq!(gemini.source, CredentialSource::Env);
                assert_eq!(gemini.env_var, Some("GOOGLE_API_KEY"));
            },
        );
    }

    #[test]
    fn discover_available_providers_includes_ready_providers() {
        // With OPENROUTER_API_KEY set, OpenRouter must appear in discovery
        // alongside the always-ready local + managed-auth providers.
        with_overrides(
            &[
                ("OPENROUTER_API_KEY", Some("or-key")),
                ("OPENAI_API_KEY", None),
                ("ANTHROPIC_API_KEY", None),
                ("GEMINI_API_KEY", None),
            ],
            || {
                let discovered = discover_available_providers();
                let providers: Vec<Provider> = discovered.iter().map(|d| d.provider).collect();

                assert!(providers.contains(&Provider::OpenRouter), "OpenRouter should be discovered");
                assert!(providers.contains(&Provider::Ollama), "Ollama should be discovered (local)");
                assert!(providers.contains(&Provider::Copilot), "Copilot should be discovered (managed auth)");
                assert!(
                    !providers.contains(&Provider::OpenAI),
                    "OpenAI should NOT be discovered when OPENAI_API_KEY is unset"
                );

                let or = find_discovered(&discovered, Provider::OpenRouter).unwrap();
                assert_eq!(or.source, CredentialSource::Env);
            },
        );
    }

    #[test]
    fn credential_source_describes_origin() {
        assert_eq!(CredentialSource::Env.describe(Provider::OpenRouter), "found in environment");
        assert_eq!(CredentialSource::Local.describe(Provider::Ollama), "local — no key required");
    }
}
