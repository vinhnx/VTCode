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
    ///
    /// Follows the "parse, don't validate" pattern: the return type is
    /// [`ResolvedStoreMode`], which has no `Auto` variant, so downstream
    /// code matches exhaustively and never needs an `unreachable!` arm.
    pub(crate) fn effective_mode(self) -> ResolvedStoreMode {
        match self {
            Self::Auto => {
                if super::keyring::is_functional() {
                    ResolvedStoreMode::Keyring
                } else {
                    tracing::debug!("Keyring not available, falling back to file storage");
                    ResolvedStoreMode::File
                }
            }
            Self::Keyring => ResolvedStoreMode::Keyring,
            Self::File => ResolvedStoreMode::File,
        }
    }
}

/// A concrete credential backend, with [`AuthCredentialsStoreMode::Auto`] resolved.
///
/// [`AuthCredentialsStoreMode::effective_mode`] parses the possibly-`Auto`
/// configuration into this type once; every consumer then handles both
/// backends exhaustively without a runtime `unreachable!` guard.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedStoreMode {
    /// Use the OS-specific keyring service.
    Keyring,
    /// Persist credentials in an encrypted file.
    File,
}

#[cfg(test)]
mod tests {
    use super::{AuthCredentialsStoreMode, ResolvedStoreMode};

    #[test]
    fn explicit_modes_pass_through_unchanged() {
        assert_eq!(AuthCredentialsStoreMode::Keyring.effective_mode(), ResolvedStoreMode::Keyring);
        assert_eq!(AuthCredentialsStoreMode::File.effective_mode(), ResolvedStoreMode::File);
    }

    #[test]
    fn auto_resolves_to_a_concrete_backend() {
        // `Auto` must never survive resolution: the match is exhaustive over
        // the concrete variants, so there is no `Auto` case to assert against.
        match AuthCredentialsStoreMode::Auto.effective_mode() {
            ResolvedStoreMode::Keyring | ResolvedStoreMode::File => {}
        }
    }
}
