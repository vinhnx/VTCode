//! Credential resolution for a provider/key identity.
//!
//! This module owns the source precedence and secure-storage boundary. The
//! parent `api_keys` module re-exports this API as the stable public facade;
//! callers must not depend on this implementation module directly.

use anyhow::{Context, Result};
use std::path::Path;
use std::str::FromStr;

use crate::auth::{CredentialIdentity, CustomApiKeyStorage};
use crate::models::Provider;

use super::{ApiKeySources, alternate_env_var, api_key_env_var, credential_identity, read_env_var};

/// A resolved credential and the source that supplied it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCredential {
    /// Provider/key identity used for this lookup.
    pub identity: CredentialIdentity,
    /// Resolution source.
    pub source: CredentialSource,
    /// Secret material. OAuth sessions that do not expose an API key have no
    /// secret here but still report their source.
    pub secret: Option<String>,
    /// Environment variable name when the source is environment-backed.
    pub env_var: Option<String>,
}

/// Where a provider's credential was discovered.
///
/// Used by the first-run wizard and model picker to show why a provider is
/// ready without re-prompting for a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    /// Process environment variable — covers shell exports (e.g. `~/.zshrc`)
    /// and values loaded from a workspace `.env` by `load_dotenv()`.
    Env,
    /// Workspace `.env` file.
    Workspace,
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
            Self::Env => "found in environment",
            Self::Workspace => "found in workspace .env",
            Self::SecureStorage => "stored in secure storage",
            Self::OAuth => "OAuth session active",
            Self::ManagedAuth => "managed by external CLI",
            Self::Local => {
                if provider.is_local() {
                    "local — no key required"
                } else {
                    "ready"
                }
            }
        }
    }
}

/// Get an API key for a provider using the platform-default storage backend.
pub fn get_api_key(provider: &str, sources: &ApiKeySources) -> Result<String> {
    get_api_key_with_mode(provider, sources, crate::auth::AuthCredentialsStoreMode::default())
}

