//! Credential storage — keyring and encrypted-file backends.
//!
//! # Module Structure
//!
//! | Submodule | Responsibility |
//! |---|---|
//! | mode | Backend selection enum (`Keyring` / `File` / `Auto`) |
//! | keyring | OS keyring creation, liveness, disable detection |
//! | encryption | AES-256-GCM encrypt/decrypt (pure, no IO) |
//! | storage | `CredentialStorage` — orchestrates backends |
//! | legacy | Legacy `auth.json` migration |

pub(crate) mod encryption;
pub(crate) mod keyring;
pub(crate) mod legacy;
pub(crate) mod mode;
pub(crate) mod storage;

pub use mode::AuthCredentialsStoreMode;
pub(crate) use mode::ResolvedStoreMode;
pub use storage::CredentialStorage;

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

/// Validated identity for a stored provider credential.
///
/// Provider names are normalized to lowercase and environment-variable names
/// are normalized to uppercase. Keeping both dimensions in a value object
/// prevents a credential for one provider profile from being reused for
/// another profile that happens to share the provider name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialIdentity {
    provider: String,
    key_name: String,
}

impl CredentialIdentity {
    /// Create a credential identity from a provider name and environment
    /// variable-style key name.
    pub fn new(provider: &str, key_name: &str) -> Result<Self> {
        let provider = provider.trim().to_ascii_lowercase();
        if provider.is_empty() || !provider.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
            bail!("credential provider must contain only letters, digits, '-' or '_'");
        }

        let key_name = key_name.trim().to_ascii_uppercase();
        let mut chars = key_name.chars();
        let valid_start = chars.next().is_some_and(|ch| ch.is_ascii_uppercase() || ch == '_');
        if !valid_start || !chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_') {
            bail!("credential key name must be a valid environment variable name");
        }

        Ok(Self { provider, key_name })
    }

    /// Normalized provider name.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Normalized credential key name.
    pub fn key_name(&self) -> &str {
        &self.key_name
    }

    /// Whether this identity uses the provider's default key name.
    pub fn uses_default_key_name(&self, default_key_name: &str) -> bool {
        self.key_name.eq_ignore_ascii_case(default_key_name.trim())
    }
}

/// Custom API Key storage for provider-specific keys.
///
/// Provides secure storage and retrieval of API keys for custom providers
/// using the OS keyring or encrypted file storage.
pub struct CustomApiKeyStorage {
    provider: String,
    identity: Option<CredentialIdentity>,
    storage: CredentialStorage,
}

impl CustomApiKeyStorage {
    /// Create a provider-only legacy storage handle.
    ///
    /// New callers should use [`Self::for_provider_key`]. This constructor is
    /// retained so older integrations can still read and clear the legacy
    /// `api_key_<provider>` entry.
    pub fn new(provider: &str) -> Self {
        let normalized_provider = provider.trim().to_lowercase();
        Self {
            provider: normalized_provider.clone(),
            identity: None,
            storage: CredentialStorage::new("vtcode", format!("api_key_{normalized_provider}")),
        }
    }

    /// Create storage scoped to a provider and credential key name.
    pub fn for_provider_key(provider: &str, key_name: &str) -> Result<Self> {
        Self::for_identity(CredentialIdentity::new(provider, key_name)?)
    }

    /// Create storage scoped to a validated credential identity.
    pub fn for_identity(identity: CredentialIdentity) -> Result<Self> {
        let provider = identity.provider().to_owned();
        let user = format!("api_key_{}_{}", identity.provider(), identity.key_name());
        Ok(Self {
            provider,
            identity: Some(identity),
            storage: CredentialStorage::new("vtcode", user),
        })
    }

    /// Return the identity for a key-scoped handle, or `None` for a legacy
    /// provider-only handle.
    pub fn identity(&self) -> Option<&CredentialIdentity> {
        self.identity.as_ref()
    }

