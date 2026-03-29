//! Generic credential storage with OS keyring and file-based backends.
//!
//! This module provides a unified interface for storing sensitive credentials
//! securely using the OS keyring (macOS Keychain, Windows Credential Manager,
//! Linux Secret Service) with fallback to AES-256-GCM encrypted files.
//!
//! ## Usage
//!
//! ```rust
//! use vtcode_auth::{AuthCredentialsStoreMode, CredentialStorage};
//!
//! # fn example() -> anyhow::Result<()> {
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
//! # Ok(())
//! # }
//! ```

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

use crate::storage_paths::auth_storage_dir;
use crate::storage_paths::legacy_auth_storage_path;

const ENCRYPTED_CREDENTIAL_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedCredential {
    nonce: String,
    ciphertext: String,
    version: u8,
}

#[derive(Debug, Deserialize)]
struct LegacyAuthFile {
    mode: String,
    provider: String,
    api_key: String,
}

/// Preferred storage backend for credentials.
///
/// - `Keyring`: Use OS-specific secure storage (macOS Keychain, Windows Credential Manager,
///   Linux Secret Service). This is the default as it's the most secure option.
/// - `File`: Use AES-256-GCM encrypted file (requires the `file-storage` feature or
///   custom implementation)
/// - `Auto`: Try keyring first, fall back to file if unavailable
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
            AuthCredentialsStoreMode::Keyring => match self.store_keyring(value) {
                Ok(()) => {
                    let _ = self.clear_file();
                    Ok(())
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to store credential in OS keyring for {}/{}; falling back to encrypted file storage: {}",
                        self.service,
                        self.user,
                        err
                    );
                    self.store_file(value)
                        .context("failed to store credential in encrypted file")
                }
            },
            AuthCredentialsStoreMode::File => self.store_file(value),
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
            AuthCredentialsStoreMode::Keyring => match self.load_keyring() {
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
            AuthCredentialsStoreMode::File => self.load_file(),
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
            AuthCredentialsStoreMode::Keyring => {
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
                    Err(anyhow!(
                        "Failed to clear credential from secure storage: {}",
                        errors.join("; ")
                    ))
                }
            }
            AuthCredentialsStoreMode::File => self.clear_file(),
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

    fn store_file(&self, value: &str) -> Result<()> {
        let path = self.file_path()?;
        let encrypted = encrypt_credential(value)?;
        let payload = serde_json::to_vec_pretty(&encrypted)
            .context("failed to serialize encrypted credential")?;
        fs::write(&path, payload).context("failed to write encrypted credential file")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .context("failed to set credential file permissions")?;
        }

        Ok(())
    }

    fn load_file(&self) -> Result<Option<String>> {
        let path = self.file_path()?;
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(anyhow!("failed to read encrypted credential file: {err}")),
        };

        let encrypted: EncryptedCredential =
            serde_json::from_slice(&data).context("failed to decode encrypted credential file")?;
        decrypt_credential(&encrypted).map(Some)
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
}

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
    ///
    /// # Arguments
    /// * `provider` - The provider identifier (e.g., "openrouter", "anthropic", "custom_provider")
    pub fn new(provider: &str) -> Self {
        let normalized_provider = provider.to_lowercase();
        Self {
            provider: normalized_provider.clone(),
            storage: CredentialStorage::new("vtcode", format!("api_key_{normalized_provider}")),
        }
    }

    /// Store an API key securely.
    ///
    /// # Arguments
    /// * `api_key` - The API key value to store
    /// * `mode` - The storage mode to use (defaults to keyring)
    pub fn store(&self, api_key: &str, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.storage.store_with_mode(api_key, mode)?;
        clear_legacy_auth_file_if_matches(&self.provider)?;
        Ok(())
    }

    /// Retrieve a stored API key.
    ///
    /// Returns `None` if no key is stored.
    pub fn load(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        if let Some(key) = self.storage.load_with_mode(mode)? {
            return Ok(Some(key));
        }

        self.load_legacy_auth_json(mode)
    }

    /// Clear (delete) a stored API key.
    pub fn clear(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.storage.clear_with_mode(mode)?;
        clear_legacy_auth_file_if_matches(&self.provider)?;
        Ok(())
    }

    fn load_legacy_auth_json(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        let Some(legacy) = load_legacy_auth_file_for_provider(&self.provider)? else {
            return Ok(None);
        };

        if let Err(err) = self.storage.store_with_mode(&legacy.api_key, mode) {
            tracing::warn!(
                "Failed to migrate legacy plaintext auth.json entry for provider '{}' into secure storage: {}",
                self.provider,
                err
            );
            return Ok(Some(legacy.api_key));
        }

        clear_legacy_auth_file_if_matches(&self.provider)?;
        tracing::warn!(
            "Migrated legacy plaintext auth.json entry for provider '{}' into secure storage",
            self.provider
        );
        Ok(Some(legacy.api_key))
    }
}