/// Get an API key using the configured secure-storage backend.
///
/// Environment variables remain the highest-priority source. Secure storage
/// is read with `storage_mode` so a key written to an explicitly configured
/// backend is resolved from that same backend on every startup.
pub fn get_api_key_with_mode(
    provider: &str,
    _sources: &ApiKeySources,
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Result<String> {
    let normalized_provider = provider.trim().to_lowercase();
    let inferred_env = api_key_env_var(&normalized_provider);

    // Local providers intentionally accept an empty key. Managed-auth
    // providers have their own login flows and must not be treated as API-key
    // providers.
    match normalized_provider.as_str() {
        "ollama" | "lmstudio" | "llamacpp" | "llama.cpp" | "llama-cpp" => {
            return Ok(read_env_var(&inferred_env)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_default());
        }
        "copilot" => {
            return Err(anyhow::anyhow!(
                "GitHub Copilot authentication is managed by the official `copilot` CLI. Run `vtcode login copilot`."
            ));
        }
        "codex" => {
            return Err(anyhow::anyhow!(
                "Codex authentication is managed by the official `codex app-server`. Run `vtcode login codex`."
            ));
        }
        _ => {}
    }

    if let Some(resolved) = resolve_credential_with_mode(&normalized_provider, &inferred_env, None, storage_mode)? {
        if let Some(secret) = resolved.secret {
            return Ok(secret);
        }

        // A ChatGPT OAuth session is a valid OpenAI credential, but it does
        // not provide an API key to callers that explicitly request API-key
        // authentication. Continue to the independent key-scoped API-key
        // entry instead of treating the OAuth marker as missing.
        if resolved.source == CredentialSource::OAuth
            && let Some(secret) = load_stored_api_key_with_mode(&normalized_provider, storage_mode)?
        {
            return Ok(secret);
        }
    }

    let message = match normalized_provider.as_str() {
        "gemini" => "GEMINI_API_KEY or GOOGLE_API_KEY not set".to_owned(),
        "qwen" => "QWEN_API_KEY or DASHSCOPE_API_KEY not set".to_owned(),
        "meta" => "META_API_KEY or MODEL_API_KEY not set".to_owned(),
        _ => format!(
            "{normalized_provider} API key not found. Export {inferred_env} in your shell, or store it with `/secret add {normalized_provider}` (it is kept in secure storage, not a workspace .env).",
        ),
    };
    Err(anyhow::anyhow!(message))
}

/// Store a provider/key credential in the configured secure-storage backend.
pub fn store_credential_with_mode(
    provider: &str,
    key_name: &str,
    secret: &str,
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Result<Option<CredentialIdentity>> {
    let Some(identity) = credential_identity(provider, key_name)? else {
        return Ok(None);
    };
    CustomApiKeyStorage::for_identity(identity.clone())?
        .store(secret, storage_mode)
        .with_context(|| {
            format!(
                "failed to persist credential for provider '{}' and key '{}' securely",
                identity.provider(),
                identity.key_name()
            )
        })?;
    Ok(Some(identity))
}

/// Clear a provider/key credential from secure storage.
///
/// Legacy provider-only storage is cleared only for the provider's default
/// key name. Non-default identities must never delete or reuse that legacy
/// entry because its profile is ambiguous.
pub fn clear_credential_with_mode(
    provider: &str,
    key_name: &str,
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Result<Option<CredentialIdentity>> {
    let Some(identity) = credential_identity(provider, key_name)? else {
        return Ok(None);
    };
    let default_key_name = api_key_env_var(provider);
    let clear_legacy = identity.uses_default_key_name(&default_key_name);
    CustomApiKeyStorage::for_identity(identity.clone())?
        .clear_with_legacy_fallback(storage_mode, clear_legacy)
        .with_context(|| {
            format!(
                "failed to clear credential for provider '{}' and key '{}' securely",
                identity.provider(),
                identity.key_name()
            )
        })?;
    Ok(Some(identity))
}

/// Resolve a credential using the platform-default secure-storage backend.
pub fn resolve_credential(
    provider: &str,
    key_name: &str,
    workspace: Option<&Path>,
) -> Result<Option<ResolvedCredential>> {
    resolve_credential_with_mode(provider, key_name, workspace, crate::auth::AuthCredentialsStoreMode::default())
}

/// Resolve a provider/key credential with explicit storage mode.
///
/// Precedence is process environment, workspace `.env`, provider OAuth, and
/// key-scoped secure storage. Provider-only storage is considered only when
/// `key_name` is equivalent to the provider's default environment variable;
/// such an entry is migrated lazily into the key-scoped namespace.
pub fn resolve_credential_with_mode(
    provider: &str,
    key_name: &str,
    workspace: Option<&Path>,
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Result<Option<ResolvedCredential>> {
    let normalized_provider = provider.trim().to_ascii_lowercase();
    let default_key_name = api_key_env_var(&normalized_provider);
    let requested_key_name = if key_name.trim().is_empty() {
        default_key_name.clone()
    } else {
        key_name.trim().to_owned()
    };
    if requested_key_name.is_empty() {
        return Ok(None);
    }

    let requested_identity = CredentialIdentity::new(&normalized_provider, &requested_key_name)?;
    let mut candidates = vec![requested_identity.clone()];
    if requested_identity.uses_default_key_name(&default_key_name)
        && let Ok(provider_enum) = Provider::from_str(&normalized_provider)
        && let Some(alternate) = alternate_env_var(provider_enum)
        && !alternate.eq_ignore_ascii_case(requested_identity.key_name())
    {
        candidates.push(CredentialIdentity::new(&normalized_provider, alternate)?);
    }

    for identity in &candidates {
        if let Some(secret) = read_env_var(identity.key_name()).filter(|value| !value.trim().is_empty()) {
            return Ok(Some(ResolvedCredential {
                identity: identity.clone(),
                source: CredentialSource::Env,
                secret: Some(secret.trim().to_owned()),
                env_var: Some(identity.key_name().to_owned()),
            }));
        }
    }

    for identity in &candidates {
        if let Some(workspace) = workspace
            && let Some(secret) = crate::workspace_env::read_workspace_env_value(workspace, identity.key_name())?
            && !secret.trim().is_empty()
        {
            return Ok(Some(ResolvedCredential {
                identity: identity.clone(),
                source: CredentialSource::Workspace,
                secret: Some(secret.trim().to_owned()),
                env_var: Some(identity.key_name().to_owned()),
            }));
        }
    }

    // Tests explicitly override an environment variable to model an unset
    // variable. Do not let host credentials make those tests depend on the
    // developer's keyring or home directory. This guard must run before the
    // OAuth branches below: a real ChatGPT/OpenRouter session on the host
    // would otherwise leak a credential into discovery even when the test
    // explicitly set the key to None.
    #[cfg(test)]
    if candidates
        .iter()
        .any(|identity| super::test_storage_lookup_is_overridden(identity.key_name()))
    {
        return Ok(None);
    }

    let uses_default_key = requested_identity.uses_default_key_name(&default_key_name);
    if uses_default_key {
        if normalized_provider == "openrouter"
            && let Some(token) = crate::auth::load_oauth_token_with_mode(storage_mode)?
        {
            return Ok(Some(ResolvedCredential {
                identity: requested_identity.clone(),
                source: CredentialSource::OAuth,
                secret: Some(token.api_key),
                env_var: None,
            }));
        }
        if normalized_provider == "openai"
            && crate::auth::load_openai_chatgpt_session_with_mode(storage_mode)?.is_some()
        {
            return Ok(Some(ResolvedCredential {
                identity: requested_identity.clone(),
                source: CredentialSource::OAuth,
                secret: None,
                env_var: None,
            }));
        }
    }

    let storage = CustomApiKeyStorage::for_identity(requested_identity.clone())?;
    if let Some(secret) = storage.load_with_legacy_fallback(storage_mode, uses_default_key)? {
        return Ok(Some(ResolvedCredential {
            identity: requested_identity,
            source: CredentialSource::SecureStorage,
            secret: Some(secret),
            env_var: None,
        }));
    }

    Ok(None)
}

/// Resolve the API-key input for OpenAI account authentication.
pub fn resolve_openai_api_key_for_auth(
    storage_mode: crate::auth::AuthCredentialsStoreMode,
    allow_chatgpt_fallback: bool,
) -> Result<Option<String>> {
    match get_api_key_with_mode("openai", &ApiKeySources::default(), storage_mode) {
        Ok(api_key) => Ok(Some(api_key)),
        Err(_err)
            if allow_chatgpt_fallback
                && crate::auth::load_openai_chatgpt_session_with_mode(storage_mode)?.is_some() =>
        {
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

/// Load a provider's default API key from secure storage only.
pub fn load_stored_api_key_with_mode(
    provider: &str,
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Result<Option<String>> {
    let key_name = api_key_env_var(provider);
    load_stored_credential_with_mode(provider, &key_name, storage_mode)
}

/// Load only the secure-storage value for a provider/key identity.
pub fn load_stored_credential_with_mode(
    provider: &str,
    key_name: &str,
    storage_mode: crate::auth::AuthCredentialsStoreMode,
) -> Result<Option<String>> {
    let Some(identity) = credential_identity(provider, key_name)? else {
        return Ok(None);
    };
    let default_key_name = api_key_env_var(provider);
    let allow_legacy = identity.uses_default_key_name(&default_key_name);
    let storage = CustomApiKeyStorage::for_identity(identity)?;
    // The auth layer handles keyring-to-file fallback internally when the
    // configured mode permits it. Provider-only storage is a legacy fallback
    // only for the provider's default key identity.
    storage
        .load_with_legacy_fallback(storage_mode, allow_legacy)
        .map(|value| value.filter(|key| !key.trim().is_empty()))
}
