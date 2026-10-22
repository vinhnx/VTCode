//! API key management module for secure retrieval from environment variables,
//! .env files, and configuration files.
//!
//! This module provides a unified interface for retrieving API keys for different providers,
//! prioritizing security by checking environment variables first, then .env files, and finally
//! falling back to configuration file values.
//!
//! The facade owns provider/key identity and discovery. Source precedence,
//! storage migration, and credential material stay behind the private
//! `credential_resolution` boundary.

use anyhow::Result;
use std::str::FromStr;

use crate::auth::CredentialIdentity;
use crate::constants::defaults;
use crate::models::Provider;

mod credential_resolution;

pub use credential_resolution::{
    CredentialSource, ResolvedCredential, clear_credential_with_mode, get_api_key, get_api_key_with_mode,
    load_stored_api_key_with_mode, load_stored_credential_with_mode, resolve_credential, resolve_credential_with_mode,
    resolve_openai_api_key_for_auth, store_credential_with_mode,
};

/// API key sources for different providers
///
/// Retained for backward compatibility. New code should use [`get_api_key`] directly —
/// the struct is no longer consumed by the key resolution logic.
#[derive(Debug, Clone, Default)]
pub struct ApiKeySources {
    gemini_env: String,
    anthropic_env: String,
    openai_env: String,
    openrouter_env: String,
    deepseek_env: String,
    zai_env: String,
    ollama_env: String,
    lmstudio_env: String,
    gemini_config: Option<String>,
    anthropic_config: Option<String>,
    openai_config: Option<String>,
    openrouter_config: Option<String>,
    deepseek_config: Option<String>,
    zai_config: Option<String>,
    ollama_config: Option<String>,
    lmstudio_config: Option<String>,
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
        .unwrap_or_else(|_| {
            let mut key = String::new();
            for ch in trimmed.chars() {
                if ch.is_ascii_alphanumeric() {
                    key.push(ch.to_ascii_uppercase());
                } else if !key.ends_with('_') {
                    key.push('_');
                }
            }
            if key.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                key.insert(0, '_');
            }
            if !key.ends_with("_API_KEY") {
                if !key.ends_with('_') {
                    key.push('_');
                }
                key.push_str("API_KEY");
            }
            key
        })
}

pub fn resolve_api_key_env(provider: &str, configured_env: &str) -> String {
    let trimmed = configured_env.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV) {
        api_key_env_var(provider)
    } else {
        trimmed.to_owned()
    }
}

/// Build the normalized identity used by credential storage for a provider
/// and an optional configured environment-variable name.
pub fn credential_identity(provider: &str, key_name: &str) -> Result<Option<CredentialIdentity>> {
    let default_key_name = api_key_env_var(provider);
    let requested_key_name = if key_name.trim().is_empty() {
        default_key_name
    } else {
        key_name.trim().to_owned()
    };
    if requested_key_name.is_empty() {
        return Ok(None);
    }
    CredentialIdentity::new(provider, &requested_key_name).map(Some)
}

/// Return the stable configuration metadata key for a provider/key identity.
pub fn credential_metadata_key(provider: &str, key_name: &str) -> Result<Option<String>> {
    Ok(credential_identity(provider, key_name)?
        .map(|identity| format!("{}/{}", identity.provider(), identity.key_name())))
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
    provider_credential_detail_with_mode(provider, crate::auth::AuthCredentialsStoreMode::default())
}

