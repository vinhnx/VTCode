//! Credential storage — keyring and encrypted-file backends.
//!
//! # Module Structure
//!
//! | Submodule | Responsibility |
//! |---|---|
//! | [`mode`] | Backend selection enum (`Keyring` / `File` / `Auto`) |
//! | [`keyring`] | OS keyring creation, liveness, disable detection |
//! | [`encryption`] | AES-256-GCM encrypt/decrypt (pure, no IO) |
//! | [`storage`] | `CredentialStorage` — orchestrates backends |
//! | [`legacy`] | Legacy `auth.json` migration |

mod encryption;
pub(crate) mod keyring;
mod legacy;
mod mode;
mod storage;

pub use mode::AuthCredentialsStoreMode;
pub use storage::CredentialStorage;

use std::collections::BTreeMap;

use anyhow::Result;

/// Custom API Key storage for provider-specific keys.
///
/// Provides secure storage and retrieval of API keys for custom providers
/// using the OS keyring or encrypted file storage.
pub struct CustomApiKeyStorage {
    provider: String,
    storage: CredentialStorage,
}

impl CustomApiKeyStorage {
    /// Create a new custom API key storage for a specific provider.
    pub fn new(provider: &str) -> Self {
        let normalized_provider = provider.to_lowercase();
        Self {
            provider: normalized_provider.clone(),
            storage: CredentialStorage::new("vtcode", format!("api_key_{normalized_provider}")),
        }
    }

    /// Store an API key securely.
    pub fn store(&self, api_key: &str, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.storage.store_with_mode(api_key, mode)?;
        let _ = legacy::clear_for_provider(&self.provider);
        Ok(())
    }

    /// Retrieve a stored API key.
    pub fn load(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        if let Some(key) = self.storage.load_with_mode(mode)? {
            return Ok(Some(key));
        }

        self.load_legacy_auth_json(mode)
    }

    /// Clear (delete) a stored API key.
    pub fn clear(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.storage.clear_with_mode(mode)?;
        let _ = legacy::clear_for_provider(&self.provider);
        Ok(())
    }

    fn load_legacy_auth_json(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        let Some(legacy_entry) = legacy::load_for_provider(&self.provider)? else {
            return Ok(None);
        };

        if let Err(err) = self.storage.store_with_mode(&legacy_entry.api_key, mode) {
            tracing::warn!(
                "Failed to migrate legacy plaintext auth.json entry for provider '{}' into secure storage: {}",
                self.provider,
                err
            );
            return Ok(Some(legacy_entry.api_key));
        }

        let path = crate::storage_paths::legacy_auth_storage_path().ok();
        if let Some(p) = path {
            let _ = legacy::delete_file(&p);
        }

        tracing::warn!(
            "Migrated legacy plaintext auth.json entry for provider '{}' into secure storage",
            self.provider
        );
        Ok(Some(legacy_entry.api_key))
    }
}

/// Migrate plain-text API keys from a config map into secure storage.
///
/// Returns a map of provider → success/failure.
pub fn migrate_custom_api_keys(
    custom_api_keys: &BTreeMap<String, String>,
    mode: AuthCredentialsStoreMode,
) -> Result<BTreeMap<String, bool>> {
    let mut results = BTreeMap::new();

    for (provider, api_key) in custom_api_keys {
        let storage = CustomApiKeyStorage::new(provider);
        match storage.store(api_key, mode) {
            Ok(()) => {
                tracing::info!("Migrated API key for provider '{provider}' to secure storage");
                results.insert(provider.clone(), true);
            }
            Err(e) => {
                tracing::warn!("Failed to migrate API key for provider '{provider}': {e}");
                results.insert(provider.clone(), false);
            }
        }
    }

    Ok(results)
}

/// Load all custom API keys from secure storage.
///
/// Returns a map of provider → API key for those that have stored keys.
pub fn load_custom_api_keys(providers: &[String], mode: AuthCredentialsStoreMode) -> Result<BTreeMap<String, String>> {
    let mut api_keys = BTreeMap::new();

    for provider in providers {
        let storage = CustomApiKeyStorage::new(provider);
        if let Some(key) = storage.load(mode)? {
            api_keys.insert(provider.clone(), key);
        }
    }

    Ok(api_keys)
}

/// Clear all custom API keys from secure storage.
pub fn clear_custom_api_keys(providers: &[String], mode: AuthCredentialsStoreMode) -> Result<()> {
    for provider in providers {
        let storage = CustomApiKeyStorage::new(provider);
        if let Err(e) = storage.clear(mode) {
            tracing::warn!("Failed to clear API key for provider '{provider}': {e}");
        }
    }
    Ok(())
}
