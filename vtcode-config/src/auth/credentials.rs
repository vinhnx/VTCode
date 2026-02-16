//! Generic credential storage with OS keyring and file-based backends.
//!
//! This module provides a unified interface for storing sensitive credentials
//! securely using the OS keyring (macOS Keychain, Windows Credential Manager,
//! Linux Secret Service) with fallback to AES-256-GCM encrypted files.
//!
//! ## Usage
//!
//! ```rust
//! use vtcode_config::auth::credentials::{CredentialStorage, AuthCredentialsStoreMode};
//!
//! // Store a credential using the default mode (keyring)
//! let storage = CredentialStorage::new("my_app", "api_key");
//! storage.store("secret_api_key")?;
//!
//! // Retrieve the credential
//! if let Some(value) = storage.load()? {
//!     println!("Found credential: {}", value);
//! }
//!
//! // Delete the credential
//! storage.clear()?;
//! ```

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

/// Preferred storage backend for credentials.
///
/// - `Keyring`: Use OS-specific secure storage (macOS Keychain, Windows Credential Manager,
///   Linux Secret Service). This is the default as it's the most secure option.
/// - `File`: Use AES-256-GCM encrypted file (requires the `file-storage` feature or
///   custom implementation)
/// - `Auto`: Try keyring first, fall back to file if unavailable
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthCredentialsStoreMode {
    /// Use OS-specific keyring service.
    /// This is the most secure option as credentials are managed by the OS
    /// and are not accessible to other users or applications.
    Keyring,
    /// Persist credentials in an encrypted file.
    /// The file is encrypted with AES-256-GCM using a machine-derived key.
    File,
    /// Use keyring when available; otherwise, fall back to file.
    Auto,
}

impl Default for AuthCredentialsStoreMode {
    /// Default to keyring on all platforms for maximum security.
    /// Falls back to file-based storage if keyring is unavailable.
    fn default() -> Self {
        Self::Keyring
    }
}

impl AuthCredentialsStoreMode {
    /// Get the effective storage mode, resolving Auto to the best available option.
    pub fn effective_mode(self) -> Self {
        match self {
            Self::Auto => {
                // Check if keyring is functional by attempting to create an entry
                if is_keyring_functional() {
                    Self::Keyring
                } else {
                    tracing::debug!("Keyring not available, falling back to file storage");
                    Self::File
                }
            }
            mode => mode,
        }
    }
}

