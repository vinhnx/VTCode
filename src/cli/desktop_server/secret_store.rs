//! `config/secret/store`: persist a provider API key from the desktop client.
//!
//! Contract: the key is written through the shared secure-storage boundary
//! (OS keyring or encrypted file) and never echoed back — responses and
//! errors carry provider labels only. After a successful store the handler
//! re-resolves the active provider so the desktop reflects the new state
//! without a service restart; when the save repairs the active provider, the
//! stale session runtime is discarded so the next session authenticates.

use super::*;

/// Maximum accepted API-key length in characters. Provider keys are short
/// opaque tokens; the bound keeps pastes from abusing the 1 MiB frame budget.
const MAX_SECRET_CHARS: usize = 8 * 1024;

/// Maximum accepted provider identifier length in characters.
const MAX_PROVIDER_CHARS: usize = 128;

/// In-app secret-form support for a snapshot: present only when the provider
/// takes a static API key through the `vtcode secret add` route.
pub(super) fn snapshot_info(config: &vtcode_config::VTCodeConfig, provider: &str) -> Option<protocol::SecretStoreInfo> {
    let provider = provider.trim();
    if provider.is_empty() || crate::cli::auth::supports_auth_provider(provider) {
        return None;
    }
    match crate::cli::secret::secret_target(config, provider, None) {
        Ok(target) if !target.local && !target.managed_auth && !target.env_key.is_empty() => {
            Some(protocol::SecretStoreInfo {
                provider: provider.to_string(),
                env_var: target.env_key,
            })
        }
        Ok(_) => None,
        // Unknown provider (typo or unconfigured custom): still offer the
        // form so the service can report the precise resolution failure.
        Err(_) => Some(protocol::SecretStoreInfo {
            provider: provider.to_string(),
            env_var: vtcode_config::api_keys::api_key_env_var(provider),
        }),
    }
}

/// Store a desktop-submitted API key and refresh authentication state.
///
/// Errors name the provider only; the key never appears in errors, logs, or
/// the response. Saves for an inactive provider persist the key and leave
/// live state untouched. When the save repairs the active provider, the stale
/// session runtime is discarded (`session_reset`) so the next session starts
/// with working credentials; rotations while authenticated keep the session.
pub(super) async fn store(
    context: &ServiceContext,
    params: protocol::SecretStoreParams,
) -> Result<protocol::SecretStoreResult> {
    ensure_no_active_turn(context).await?;
    let provider = params.provider.trim().to_string();
    if provider.is_empty() {
        bail!("Provider must not be empty.");
    }
    if provider.chars().count() > MAX_PROVIDER_CHARS {
        bail!("Provider identifier is too long.");
    }
    // Pastes routinely carry surrounding whitespace; the CLI trims piped
    // input the same way before storing.
    let key = params.api_key.trim().to_string();
    if key.is_empty() {
        bail!("API key must not be empty.");
    }
    if key.chars().count() > MAX_SECRET_CHARS {
        bail!("API key exceeds the 8 KiB limit.");
    }

    let target = crate::cli::secret::secret_target(&context.startup.config, &provider, None)?;
    if target.local {
        bail!("{} does not use an API key.", target.label);
    }
    if target.managed_auth || target.env_key.is_empty() {
        if crate::cli::auth::supports_auth_provider(&target.provider_name) {
            bail!(
                "{} does not use a static API key. Run `vtcode login {}` in Terminal instead.",
                target.label,
                target.provider_name
            );
        }
        bail!("{} does not use a static API key. Use its configured authentication flow instead.", target.label);
    }

    let storage_mode = context.startup.config.agent.credential_storage_mode;
    vtcode_config::api_keys::store_credential_with_mode(&target.provider_name, &target.env_key, &key, storage_mode)
        .with_context(|| format!("Failed to store the {} API key securely.", target.label))?;

    // Purge a stale workspace `.env` entry so it cannot shadow the secure
    // entry; mirrors `vtcode secret add`.
    if vtcode_config::workspace_env::read_workspace_env_value(&context.workspace, &target.env_key)?.is_some() {
        vtcode_config::workspace_env::remove_workspace_env_value(&context.workspace, &target.env_key)?;
    }

    let agent = active_agent_config(context).await;
    let before = configuration_snapshot(context, &agent).await?;
    if !target.provider_name.eq_ignore_ascii_case(&agent.provider) {
        // Saved for an inactive provider: future selections re-resolve from
        // storage on their own; leave live state untouched.
        return Ok(protocol::SecretStoreResult { configuration: before, session_reset: false });
    }
    // Re-resolve with the stored identity's key name: the boot config may
    // override the env var, and an override's `api_key_env` trails the boot
    // provider, so neither is guaranteed to name the stored credential.
    let auth = catalog::resolve_override_auth(context, &agent.provider, &agent.model, &target.env_key).await;
    let repaired = auth.error.is_none() && !before.authenticated;
    {
        let mut state = context.state.lock().await;
        if state.model_override.is_some() {
            state.model_override_auth = Some(auth);
        } else {
            state.secret_store_auth = Some(catalog::StoredSecretAuth { provider: target.provider_name.clone(), auth });
        }
    }
    let session_reset = if repaired {
        shutdown_current_session(context, false).await?;
        context.state.lock().await.session_id = None;
        true
    } else {
        false
    };
    let agent = active_agent_config(context).await;
    let configuration = configuration_snapshot(context, &agent).await?;
    Ok(protocol::SecretStoreResult { configuration, session_reset })
}
