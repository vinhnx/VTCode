//! Storage backend selection for credentials.

/// Preferred storage backend for credentials.
///
/// - `Keyring`: Use OS-specific secure storage (macOS Keychain, Windows Credential Manager,
///   Linux Secret Service). This is the default as it's the most secure option.
/// - `File`: Use AES-256-GCM encrypted file (requires the `file-storage` feature or
///   custom implementation)
/// - `Auto`: Try keyring first, fall back to file if unavailable
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Platform-aware default:
    ///
    /// - **macOS**: `File` — AES-256-GCM encrypted file with machine-derived key.
    ///   Avoids macOS Keychain authorization popups that trigger on every new
    ///   binary (including each release update). Users can opt into `Keyring`
    ///   via `credential_storage_mode` in `vtcode.toml`.
    ///
    /// - **Linux / Windows / others**: `Auto` — try OS keyring (Secret Service
    ///   / Windows Credential Manager, no popups), fall back to encrypted file.
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::File
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::Auto
        }
    }
}

impl AuthCredentialsStoreMode {
    /// Resolve `Auto` to the best available concrete backend.
    /// `Keyring` and `File` pass through unchanged.
    pub fn effective_mode(self) -> Self {
        match self {
            Self::Auto => {
                if super::keyring::is_functional() {
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