/// Check if the OS keyring is functional by attempting a test operation.
///
/// This creates a test entry, verifies it can be written and read, then deletes it.
/// This is more reliable than just checking if Entry creation succeeds.
pub(crate) fn is_keyring_functional() -> bool {
    // Create a test entry with a unique name to avoid conflicts
    let test_user = format!("test_{}", std::process::id());
    let entry = match keyring::Entry::new("vtcode", &test_user) {
        Ok(e) => e,
        Err(_) => return false,
    };

    // Try to write a test value
    if entry.set_password("test").is_err() {
        return false;
    }

    // Try to read it back
    let functional = entry.get_password().is_ok();

    // Clean up - ignore errors during cleanup
    let _ = entry.delete_credential();

    functional
}

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
    ///
    /// # Arguments
    /// * `service` - The service name (e.g., "vtcode", "openrouter", "github")
    /// * `user` - The user/account identifier (e.g., "api_key", "oauth_token")
    pub fn new(service: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            user: user.into(),
        }
    }

    /// Store a credential using the specified mode.
    ///
    /// # Arguments
    /// * `value` - The credential value to store
    /// * `mode` - The storage mode to use
    pub fn store_with_mode(&self, value: &str, mode: AuthCredentialsStoreMode) -> Result<()> {
        match mode.effective_mode() {
            AuthCredentialsStoreMode::Keyring => self.store_keyring(value),
            AuthCredentialsStoreMode::File => Err(anyhow!(
                "File storage requires the file_storage feature or custom implementation"
            )),
            _ => unreachable!(),
        }
    }

    /// Store a credential using the default mode (keyring).
    pub fn store(&self, value: &str) -> Result<()> {
        self.store_keyring(value)
    }

    /// Store credential in OS keyring.
    fn store_keyring(&self, value: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, &self.user)
            .context("Failed to access OS keyring")?;

        entry
            .set_password(value)
            .context("Failed to store credential in OS keyring")?;

        tracing::debug!(
            "Credential stored in OS keyring for {}/{}",
            self.service,
            self.user
        );
        Ok(())
    }

    /// Load a credential using the specified mode.
    ///
    /// Returns `None` if no credential exists.
    pub fn load_with_mode(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        match mode.effective_mode() {
            AuthCredentialsStoreMode::Keyring => self.load_keyring(),
            AuthCredentialsStoreMode::File => Err(anyhow!(
                "File storage requires the file_storage feature or custom implementation"
            )),
            _ => unreachable!(),
        }
    }

    /// Load a credential using the default mode (keyring).
    ///
    /// Returns `None` if no credential exists.
    pub fn load(&self) -> Result<Option<String>> {
        self.load_keyring()
    }

    /// Load credential from OS keyring.
    fn load_keyring(&self) -> Result<Option<String>> {
        let entry = match keyring::Entry::new(&self.service, &self.user) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };

        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow!("Failed to read from keyring: {}", e)),
        }
    }

    /// Clear (delete) a credential using the specified mode.
    pub fn clear_with_mode(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        match mode.effective_mode() {
            AuthCredentialsStoreMode::Keyring => self.clear_keyring(),
            AuthCredentialsStoreMode::File => Ok(()), // File storage not implemented here
            _ => unreachable!(),
        }
    }

    /// Clear (delete) a credential using the default mode.
    pub fn clear(&self) -> Result<()> {
        self.clear_keyring()
    }

    /// Clear credential from OS keyring.
    fn clear_keyring(&self) -> Result<()> {
        let entry = match keyring::Entry::new(&self.service, &self.user) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        match entry.delete_credential() {
            Ok(_) => {
                tracing::debug!(
                    "Credential cleared from keyring for {}/{}",
                    self.service,
                    self.user
                );
            }
            Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(anyhow!("Failed to clear keyring entry: {}", e)),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_mode_default_is_keyring() {
        assert_eq!(
            AuthCredentialsStoreMode::default(),
            AuthCredentialsStoreMode::Keyring
        );
    }

    #[test]
    fn test_storage_mode_effective_mode() {
        assert_eq!(
            AuthCredentialsStoreMode::Keyring.effective_mode(),
            AuthCredentialsStoreMode::Keyring
        );
        assert_eq!(
            AuthCredentialsStoreMode::File.effective_mode(),
            AuthCredentialsStoreMode::File
        );

        // Auto should resolve to either Keyring or File
        let auto_mode = AuthCredentialsStoreMode::Auto.effective_mode();
        assert!(
            auto_mode == AuthCredentialsStoreMode::Keyring
                || auto_mode == AuthCredentialsStoreMode::File
        );
    }

    #[test]
    fn test_storage_mode_serialization() {
        let keyring_json = serde_json::to_string(&AuthCredentialsStoreMode::Keyring).unwrap();
        assert_eq!(keyring_json, "\"keyring\"");

        let file_json = serde_json::to_string(&AuthCredentialsStoreMode::File).unwrap();
        assert_eq!(file_json, "\"file\"");

        let auto_json = serde_json::to_string(&AuthCredentialsStoreMode::Auto).unwrap();
        assert_eq!(auto_json, "\"auto\"");

        // Test deserialization
        let parsed: AuthCredentialsStoreMode = serde_json::from_str("\"keyring\"").unwrap();
        assert_eq!(parsed, AuthCredentialsStoreMode::Keyring);

        let parsed: AuthCredentialsStoreMode = serde_json::from_str("\"file\"").unwrap();
        assert_eq!(parsed, AuthCredentialsStoreMode::File);

        let parsed: AuthCredentialsStoreMode = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(parsed, AuthCredentialsStoreMode::Auto);
    }

    #[test]
    fn test_credential_storage_new() {
        let storage = CredentialStorage::new("vtcode", "test_key");
        assert_eq!(storage.service, "vtcode");
        assert_eq!(storage.user, "test_key");
    }

    #[test]
    fn test_is_keyring_functional_check() {
        // This test just verifies the function doesn't panic
        // The actual result depends on the OS environment
        let _functional = is_keyring_functional();
    }
}