fn encrypt_credential(value: &str) -> Result<EncryptedCredential> {
    let key = derive_file_encryption_key()?;
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("failed to generate credential nonce"))?;

    let mut ciphertext = value.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce_bytes),
        Aad::empty(),
        &mut ciphertext,
    )
    .map_err(|_| anyhow!("failed to encrypt credential"))?;

    Ok(EncryptedCredential {
        nonce: STANDARD.encode(nonce_bytes),
        ciphertext: STANDARD.encode(ciphertext),
        version: ENCRYPTED_CREDENTIAL_VERSION,
    })
}

fn decrypt_credential(encrypted: &EncryptedCredential) -> Result<String> {
    if encrypted.version != ENCRYPTED_CREDENTIAL_VERSION {
        return Err(anyhow!("unsupported encrypted credential format"));
    }

    let nonce_bytes = STANDARD
        .decode(&encrypted.nonce)
        .context("failed to decode credential nonce")?;
    let nonce_array: [u8; NONCE_LEN] = nonce_bytes
        .try_into()
        .map_err(|_| anyhow!("invalid credential nonce length"))?;
    let mut ciphertext = STANDARD
        .decode(&encrypted.ciphertext)
        .context("failed to decode credential ciphertext")?;

    let key = derive_file_encryption_key()?;
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_array),
            Aad::empty(),
            &mut ciphertext,
        )
        .map_err(|_| anyhow!("failed to decrypt credential"))?;

    String::from_utf8(plaintext.to_vec()).context("failed to parse decrypted credential")
}

fn derive_file_encryption_key() -> Result<LessSafeKey> {
    use ring::digest::SHA256;
    use ring::digest::digest;

    let mut key_material = Vec::new();
    if let Ok(hostname) = hostname::get() {
        key_material.extend_from_slice(hostname.as_encoded_bytes());
    }

    #[cfg(unix)]
    {
        key_material.extend_from_slice(&nix::unistd::getuid().as_raw().to_le_bytes());
    }
    #[cfg(not(unix))]
    {
        if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
            key_material.extend_from_slice(user.as_bytes());
        }
    }

    key_material.extend_from_slice(b"vtcode-credentials-v1");

    let hash = digest(&SHA256, &key_material);
    let key_bytes: &[u8; 32] = hash.as_ref()[..32]
        .try_into()
        .context("credential encryption key was too short")?;
    let unbound = UnboundKey::new(&aead::AES_256_GCM, key_bytes)
        .map_err(|_| anyhow!("invalid credential encryption key"))?;
    Ok(LessSafeKey::new(unbound))
}

fn load_legacy_auth_file_for_provider(provider: &str) -> Result<Option<LegacyAuthFile>> {
    let path = legacy_auth_storage_path()?;
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(anyhow!("failed to read legacy auth file: {err}")),
    };

    let legacy: LegacyAuthFile =
        serde_json::from_slice(&data).context("failed to parse legacy auth file")?;
    let matches_provider = legacy.provider.eq_ignore_ascii_case(provider);
    let stores_api_key = legacy.mode.eq_ignore_ascii_case("api_key");
    let has_key = !legacy.api_key.trim().is_empty();

    if matches_provider && stores_api_key && has_key {
        Ok(Some(legacy))
    } else {
        Ok(None)
    }
}

fn clear_legacy_auth_file_if_matches(provider: &str) -> Result<()> {
    let path = legacy_auth_storage_path()?;
    let Some(_legacy) = load_legacy_auth_file_for_provider(provider)? else {
        return Ok(());
    };

    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(anyhow!("failed to delete legacy auth file: {err}")),
    }
}

/// Migrate plain-text API keys from config to secure storage.
///
/// This function reads API keys from the provided BTreeMap and stores them
/// securely using the specified storage mode. After migration, the keys
/// should be removed from the config file.
///
/// # Arguments
/// * `custom_api_keys` - Map of provider names to API keys (from config)
/// * `mode` - The storage mode to use
///
/// # Returns
/// A map of providers that were successfully migrated (for tracking purposes)
pub fn migrate_custom_api_keys_to_keyring(
    custom_api_keys: &BTreeMap<String, String>,
    mode: AuthCredentialsStoreMode,
) -> Result<BTreeMap<String, bool>> {
    let mut migration_results = BTreeMap::new();

    for (provider, api_key) in custom_api_keys {
        let storage = CustomApiKeyStorage::new(provider);
        match storage.store(api_key, mode) {
            Ok(()) => {
                tracing::info!(
                    "Migrated API key for provider '{}' to secure storage",
                    provider
                );
                migration_results.insert(provider.clone(), true);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to migrate API key for provider '{}': {}",
                    provider,
                    e
                );
                migration_results.insert(provider.clone(), false);
            }
        }
    }

    Ok(migration_results)
}