    /// Store an API key securely.
    pub fn store(&self, api_key: &str, mode: AuthCredentialsStoreMode) -> Result<()> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            bail!("API key cannot be empty");
        }
        self.store_value(api_key, mode)?;
        Ok(())
    }

    /// Retrieve a stored API key.
    pub fn load(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        if let Some(key) = self.storage.load_with_mode(mode)? {
            let key = key.trim();
            return Ok((!key.is_empty()).then(|| key.to_owned()));
        }

        if self.identity.is_none() {
            self.load_legacy_auth_json(mode)
        } else {
            Ok(None)
        }
    }

    /// Load a key-scoped credential and, when explicitly allowed, lazily
    /// migrate the provider-only legacy entry into this identity.
    pub fn load_with_legacy_fallback(
        &self,
        mode: AuthCredentialsStoreMode,
        allow_legacy: bool,
    ) -> Result<Option<String>> {
        if let Some(key) = self.load(mode)? {
            return Ok(Some(key));
        }
        if !allow_legacy || self.identity.is_none() {
            return Ok(None);
        }

        let legacy_storage = Self::new(&self.provider);
        let Some(key) = legacy_storage.load(mode)? else {
            return Ok(None);
        };

        self.store_value(&key, mode)
            .context("failed to migrate provider-only credential to key-scoped storage")?;
        // This is the obsolete provider-only secure-storage entry, not the
        // rollback-safe plaintext file under the legacy directory. Remove the
        // former after the key-scoped copy succeeds, but preserve the latter.
        legacy_storage
            .storage
            .clear_with_mode(mode)
            .context("failed to remove provider-only credential after migration")?;
        self.load(mode)
    }

    /// Clear (delete) a stored API key.
    pub fn clear(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.storage.clear_with_mode(mode)?;
        if self.identity.is_none() {
            legacy::clear_for_provider(&self.provider).context("failed to remove legacy plaintext credential")?;
        }
        Ok(())
    }

    /// Clear this key-scoped credential and optionally its provider-only
    /// legacy fallback.
    pub fn clear_with_legacy_fallback(&self, mode: AuthCredentialsStoreMode, clear_legacy: bool) -> Result<()> {
        self.clear(mode)?;
        if clear_legacy && self.identity.is_some() {
            Self::new(&self.provider).clear(mode)?;
        }
        Ok(())
    }

    fn store_value(&self, api_key: &str, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.storage.store_with_mode(api_key, mode)?;
        let persisted = self
            .storage
            .load_with_mode(mode)
            .context("failed to verify persisted API key")?;
        if persisted.as_deref().map(str::trim) != Some(api_key) {
            bail!("secure storage did not return the API key after saving");
        }
        Ok(())
    }

    fn load_legacy_auth_json(&self, mode: AuthCredentialsStoreMode) -> Result<Option<String>> {
        let Some(legacy_entry) = legacy::load_for_provider(&self.provider)? else {
            return Ok(None);
        };

        if let Err(err) = self.store(&legacy_entry.credentials.api_key, mode) {
            tracing::warn!(
                "Failed to migrate legacy plaintext auth.json entry for provider '{}' into secure storage: {}",
                self.provider,
                err
            );
            return Err(err).context("failed to migrate legacy API key into secure storage");
        }

        tracing::warn!(self.provider);
        self.load(mode)
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
                let _ignored = results.insert(provider.clone(), true);
            }
            Err(e) => {
                tracing::warn!("Failed to migrate API key for provider '{provider}': {e}");
                let _ignored = results.insert(provider.clone(), false);
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
            drop(api_keys.insert(provider.clone(), key));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct TestAuthDirGuard {
        temp_dir: Option<TempDir>,
        previous: Option<PathBuf>,
    }

    impl TestAuthDirGuard {
        fn new() -> Self {
            let temp_dir = TempDir::new().expect("create temp auth dir");
            let previous = crate::storage_paths::auth_storage_dir_override_for_tests().expect("read auth dir override");
            crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(temp_dir.path().to_path_buf()))
                .expect("set temp auth dir override");
            Self { temp_dir: Some(temp_dir), previous }
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
    fn credential_identity_normalizes_provider_and_key_name() {
        let identity = CredentialIdentity::new(" MiMo ", "mimo_token_plan_key").expect("valid identity");

        assert_eq!(identity.provider(), "mimo");
        assert_eq!(identity.key_name(), "MIMO_TOKEN_PLAN_KEY");
        assert!(identity.uses_default_key_name("MIMO_TOKEN_PLAN_KEY"));
        assert!(!identity.uses_default_key_name("MIMO_API_KEY"));
    }

    #[test]
    fn credential_identity_rejects_invalid_names() {
        assert!(CredentialIdentity::new("my corp", "MYCORP_API_KEY").is_err());
        assert!(CredentialIdentity::new("mycorp", "MY-CORP-API-KEY").is_err());
        assert!(CredentialIdentity::new("mycorp", "1MYCORP_API_KEY").is_err());
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn file_api_key_storage_round_trips_with_private_permissions() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let guard = TestAuthDirGuard::new();
        let storage = CustomApiKeyStorage::new("stepfun");
        storage
            .store("test-stepfun-key", AuthCredentialsStoreMode::File)
            .expect("store API key");
        assert_eq!(
            storage.load(AuthCredentialsStoreMode::File).expect("load API key").as_deref(),
            Some("test-stepfun-key")
        );

        let auth_dir = guard.temp_dir.as_ref().expect("test auth dir").path();
        assert_eq!(fs::metadata(auth_dir).expect("auth dir metadata").permissions().mode() & 0o777, 0o700);
        let credential_file = fs::read_dir(auth_dir)
            .expect("read auth dir")
            .map(|entry| entry.expect("credential entry").path())
            .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("json"))
            .expect("credential file");
        assert_eq!(fs::metadata(credential_file).expect("credential metadata").permissions().mode() & 0o777, 0o600);
    }

    #[test]
    #[serial]
    fn keyring_mode_falls_back_to_encrypted_file_when_keyring_is_unavailable() {
        let _guard = TestAuthDirGuard::new();
        let storage = CustomApiKeyStorage::new("stepfun");

        storage
            .store("test-stepfun-key", AuthCredentialsStoreMode::Keyring)
            .expect("keyring mode should fall back to encrypted file storage");
        assert_eq!(
            storage
                .load(AuthCredentialsStoreMode::Keyring)
                .expect("load API key")
                .as_deref(),
            Some("test-stepfun-key")
        );
    }

    #[test]
    #[serial]
    fn key_scoped_storage_keeps_provider_profiles_isolated() {
        let _guard = TestAuthDirGuard::new();
        let payg = CustomApiKeyStorage::for_provider_key(" MiMo ", "mimo_api_key").expect("payg storage");
        let token_plan =
            CustomApiKeyStorage::for_provider_key("mimo", "MIMO_TOKEN_PLAN_KEY").expect("token-plan storage");

        payg.store("sk-payg", AuthCredentialsStoreMode::File).expect("store payg");
        token_plan
            .store("tp-token-plan", AuthCredentialsStoreMode::File)
            .expect("store token plan");

        assert_eq!(payg.load(AuthCredentialsStoreMode::File).expect("load payg").as_deref(), Some("sk-payg"));
        assert_eq!(
            token_plan
                .load(AuthCredentialsStoreMode::File)
                .expect("load token plan")
                .as_deref(),
            Some("tp-token-plan")
        );
    }

    #[test]
    #[serial]
    fn default_identity_lazily_migrates_provider_only_storage() {
        let _guard = TestAuthDirGuard::new();
        let legacy = CustomApiKeyStorage::new("mimo");
        let target = CustomApiKeyStorage::for_provider_key("mimo", "MIMO_API_KEY").expect("target storage");

        legacy
            .store("legacy-key", AuthCredentialsStoreMode::File)
            .expect("store legacy key");
        assert_eq!(
            target
                .load_with_legacy_fallback(AuthCredentialsStoreMode::File, true)
                .expect("migrate legacy key")
                .as_deref(),
            Some("legacy-key")
        );
        assert_eq!(legacy.load(AuthCredentialsStoreMode::File).expect("legacy should be cleared"), None);
        assert_eq!(
            target
                .load(AuthCredentialsStoreMode::File)
                .expect("load migrated key")
                .as_deref(),
            Some("legacy-key")
        );
    }

    #[test]
    #[serial]
    fn non_default_identity_does_not_reuse_provider_only_storage() {
        let _guard = TestAuthDirGuard::new();
        let legacy = CustomApiKeyStorage::new("mimo");
        let token_plan =
            CustomApiKeyStorage::for_provider_key("mimo", "MIMO_TOKEN_PLAN_KEY").expect("token-plan storage");

        legacy
            .store("legacy-payg", AuthCredentialsStoreMode::File)
            .expect("store legacy key");
        assert_eq!(
            token_plan
                .load_with_legacy_fallback(AuthCredentialsStoreMode::File, false)
                .expect("load token-plan key"),
            None
        );
        assert_eq!(
            legacy.load(AuthCredentialsStoreMode::File).expect("legacy remains"),
            Some("legacy-payg".to_string())
        );
    }

    #[test]
    #[serial]
    fn clearing_one_identity_does_not_clear_another() {
        let _guard = TestAuthDirGuard::new();
        let payg = CustomApiKeyStorage::for_provider_key("mimo", "MIMO_API_KEY").expect("payg storage");
        let token_plan =
            CustomApiKeyStorage::for_provider_key("mimo", "MIMO_TOKEN_PLAN_KEY").expect("token-plan storage");

        payg.store("sk-payg", AuthCredentialsStoreMode::File).expect("store payg");
        token_plan
            .store("tp-token-plan", AuthCredentialsStoreMode::File)
            .expect("store token plan");
        payg.clear_with_legacy_fallback(AuthCredentialsStoreMode::File, true)
            .expect("clear payg");

        assert_eq!(payg.load(AuthCredentialsStoreMode::File).expect("payg cleared"), None);
        assert_eq!(
            token_plan
                .load(AuthCredentialsStoreMode::File)
                .expect("token plan remains")
                .as_deref(),
            Some("tp-token-plan")
        );
    }
}
