//! In-app API-key storage (`config/secret/store`) types.
//!
//! The desktop client collects a provider API key through a masked input and
//! sends it to the local service over the supervised stdio channel. The
//! service persists it through the shared secure-storage boundary (OS keyring
//! or encrypted file) and never echoes it back: responses and errors carry
//! provider labels only.

use serde::{Deserialize, Serialize};

use super::ConfigurationSnapshot;

/// Parameters for storing a provider API key from the desktop client.
///
/// The secret travels the local stdio channel only. Callers must never log,
/// persist, or re-render the key; the service stores it via the platform
/// secure-storage backend and reports provider labels on failure.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStoreParams {
    /// Provider identifier, such as `meta`.
    pub provider: String,
    /// API key to persist in secure storage. Never echoed back.
    pub api_key: String,
}

impl std::fmt::Debug for SecretStoreParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretStoreParams")
            .field("provider", &self.provider)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

/// Outcome of `config/secret/store`: the refreshed configuration plus whether
/// the service restarted the session runtime to pick up repaired credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStoreResult {
    /// Configuration snapshot reflecting post-save authentication state.
    pub configuration: ConfigurationSnapshot,
    /// True when the save repaired the active provider and the previous
    /// session runtime was discarded; the client must clear its selection.
    pub session_reset: bool,
}

/// In-app secret-form support advertised by the service for the active provider.
///
/// Present only when the provider takes a static API key (`vtcode secret add`
/// route). Absent for login-flow providers (OAuth/managed auth), local
/// providers, and older services that predate in-app secret storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStoreInfo {
    /// Provider the form stores a key for.
    pub provider: String,
    /// Expected environment variable, shown so users paste the right key.
    pub env_var: String,
}
