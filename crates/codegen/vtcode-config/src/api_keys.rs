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
    CredentialSource, ResolvedCredential, clear_credential_with_mode, get_api_key, get_api_key_in_workspace_with_mode,
    get_api_key_with_mode, load_stored_api_key_with_mode, load_stored_credential_with_mode, resolve_credential,
    resolve_credential_with_mode, resolve_openai_api_key_for_auth, store_credential_with_mode,
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

/// Retained for compatibility. Workspace credentials are read on demand and
/// are never imported into the process environment.
#[deprecated(since = "0.180.6", note = "workspace `.env` is never imported; this is a no-op")]
pub fn load_dotenv() -> Result<()> {
    Ok(())
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
    provider_credential_detail_in_workspace_with_mode(provider, None, storage_mode)
}

/// Workspace-aware [`provider_credential_detail_with_mode`]: pass the turn's
/// workspace so a workspace `.env` credential wins over secure storage, the
/// same order the turn's own resolution used. Auth-failure guidance needs
/// this to name the rejected key's actual source.
pub fn provider_credential_detail_in_workspace_with_mode(
    provider: Provider,
    workspace: Option<&std::path::Path>,
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
        resolve_credential_with_mode(provider.as_ref(), provider.default_api_key_env(), workspace, storage_mode)
            .ok()??;
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
mod tests;