/// Load all custom API keys from secure storage.
///
/// This function retrieves API keys for all providers that have keys stored.
///
/// # Arguments
/// * `providers` - List of provider names to check for stored keys
/// * `mode` - The storage mode to use
///
/// # Returns
/// A BTreeMap of provider names to their API keys (only includes providers with stored keys)
pub fn load_custom_api_keys(
    providers: &[String],
    mode: AuthCredentialsStoreMode,
) -> Result<BTreeMap<String, String>> {
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
///
/// # Arguments
/// * `providers` - List of provider names to clear
/// * `mode` - The storage mode to use
pub fn clear_custom_api_keys(providers: &[String], mode: AuthCredentialsStoreMode) -> Result<()> {
    for provider in providers {
        let storage = CustomApiKeyStorage::new(provider);
        if let Err(e) = storage.clear(mode) {
            tracing::warn!("Failed to clear API key for provider '{}': {}", provider, e);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use serial_test::serial;

    struct TestAuthDirGuard {
        temp_dir: Option<TempDir>,
        previous: Option<std::path::PathBuf>,
    }

    impl TestAuthDirGuard {
        fn new() -> Self {
            let temp_dir = TempDir::new().expect("create temp auth dir");
            let previous = crate::storage_paths::auth_storage_dir_override_for_tests()
                .expect("read auth dir override");
            crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(
                temp_dir.path().to_path_buf(),
            ))
            .expect("set auth dir override");

            Self {
                temp_dir: Some(temp_dir),
                previous,
            }
        }
    }

    impl Drop for TestAuthDirGuard {
        fn drop(&mut self) {
            crate::storage_paths::set_auth_storage_dir_override_for_tests(self.previous.clone())
                .expect("restore auth dir override");
            if let Some(temp_dir) = self.temp_dir.take() {
                temp_dir.close().expect("remove temp auth dir");
            }
        }
    }

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

    #[test]
    #[serial]
    fn credential_storage_file_mode_round_trips_without_plaintext() {
        let _guard = TestAuthDirGuard::new();
        let storage = CredentialStorage::new("vtcode", "test_key");

        storage
            .store_with_mode("secret_api_key", AuthCredentialsStoreMode::File)
            .expect("store encrypted credential");

        let loaded = storage
            .load_with_mode(AuthCredentialsStoreMode::File)
            .expect("load encrypted credential");
        assert_eq!(loaded.as_deref(), Some("secret_api_key"));

        let stored = fs::read_to_string(storage.file_path().expect("credential path"))
            .expect("read encrypted credential file");
        assert!(!stored.contains("secret_api_key"));
    }

    #[test]
    #[serial]
    fn keyring_mode_load_falls_back_to_encrypted_file() {
        let _guard = TestAuthDirGuard::new();
        let storage = CredentialStorage::new("vtcode", "test_key");

        storage
            .store_with_mode("secret_api_key", AuthCredentialsStoreMode::File)
            .expect("store encrypted credential");

        let loaded = storage
            .load_with_mode(AuthCredentialsStoreMode::Keyring)
            .expect("load credential");
        assert_eq!(loaded.as_deref(), Some("secret_api_key"));
    }

    #[test]
    #[serial]
    fn custom_api_key_load_migrates_legacy_auth_json() {
        let _guard = TestAuthDirGuard::new();
        let legacy_path = legacy_auth_storage_path().expect("legacy auth path");
        fs::write(
            &legacy_path,
            r#"{
  "version": 1,
  "mode": "api_key",
  "provider": "openai",
  "api_key": "legacy-secret",
  "authenticated_at": 1768406185
}"#,
        )
        .expect("write legacy auth file");

        let storage = CustomApiKeyStorage::new("openai");
        let loaded = storage
            .load(AuthCredentialsStoreMode::File)
            .expect("load migrated api key");
        assert_eq!(loaded.as_deref(), Some("legacy-secret"));
        assert!(!legacy_path.exists());

        let encrypted = fs::read_to_string(storage.storage.file_path().expect("credential path"))
            .expect("read migrated credential file");
        assert!(!encrypted.contains("legacy-secret"));
    }
}
