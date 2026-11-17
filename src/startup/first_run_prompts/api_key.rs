use anyhow::{Context, Result};
use vtcode_config::api_keys::{CredentialSource, store_credential_with_mode};
use vtcode_config::auth::AuthCredentialsStoreMode;
use vtcode_core::config::models::Provider;
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};

use super::common::prompt_with_placeholder;
use super::secret_input::{mask_key, read_secret_line};

/// Configure the API key for `provider` during first-run setup.
///
/// Resolution (no paste prompt unless everything else misses):
/// 1. Local provider → no key required.
/// 2. Managed-auth provider → defer to its CLI (`vtcode login <provider>`).
/// 3. Credential already discoverable in the process environment (shell export
///    or loaded `.env`) or in secure storage / OAuth → confirm and skip. When
///    a key is already in secure storage, the user may choose to replace it.
/// 4. Otherwise → prompt to paste. The pasted key is read with terminal echo
///    disabled (so it does not appear in scrollback), shown back as a masked
///    preview for confirmation, and stored in secure storage via
///    the central credential resolver/storage boundary (with encrypted-file
///    fallback) — **not** the workspace `.env`. Whitespace paste mistakes
///    re-prompt instead of silently skipping. The env var name is surfaced as
///    the preferred, no-duplication alternative.
pub(crate) fn prompt_api_key_interactive(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
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
    let detail = vtcode_config::api_keys::provider_credential_detail_with_mode(provider, storage_mode);

    // Discovery intentionally returns a compact `Option` for UI summaries. A
    // launch-time prompt must still distinguish "no key" from an unreadable
    // persisted key, otherwise a transient backend/decryption failure looks
    // like a request to paste a replacement secret.
    if detail.is_none() {
        vtcode_config::api_keys::load_stored_api_key_with_mode(provider.as_ref(), storage_mode)
            .with_context(|| format!("failed to read stored {} credential", provider.label()))?;
    }

    // Already have a credential — confirm and skip the paste prompt, unless
    // the user wants to replace a stored keyring key.
    if let Some(discovered) = detail {
        match discovered.source {
            CredentialSource::Env => {
                let var_name = discovered.env_var.unwrap_or(env_key);
                renderer.line(MessageStyle::Status, &format!("Found {var_name} in your environment — using it."))?;
                renderer
                    .line(MessageStyle::Info, "No key was written anywhere — switch providers anytime with /model.")?;
                return Ok(());
            }
            CredentialSource::Workspace => {
                let var_name = discovered.env_var.unwrap_or(env_key);
                renderer.line(MessageStyle::Status, &format!("Found {var_name} in the workspace .env — using it."))?;
                renderer
                    .line(MessageStyle::Info, "No key was written anywhere — switch providers anytime with /model.")?;
                return Ok(());
            }
            CredentialSource::SecureStorage => {
                renderer.line(
                    MessageStyle::Status,
                    &format!("Found a stored {} key in secure storage.", provider.label()),
                )?;
                if ask_replace_stored_key(renderer)? {
                    // Fall through to the paste flow to overwrite the stored key.
                    // (`prompt_paste_flow` prints its own header, so no extra
                    // "replace" banner is needed here.)
                } else {
                    renderer.line(
                        MessageStyle::Info,
                        "Using the stored key. Switch providers or clear it anytime with /model.",
                    )?;
                    return Ok(());
                }
            }
            CredentialSource::OAuth => {
                renderer
                    .line(MessageStyle::Status, &format!("Using your active {} OAuth session.", provider.label()))?;
                return Ok(());
            }
            CredentialSource::ManagedAuth | CredentialSource::Local => {
                // Handled above; unreachable but kept exhaustive.
                renderer.line(MessageStyle::Status, &format!("{} is ready.", provider.label()))?;
                return Ok(());
            }
        }
    }

    // No credential yet (or the user chose to replace a stored one) — prompt
    // to paste. Loop so a paste mistake re-prompts instead of dead-ending.
    prompt_paste_flow(renderer, provider, env_key, storage_mode)
}

