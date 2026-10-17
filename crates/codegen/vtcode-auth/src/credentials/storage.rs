//! Generic credential storage that orchestrates the keyring and file backends.

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use std::fs;

use super::encryption;
use super::keyring;
use super::mode::AuthCredentialsStoreMode;
use super::mode::ResolvedStoreMode;
use crate::storage_paths::{auth_storage_dir, read_private_file, write_private_file};

/// Generic credential storage interface.
///
/// Provides methods to store, load, and clear credentials using either
/// the OS keyring or file-based storage.
pub struct CredentialStorage {
    service: String,
    user: String,
}

impl CredentialStorage {
    /// Create a new credential storage handle.
    pub(crate) fn new(service: impl Into<String>, user: impl Into<String>) -> Self {
        Self { service: service.into(), user: user.into() }
    }

    /// Store a credential using the specified mode.
    pub(crate) fn store_with_mode(&self, value: &str, mode: AuthCredentialsStoreMode) -> Result<()> {
        match mode.effective_mode() {
            ResolvedStoreMode::Keyring => match self.store_keyring(value) {
                Ok(()) => {
                    if let Err(err) = self.store_file(value) {
                        tracing::warn!(
                            "Failed to write encrypted file backup for {}/{}: {}",
                            self.service,
                            self.user,
                            err
                        );
                    }
                    Ok(())
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to store credential in OS keyring for {}/{}; falling back to encrypted file storage: {}",
                        self.service,
                        self.user,
                        err
                    );
                    self.store_file(value).context("failed to store credential in encrypted file")
                }
            },
            ResolvedStoreMode::File => self.store_file(value),
        }
    }

    /// Store a credential in exactly the selected backend.
    ///
    /// Provider-specific storage adapters use this boundary when their public
    /// API promises that an operation affects only the configured backend.
    /// Unlike [`Self::store_with_mode`], this method never falls back or writes
    /// a backup to another backend. It takes an already-resolved backend so no
    /// `Auto` case can reach it.
    pub(crate) fn store_exact_with_mode(&self, value: &str, mode: ResolvedStoreMode) -> Result<()> {
        match mode {
            ResolvedStoreMode::Keyring => self.store_keyring(value),
            ResolvedStoreMode::File => self.store_file(value),
        }
    }

    /// Store a serializable value using the shared credential backends.
    pub(crate) fn store_json<T: serde::Serialize>(&self, value: &T, mode: AuthCredentialsStoreMode) -> Result<()> {
        let serialized = serde_json::to_string(value).context("failed to serialize credential")?;
        self.store_with_mode(&serialized, mode)
    }

    /// Store a serializable value in exactly the selected backend.
    pub(crate) fn store_json_exact_with_mode<T: serde::Serialize>(
        &self,
        value: &T,
        mode: ResolvedStoreMode,
    ) -> Result<()> {
        let serialized = serde_json::to_string(value).context("failed to serialize credential")?;
        self.store_exact_with_mode(&serialized, mode)
    }

    /// Store a credential using `Auto` mode.
    pub fn store(&self, value: &str) -> Result<()> {
        self.store_with_mode(value, AuthCredentialsStoreMode::Auto)
    }

    /// Load a credential using the specified mode.
    pub(crate) fn load_with_mode(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        match mode.effective_mode() {
            ResolvedStoreMode::Keyring => match self.load_keyring() {
                Ok(Some(value)) => Ok(Some(value)),
                Ok(None) => self.load_file(),
                Err(err) => {
                    tracing::warn!(
                        "Failed to read credential from OS keyring for {}/{}; falling back to encrypted file storage: {}",
                        self.service,
                        self.user,
                        err
                    );
                    self.load_file()
                }
            },
            ResolvedStoreMode::File => self.load_file(),
        }
    }

    /// Load a credential from exactly the selected backend.
    ///
    /// This deliberately does not fall back to another backend. Callers that
    /// want a preferred-backend lookup must compose that policy explicitly.
    pub(crate) fn load_exact_with_mode(&self, mode: ResolvedStoreMode) -> Result<Option<String>> {
        match mode {
            ResolvedStoreMode::Keyring => self.load_keyring(),
            ResolvedStoreMode::File => self.load_file(),
        }
    }

    /// Load and deserialize a value from the shared credential backends.
    pub(crate) fn load_json<T: serde::de::DeserializeOwned>(
        &self,
        mode: AuthCredentialsStoreMode,
    ) -> Result<Option<T>> {
        let Some(serialized) = self.load_with_mode(mode)? else {
            return Ok(None);
        };
        serde_json::from_str(&serialized)
            .context("failed to deserialize credential")
            .map(Some)
    }

    /// Load and deserialize a value from exactly the selected backend.
    pub(crate) fn load_json_exact_with_mode<T: serde::de::DeserializeOwned>(
        &self,
        mode: ResolvedStoreMode,
    ) -> Result<Option<T>> {
        let Some(serialized) = self.load_exact_with_mode(mode)? else {
            return Ok(None);
        };
        serde_json::from_str(&serialized)
            .context("failed to deserialize credential")
            .map(Some)
    }

    /// Load a credential using `Auto` mode.
    pub fn load(&self) -> Result<Option<String>> {
        self.load_with_mode(AuthCredentialsStoreMode::Auto)
    }

    /// Clear (delete) a credential using the specified mode.
    pub(crate) fn clear_with_mode(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        match mode.effective_mode() {
            ResolvedStoreMode::Keyring => {
                let mut errors = Vec::new();

                if let Err(err) = self.clear_keyring() {
                    errors.push(err.to_string());
                }
                if let Err(err) = self.clear_file() {
                    errors.push(err.to_string());
                }

                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(anyhow!("Failed to clear credential from secure storage: {}", errors.join("; ")))
                }
            }
            ResolvedStoreMode::File => self.clear_file(),
        }
    }

    /// Clear a credential from exactly the selected backend.
    ///
    /// This is the deletion counterpart to [`Self::store_exact_with_mode`].
    /// It is intentionally separate from [`Self::clear_with_mode`], whose
    /// keyring branch also removes the encrypted backup written by the generic
    /// storage policy.
    pub(crate) fn clear_exact_with_mode(&self, mode: ResolvedStoreMode) -> Result<()> {
        match mode {
            ResolvedStoreMode::Keyring => self.clear_keyring(),
            ResolvedStoreMode::File => self.clear_file(),
        }
    }

    /// Clear a credential using `Auto` mode.
    pub fn clear(&self) -> Result<()> {
        self.clear_with_mode(AuthCredentialsStoreMode::Auto)
    }

    // ------------------------------------------------------------------
    // Private backend helpers
    // ------------------------------------------------------------------

    fn store_keyring(&self, value: &str) -> Result<()> {
        let entry = keyring::entry(&self.service, &self.user).context("Failed to access OS keyring")?;
        entry.set_password(value).context("Failed to store credential in OS keyring")?;
        tracing::debug!("Credential stored in OS keyring for {}/{}", self.service, self.user);
        Ok(())
    }

    fn load_keyring(&self) -> Result<Option<String>> {
        let entry = match keyring::entry(&self.service, &self.user) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };

        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow!("Failed to read from keyring: {e}")),
        }
    }

    fn clear_keyring(&self) -> Result<()> {
        let entry = match keyring::entry(&self.service, &self.user) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        match entry.delete_credential() {
            Ok(_) => {
                tracing::debug!("Credential cleared from keyring for {}/{}", self.service, self.user);
            }
            Err(keyring_core::Error::NoEntry) => {}
            Err(e) => return Err(anyhow!("Failed to clear keyring entry: {e}")),
        }

        Ok(())
    }

    fn store_file(&self, value: &str) -> Result<()> {
        let path = self.file_path()?;
        let encrypted = encryption::encrypt(value)?;
        let payload = serde_json::to_vec_pretty(&encrypted).context("failed to serialize encrypted credential")?;
        write_private_file(&path, &payload).context("failed to write encrypted credential file")?;
        Ok(())
    }

    fn load_file(&self) -> Result<Option<String>> {
        let path = self.file_path()?;
        let Some(data) = read_private_file(&path).context("failed to read encrypted credential file")? else {
            return Ok(None);
        };

        let encrypted: encryption::EncryptedCredential =
            serde_json::from_slice(&data).context("failed to decode encrypted credential file")?;
        encryption::decrypt(&encrypted).map(Some)
    }

    fn clear_file(&self) -> Result<()> {
        let path = self.file_path()?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(anyhow!("failed to delete encrypted credential file: {err}")),
        }
    }

    fn file_path(&self) -> Result<std::path::PathBuf> {
        use sha2::Digest as _;

        let mut hasher = sha2::Sha256::new();
        hasher.update(self.service.as_bytes());
        hasher.update([0]);
        hasher.update(self.user.as_bytes());
        let digest = hasher.finalize();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

        Ok(auth_storage_dir()?.join(format!("credential_{encoded}.json")))
    }

    #[cfg(test)]
    pub(crate) fn file_path_for_tests(&self) -> Result<std::path::PathBuf> {
        self.file_path()
    }
}
