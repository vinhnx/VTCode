use std::io::{self, Write};

use anyhow::Result;
use vtcode_config::api_keys::{CredentialSource, provider_credential_source};
use vtcode_config::auth::{AuthCredentialsStoreMode, CustomApiKeyStorage};
use vtcode_core::config::models::Provider;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Configure the API key for `provider` during first-run setup.
///
/// Resolution (no paste prompt unless everything else misses):
/// 1. Local provider → no key required.
/// 2. Managed-auth provider → defer to its CLI (`vtcode login <provider>`).
/// 3. Credential already discoverable in the process environment (shell export
///    or loaded `.env`) or in secure storage → confirm and skip the prompt.
/// 4. Otherwise → prompt to paste; the pasted key is stored in the OS keyring
///    via `CustomApiKeyStorage` (with encrypted-file fallback), **not** the
///    workspace `.env`. The env var name is surfaced as the preferred,
///    no-duplication alternative.
pub(crate) fn prompt_api_key_interactive(renderer: &mut AnsiRenderer, provider: Provider) -> Result<()> {
    if provider.is_local() {
        renderer
            .line(MessageStyle::Info, &format!("No API key required for {} (local provider).", provider.label()))?;
        return Ok(());
    }

    if provider.uses_managed_auth() {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "{} auth is managed by its official CLI. Run `vtcode login {}` to authenticate.",
                provider.label(),
                provider
            ),
        )?;
        return Ok(());
    }

    let env_key = provider.default_api_key_env();
    let source = provider_credential_source(provider);

    // Already have a credential — confirm and skip the paste prompt.
    if let Some(discovered) = source {
        let description = match discovered {
            CredentialSource::Env => format!("Found {env_key} in your environment — using it."),
            CredentialSource::SecureStorage => format!("Using the {} key stored in your OS keyring.", provider.label()),
            CredentialSource::OAuth => format!("Using your active {} OAuth session.", provider.label()),
            CredentialSource::ManagedAuth | CredentialSource::Local => {
                // Handled above; unreachable but kept exhaustive.
                format!("{} is ready.", provider.label())
            }
        };
        renderer.line(MessageStyle::Status, &description)?;
        renderer.line(MessageStyle::Info, "No key was written anywhere — switch providers anytime with /model.")?;
        return Ok(());
    }

    // No credential yet — prompt to paste. Store in the OS keyring, not .env.
    renderer.line(MessageStyle::Status, &format!("Set up your {} API key (env: {env_key}).", provider.label()))?;
    renderer.line(
        MessageStyle::Info,
        "Paste your API key now, or press Enter to skip (you can set it later with /model).",
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Tip: export {env_key} in your shell (e.g. ~/.zshrc) to skip this in every workspace — no duplication, no .env file."
        ),
    )?;

    print!("{} API key: ", provider.label());
    io::stdout()
        .flush()
        .map_err(|e| anyhow::anyhow!("Failed to flush prompt: {e}"))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| anyhow::anyhow!("Failed to read API key input: {e}"))?;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        renderer.line(
            MessageStyle::Info,
            &format!("Skipped. Set {env_key} in your environment, or paste it later with /model."),
        )?;
        return Ok(());
    }

    // Basic validation: reject keys with internal whitespace (common paste mistake).
    if trimmed.chars().any(|c| c.is_whitespace()) {
        renderer
            .line(MessageStyle::Warning, "API key contains whitespace characters -- this is likely a paste error.")?;
        renderer
            .line(MessageStyle::Info, "Please re-enter the key without spaces or newlines, or press Enter to skip.")?;
        return Ok(());
    }

    // Store in the OS keyring (encrypted-file fallback is handled by the auth layer).
    let storage = CustomApiKeyStorage::new(provider.as_ref());
    let mode = AuthCredentialsStoreMode::default();
    storage
        .store(trimmed, mode)
        .map_err(|e| anyhow::anyhow!("Failed to store {} API key in OS keyring: {e}", provider.label()))?;

    renderer.line(MessageStyle::Info, "API key saved to your OS keyring (not workspace .env).")?;
    renderer.line(
        MessageStyle::Info,
        &format!("To use the env var instead, export {env_key} in your shell and clear this key with /model."),
    )?;

    Ok(())
}