/// Ask whether to replace an existing keyring key. Returns `true` for
/// "replace", `false` for "use the stored key". Defaults to "use" (Enter).
fn ask_replace_stored_key(renderer: &mut AnsiRenderer) -> Result<bool> {
    renderer.line(MessageStyle::Info, "Use the stored key (Enter) or replace it (r)?")?;
    loop {
        let input = prompt_with_placeholder("[Enter=use / r=replace]")?;
        let trimmed = input.trim().to_ascii_lowercase();
        match trimmed.as_str() {
            "" | "u" | "use" => return Ok(false),
            "r" | "replace" => return Ok(true),
            other => {
                renderer.line(
                    MessageStyle::Warning,
                    &format!("Unrecognized choice `{other}`. Press Enter to use the stored key, or `r` to replace it."),
                )?;
            }
        }
    }
}

/// Echo-off paste → validate → masked confirm → store in secure storage.
///
/// Loops on validation failure or a "don't save" confirmation so the user can
/// re-enter without restarting the wizard. Exits on: empty input (skip),
/// successful save, or `SetupInterrupted` (Ctrl-C, propagated as an error).
fn prompt_paste_flow(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    env_key: &'static str,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
    renderer.line(MessageStyle::Status, &format!("Set up your {} API key (env: {env_key}).", provider.label()))?;
    renderer.line(
        MessageStyle::Info,
        "Paste your API key now (input is hidden), or press Enter to skip. You can also add it later with `/secret`.",
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Tip: export {env_key} in your shell (e.g. ~/.zshrc) to skip this in every workspace — no duplication needed."
        ),
    )?;

    loop {
        // The prompt itself is printed by `read_secret_line` on the same line
        // it reads from, so the user sees `OpenRouter API key: ********`.
        let prompt = format!("{} API key: ", provider.label());
        let input = read_secret_line(&prompt)?;

        let Some(key) = input else {
            // Empty submit → explicit skip.
            renderer.line(
                MessageStyle::Info,
                &format!("Skipped. Set {env_key} in your environment, or paste it later with /model."),
            )?;
            return Ok(());
        };

        // Basic validation: reject keys with internal whitespace (a common
        // paste mistake — e.g. a trailing newline that didn't get trimmed, or
        // a copied key with a space in the middle). Re-prompt rather than skip.
        if key.chars().any(|c| c.is_whitespace()) {
            renderer.line(
                MessageStyle::Warning,
                "API key contains whitespace characters — this is likely a paste error.",
            )?;
            renderer.line(MessageStyle::Info, "Please re-enter the key without spaces, or press Enter to skip.")?;
            continue;
        }

        // Masked preview + confirm before saving, so the user can catch a
        // wrong/truncated paste without the full key being shown.
        if !confirm_save(renderer, provider, &key)? {
            renderer.line(MessageStyle::Info, "Discarded. Re-enter the key, or press Enter to skip.")?;
            continue;
        }

        // Store in the configured secure backend (the auth layer handles any
        // platform-specific keyring or encrypted-file fallback).
        store_credential_with_mode(provider.as_ref(), env_key, &key, storage_mode)
            .map_err(|e| anyhow::anyhow!("Failed to store {} API key securely: {e}", provider.label()))?;

        renderer.line(MessageStyle::Status, "API key saved to secure storage (not workspace environment files).")?;
        renderer.line(
            MessageStyle::Info,
            &format!("To use the env var instead, export {env_key} in your shell and clear this key with /model."),
        )?;
        return Ok(());
    }
}

/// Show a masked preview and ask the user to confirm before storing. Returns
/// `true` to save, `false` to discard and re-enter. Defaults to save (Enter).
fn confirm_save(renderer: &mut AnsiRenderer, provider: Provider, key: &str) -> Result<bool> {
    renderer.line(
        MessageStyle::Info,
        &format!("{} key received: {} — length {} chars.", provider.label(), mask_key(key), key.chars().count()),
    )?;
    renderer.line(MessageStyle::Info, "Save to secure storage? [Y]es (Enter) / [n]o / re-enter [r]")?;
    loop {
        let input = prompt_with_placeholder("[Enter=yes / n=no / r=re-enter]")?;
        let trimmed = input.trim().to_ascii_lowercase();
        match trimmed.as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            "r" | "re-enter" | "reenter" => return Ok(false),
            other => {
                renderer.line(
                    MessageStyle::Warning,
                    &format!("Unrecognized choice `{other}`. Press Enter to save, `n` to discard, or `r` to re-enter."),
                )?;
            }
        }
    }
}