/// Determine whether a provider has a usable credential using `storage_mode`.
pub fn provider_credential_detail_with_mode(
    provider: Provider,
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Option<DiscoveredProvider> {
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

    let resolved =
        resolve_credential_with_mode(provider.as_ref(), provider.default_api_key_env(), None, storage_mode).ok()??;
    if matches!(resolved.source, CredentialSource::SecureStorage)
        || matches!(resolved.source, CredentialSource::Env | CredentialSource::Workspace | CredentialSource::OAuth)
    {
        return Some(DiscoveredProvider {
            provider,
            source: resolved.source,
            env_var: resolved.env_var.as_deref().and_then(static_env_var),
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
    discover_available_providers_with_mode(crate::auth::AuthCredentialsStoreMode::default())
}

/// Scan all built-in providers using the configured secure-storage backend.
pub fn discover_available_providers_with_mode(
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Vec<DiscoveredProvider> {
    Provider::all_providers()
        .into_iter()
        .filter_map(|provider| provider_credential_detail_with_mode(provider, storage_mode))
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

fn static_env_var(env_key: &str) -> Option<&'static str> {
    Provider::all_providers()
        .into_iter()
        .find(|provider| provider.default_api_key_env().eq_ignore_ascii_case(env_key))
        .map(|provider| provider.default_api_key_env())
        .or(match env_key {
            "GOOGLE_API_KEY" => Some("GOOGLE_API_KEY"),
            "DASHSCOPE_API_KEY" => Some("DASHSCOPE_API_KEY"),
            "MODEL_API_KEY" => Some("MODEL_API_KEY"),
            _ => None,
        })
}

/// Alternate env var names that `get_api_key` accepts for a provider.
fn alternate_env_var(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::Gemini => Some("GOOGLE_API_KEY"),
        Provider::Qwen => Some("DASHSCOPE_API_KEY"),
        Provider::Meta => Some("MODEL_API_KEY"),
        _ => None,
    }
}

#[cfg(test)]
fn test_storage_lookup_is_overridden(key_name: &str) -> bool {
    crate::env_helpers::test_env_overrides::is_overridden(key_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

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
        with_override("SOMEUNKNOWN_API_KEY", None, || {
            let result = get_api_key("someunknown", &default_sources());
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(msg.contains("SOMEUNKNOWN_API_KEY"));
        });
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
    fn merge_gateway_reads_env_var() {
        with_override("MERGE_GATEWAY_API_KEY", Some("test-merge-gateway-key"), || {
            let result = get_api_key("merge-gateway", &default_sources());
            assert_eq!(result.unwrap(), "test-merge-gateway-key");
        });
    }

    #[test]
    fn meta_reads_provider_specific_env_var() {
        with_overrides(
            &[
                ("META_API_KEY", Some("meta-primary")),
                ("MODEL_API_KEY", Some("model-fallback")),
            ],
            || {
                let result = get_api_key("meta", &default_sources());
                assert_eq!(result.expect("Meta key"), "meta-primary");
            },
        );
    }

    #[test]
    fn meta_falls_back_to_documented_model_api_key() {
        with_overrides(&[("META_API_KEY", None), ("MODEL_API_KEY", Some("model-key"))], || {
            let result = get_api_key("meta", &default_sources());
            assert_eq!(result.expect("Meta fallback key"), "model-key");
        });
    }

    #[test]
    fn meta_missing_key_error_names_both_supported_env_vars() {
        with_overrides(&[("META_API_KEY", None), ("MODEL_API_KEY", None)], || {
            let error = get_api_key("meta", &default_sources()).expect_err("missing Meta key");
            assert!(error.to_string().contains("META_API_KEY or MODEL_API_KEY"));
        });
    }

    #[test]
    fn api_key_env_var_uses_provider_defaults() {
        assert_eq!(api_key_env_var("codex"), "");
        assert_eq!(api_key_env_var("minimax"), "MINIMAX_API_KEY");
        assert_eq!(api_key_env_var("huggingface"), "HF_TOKEN");
        assert_eq!(api_key_env_var("poolside"), "POOLSIDE_API_KEY");
        assert_eq!(api_key_env_var("merge-gateway"), "MERGE_GATEWAY_API_KEY");
        assert_eq!(api_key_env_var("my-corp"), "MY_CORP_API_KEY");
        assert_eq!(api_key_env_var("123corp"), "_123CORP_API_KEY");
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
    fn credential_metadata_key_normalizes_provider_and_key() {
        assert_eq!(
            credential_metadata_key(" MyCorp ", "mycorp_billing_key").expect("metadata key"),
            Some("mycorp/MYCORP_BILLING_KEY".to_string())
        );
    }

    #[test]
    fn resolver_prefers_process_environment_over_workspace_dotenv() {
        let workspace = tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".env"), "MYCORP_API_KEY=workspace-key\n").expect("write dotenv");

        with_override("MYCORP_API_KEY", Some("process-key"), || {
            let resolved = resolve_credential_with_mode(
                "mycorp",
                "MYCORP_API_KEY",
                Some(workspace.path()),
                crate::auth::AuthCredentialsStoreMode::File,
            )
            .expect("resolve credential")
            .expect("credential");
            assert_eq!(resolved.secret.as_deref(), Some("process-key"));
            assert_eq!(resolved.source, CredentialSource::Env);
            assert_eq!(resolved.identity.provider(), "mycorp");
            assert_eq!(resolved.identity.key_name(), "MYCORP_API_KEY");
        });
    }

    #[test]
    fn resolver_prefers_alternate_process_environment_over_primary_workspace_dotenv() {
        let workspace = tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".env"), "GEMINI_API_KEY=workspace-key\n").expect("write dotenv");

        with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("process-key"))], || {
            let resolved = resolve_credential_with_mode(
                "gemini",
                "GEMINI_API_KEY",
                Some(workspace.path()),
                crate::auth::AuthCredentialsStoreMode::File,
            )
            .expect("resolve credential")
            .expect("credential");
            assert_eq!(resolved.secret.as_deref(), Some("process-key"));
            assert_eq!(resolved.source, CredentialSource::Env);
            assert_eq!(resolved.env_var.as_deref(), Some("GOOGLE_API_KEY"));
        });
    }

    #[test]
    fn resolver_reads_workspace_dotenv_for_custom_provider_key() {
        let workspace = tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".env"), "MYCORP_BILLING_KEY=workspace-key\n").expect("write dotenv");

        with_override("MYCORP_BILLING_KEY", None, || {
            let resolved = resolve_credential_with_mode(
                "mycorp",
                "mycorp_billing_key",
                Some(workspace.path()),
                crate::auth::AuthCredentialsStoreMode::File,
            )
            .expect("resolve credential")
            .expect("credential");
            assert_eq!(resolved.secret.as_deref(), Some("workspace-key"));
            assert_eq!(resolved.source, CredentialSource::Workspace);
            assert_eq!(resolved.env_var.as_deref(), Some("MYCORP_BILLING_KEY"));
        });
    }

    #[test]
    fn resolver_does_not_reuse_legacy_storage_for_non_default_key() {
        with_override("MIMO_TOKEN_PLAN_KEY", None, || {
            let resolved = resolve_credential_with_mode(
                "mimo",
                "MIMO_TOKEN_PLAN_KEY",
                None,
                crate::auth::AuthCredentialsStoreMode::File,
            )
            .expect("resolve credential");
            assert!(resolved.is_none());
        });
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
    fn credential_detail_surfaces_meta_alternate_env_var_name() {
        with_overrides(&[("META_API_KEY", None), ("MODEL_API_KEY", Some("model-key"))], || {
            let detail = provider_credential_detail(Provider::Meta).expect("Meta discovered");
            assert_eq!(detail.source, CredentialSource::Env);
            assert_eq!(detail.env_var, Some("MODEL_API_KEY"));
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

    #[test]
    fn get_api_key_trims_non_empty_environment_values() {
        with_override("STEPFUN_API_KEY", Some("  test-stepfun-key  "), || {
            let result =
                get_api_key_with_mode("stepfun", &default_sources(), crate::auth::AuthCredentialsStoreMode::File);
            assert_eq!(result.unwrap(), "test-stepfun-key");
        });
    }
}
