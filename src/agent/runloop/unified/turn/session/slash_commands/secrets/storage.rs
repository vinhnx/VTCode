//! Strict storage boundary for the interactive secret manager.
//!
//! UI code should depend on [`SecretStorage`] rather than constructing auth
//! backends directly. This keeps provider validation, normalization, and the
//! configured backend selection in one independently testable interface.

use anyhow::{Context, Result, bail};
use std::path::Path;
use vtcode_auth::AuthCredentialsStoreMode;
use vtcode_config::api_keys::{
    clear_credential_with_mode, load_stored_credential_with_mode, resolve_credential_with_mode,
    store_credential_with_mode,
};

pub(super) struct SecretStorage {
    mode: AuthCredentialsStoreMode,
}

impl SecretStorage {
    pub(super) fn new(mode: AuthCredentialsStoreMode) -> Self {
        Self { mode }
    }

    pub(super) fn load_resolved(&self, provider: &str, key_name: &str, workspace: &Path) -> Result<Option<String>> {
        resolve_credential_with_mode(provider, key_name, Some(workspace), self.mode)
            .with_context(|| format!("failed to resolve {provider} credential"))
            .map(|resolved| resolved.and_then(|credential| credential.secret))
    }

    pub(super) fn load_stored(&self, provider: &str, key_name: &str) -> Result<Option<String>> {
        load_stored_credential_with_mode(provider, key_name, self.mode)
            .with_context(|| format!("failed to load stored {provider} credential"))
    }

    pub(super) fn store(&self, provider: &str, key_name: &str, value: &str) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            bail!("API key cannot be empty");
        }

        store_credential_with_mode(provider, key_name, value, self.mode)
            .with_context(|| format!("failed to store {provider} credential"))
            .map(|_| ())
    }

    pub(super) fn clear(&self, provider: &str, key_name: &str) -> Result<()> {
        clear_credential_with_mode(provider, key_name, self.mode)
            .with_context(|| format!("failed to clear {provider} credential"))
            .map(|_| ())
    }
}
