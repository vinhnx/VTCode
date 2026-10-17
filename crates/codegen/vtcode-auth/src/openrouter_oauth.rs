//! OpenRouter OAuth PKCE authentication flow.
//!
//! This module implements the OAuth PKCE flow for OpenRouter, allowing users
//! to authenticate with their OpenRouter account securely.
//!
//! ## Security Model
//!
//! Tokens use the shared credential storage boundary: OS keyring when selected
//! and AES-256-GCM encrypted files as the fallback backend.
//!
//! ### Keyring Storage (Default)
//! Uses the platform-native credential store:
//! - **macOS**: Keychain (accessible only to the user)
//! - **Windows**: Credential Manager (encrypted with user's credentials)
//! - **Linux**: Secret Service API / libsecret (requires a keyring daemon)
//!
//! Existing `openrouter.json` files are decrypted and migrated when loaded.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fmt;

pub use super::credentials::AuthCredentialsStoreMode;
use super::pkce::PkceChallenge;
use crate::openrouter_token_storage::OpenRouterTokenStorage;
#[cfg(test)]
use crate::openrouter_token_storage::{
    decrypt_legacy_token as decrypt_token, encrypt_legacy_token as encrypt_token, legacy_token_path as get_token_path,
};

/// OpenRouter API endpoints
const OPENROUTER_AUTH_URL: &str = "https://openrouter.ai/auth";
const OPENROUTER_KEYS_URL: &str = "https://openrouter.ai/api/v1/auth/keys";

/// Default callback port for localhost OAuth server
const DEFAULT_CALLBACK_PORT: u16 = 8484;

/// Configuration for OpenRouter OAuth authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct OpenRouterOAuthConfig {
    /// Whether to use OAuth instead of API key
    use_oauth: bool,
    /// Port for the local callback server
    pub callback_port: u16,
    /// Whether to automatically refresh tokens
    auto_refresh: bool,
    /// Timeout in seconds for completing the OAuth browser flow.
    pub flow_timeout_secs: u64,
}

impl Default for OpenRouterOAuthConfig {
    fn default() -> Self {
        Self {
            use_oauth: false,
            callback_port: DEFAULT_CALLBACK_PORT,
            auto_refresh: true,
            flow_timeout_secs: 300,
        }
    }
}

/// Stored OAuth token with metadata.
#[derive(Clone, Serialize, Deserialize)]
pub struct OpenRouterToken {
    /// The API key obtained via OAuth
    pub api_key: String,
    /// When the token was obtained (Unix timestamp)
    pub obtained_at: u64,
    /// Optional expiry time (Unix timestamp)
    pub expires_at: Option<u64>,
    /// User-friendly label for the token
    pub label: Option<String>,
}

impl fmt::Debug for OpenRouterToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenRouterToken")
            .field("api_key", &"<redacted>")
            .field("obtained_at", &self.obtained_at)
            .field("expires_at", &self.expires_at)
            .field("label", &self.label)
            .finish()
    }
}

impl OpenRouterToken {
    /// Check if the token has expired.
    fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            now >= expires_at
        } else {
            false
        }
    }
}

/// Generate the OAuth authorization URL.
///
/// # Arguments
/// * `challenge` - PKCE challenge containing the code_challenge
/// * `callback_port` - Port for the localhost callback server
///
/// # Returns
/// The full authorization URL to redirect the user to.
pub fn get_auth_url(challenge: &PkceChallenge, callback_port: u16) -> String {
    let callback_url = format!("http://localhost:{callback_port}/callback");
    format!(
        "{}?callback_url={}&code_challenge={}&code_challenge_method={}",
        OPENROUTER_AUTH_URL,
        urlencoding::encode(&callback_url),
        urlencoding::encode(&challenge.code_challenge),
        challenge.code_challenge_method
    )
}

/// Exchange an authorization code for an API key.
///
/// This makes a POST request to OpenRouter's token endpoint with the
/// authorization code and PKCE verifier.
///
/// # Arguments
/// * `code` - The authorization code from the callback URL
/// * `challenge` - The PKCE challenge used during authorization
///
/// # Returns
/// The obtained API key on success.
pub async fn exchange_code_for_token(code: &str, challenge: &PkceChallenge) -> Result<String> {
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "code": code,
        "code_verifier": challenge.code_verifier,
        "code_challenge_method": challenge.code_challenge_method
    });

    let response = client
        .post(OPENROUTER_KEYS_URL)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .context("Failed to send token exchange request")?;

    let status = response.status();
    if !status.is_success() {
        // Never expose the raw response body: OAuth providers may echo codes,
        // tokens, or other sensitive diagnostics in an error payload.
        return Err(token_exchange_error(status));
    }

    // Parse the response to extract the key
    let body = response.text().await.context("Failed to read response body")?;
    let response_json: serde_json::Value = serde_json::from_str(&body).context("Failed to parse token response")?;

    let api_key = response_json
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Response missing 'key' field"))?
        .to_string();

    Ok(api_key)
}

fn token_exchange_error(status: reqwest::StatusCode) -> anyhow::Error {
    match status.as_u16() {
        400 => anyhow!("Invalid code_challenge_method. Ensure you're using the same method (S256) in both steps."),
        403 => anyhow!("Invalid code or code_verifier. The authorization code may have expired."),
        405 => anyhow!("Method not allowed. Ensure you're using POST over HTTPS."),
        _ => anyhow!("Token exchange failed (HTTP {status})"),
    }
}

/// Save an OAuth token to encrypted storage with specified mode.
///
/// # Arguments
/// * `token` - The OAuth token to save
/// * `mode` - The storage mode to use
pub fn save_oauth_token_with_mode(token: &OpenRouterToken, mode: AuthCredentialsStoreMode) -> Result<()> {
    OpenRouterTokenStorage::new().save(token, mode)
}

/// Save an OAuth token to encrypted storage using the default mode.
///
/// Uses the configured default credential storage mode.
pub fn save_oauth_token(token: &OpenRouterToken) -> Result<()> {
    save_oauth_token_with_mode(token, AuthCredentialsStoreMode::default())
}

/// Load an OAuth token from storage with specified mode.
///
/// Returns `None` if no token exists or the token has expired.
pub fn load_oauth_token_with_mode(mode: AuthCredentialsStoreMode) -> Result<Option<OpenRouterToken>> {
    let storage = OpenRouterTokenStorage::new();
    let Some(token) = storage.load(mode)? else {
        return Ok(None);
    };

    if token.is_expired() {
        tracing::warn!("OpenRouter OAuth token has expired, removing it");
        storage.clear(mode)?;
        return Ok(None);
    }

    Ok(Some(token))
}

/// Load an OAuth token from storage using the default mode.
///
/// This function checks the selected secure backend and migrates the legacy
/// encrypted file format when necessary.
pub fn load_oauth_token() -> Result<Option<OpenRouterToken>> {
    let storage = OpenRouterTokenStorage::new();
    for mode in [AuthCredentialsStoreMode::Keyring, AuthCredentialsStoreMode::File] {
        let Some(token) = storage.load(mode)? else {
            continue;
        };

        if token.is_expired() {
            tracing::warn!("OpenRouter OAuth token has expired, removing it");
            storage.clear(mode)?;
            continue;
        }

        return Ok(Some(token));
    }

    Ok(None)
}

/// Clear the stored OAuth token using the selected storage mode.
pub fn clear_oauth_token_with_mode(mode: AuthCredentialsStoreMode) -> Result<()> {
    OpenRouterTokenStorage::new().clear(mode)
}

/// Clear the token from both shared backends and the legacy file format.
pub fn clear_oauth_token() -> Result<()> {
    OpenRouterTokenStorage::new().clear_all()
}

/// Get the current OAuth authentication status.
pub fn get_auth_status_with_mode(mode: AuthCredentialsStoreMode) -> Result<AuthStatus> {
    match load_oauth_token_with_mode(mode)? {
        Some(token) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let age_seconds = now.saturating_sub(token.obtained_at);

            Ok(AuthStatus::Authenticated {
                label: token.label,
                age_seconds,
                expires_in: token.expires_at.map(|e| e.saturating_sub(now)),
            })
        }
        None => Ok(AuthStatus::NotAuthenticated),
    }
}

pub fn get_auth_status() -> Result<AuthStatus> {
    match load_oauth_token()? {
        Some(token) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let age_seconds = now.saturating_sub(token.obtained_at);

            Ok(AuthStatus::Authenticated {
                label: token.label,
                age_seconds,
                expires_in: token.expires_at.map(|e| e.saturating_sub(now)),
            })
        }
        None => Ok(AuthStatus::NotAuthenticated),
    }
}

/// OAuth authentication status.
#[derive(Debug, Clone)]
pub enum AuthStatus {
    /// User is authenticated with OAuth
    Authenticated {
        /// Optional label for the token
        label: Option<String>,
        /// How long ago the token was obtained (seconds)
        age_seconds: u64,
        /// Time until expiry (seconds), if known
        expires_in: Option<u64>,
    },
    /// User is not authenticated via OAuth
    NotAuthenticated,
}

impl AuthStatus {
    /// Check if the user is authenticated.
    pub fn is_authenticated(&self) -> bool {
        matches!(self, AuthStatus::Authenticated { .. })
    }

    /// Get a human-readable status string.
    fn display_string(&self) -> String {
        match self {
            AuthStatus::Authenticated { label, age_seconds, expires_in } => {
                let label_str = label.as_ref().map(|l| format!(" ({l})")).unwrap_or_default();
                let age_str = format!("{} ago", vtcode_commons::formatting::humanize_duration_compact(*age_seconds));
                let expiry_str = expires_in
                    .map(|e| format!(", expires in {}", vtcode_commons::formatting::humanize_duration_compact(e)))
                    .unwrap_or_default();
                format!("Authenticated{label_str}, obtained {age_str}{expiry_str}")
            }
            AuthStatus::NotAuthenticated => "Not authenticated".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use serial_test::serial;
    use std::fs;
    use std::path::PathBuf;

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
    fn test_auth_url_generation() {
        let challenge = PkceChallenge {
            code_verifier: "test_verifier".to_string(),
            code_challenge: "test_challenge".to_string(),
            code_challenge_method: "S256".to_string(),
        };

        let url = get_auth_url(&challenge, 8484);

        assert!(url.starts_with("https://openrouter.ai/auth"));
        assert!(url.contains("callback_url="));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn debug_impl_redacts_api_key() {
        let token = OpenRouterToken {
            api_key: "sk-openrouter-secret".to_string(),
            obtained_at: 123,
            expires_at: Some(456),
            label: Some("test token".to_string()),
        };

        let debug = format!("{token:?}");

        assert!(!debug.contains("sk-openrouter-secret"), "api key leaked: {debug}");
        assert!(debug.contains("<redacted>"), "api key should be redacted: {debug}");
        assert!(debug.contains("test token"), "non-secret metadata should remain visible: {debug}");
    }

    #[test]
    fn token_exchange_errors_do_not_include_response_bodies() {
        let error = token_exchange_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        let message = error.to_string();

        assert_eq!(message, "Token exchange failed (HTTP 500 Internal Server Error)");
        assert!(!message.contains("sk-openrouter-secret"));
    }

    #[test]
    fn test_token_expiry_check() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Non-expired token
        let token = OpenRouterToken {
            api_key: "test".to_string(),
            obtained_at: now,
            expires_at: Some(now + 3600),
            label: None,
        };
        assert!(!token.is_expired());

        // Expired token
        let expired_token = OpenRouterToken {
            api_key: "test".to_string(),
            obtained_at: now - 7200,
            expires_at: Some(now - 3600),
            label: None,
        };
        assert!(expired_token.is_expired());

        // No expiry
        let no_expiry_token = OpenRouterToken {
            api_key: "test".to_string(),
            obtained_at: now,
            expires_at: None,
            label: None,
        };
        assert!(!no_expiry_token.is_expired());
    }

    #[test]
    fn test_encryption_roundtrip() {
        let token = OpenRouterToken {
            api_key: "sk-test-key-12345".to_string(),
            obtained_at: 1234567890,
            expires_at: Some(1234567890 + 86400),
            label: Some("Test Token".to_string()),
        };

        let encrypted = encrypt_token(&token).unwrap();
        let decrypted = decrypt_token(&encrypted).unwrap();

        assert_eq!(decrypted.api_key, token.api_key);
        assert_eq!(decrypted.obtained_at, token.obtained_at);
        assert_eq!(decrypted.expires_at, token.expires_at);
        assert_eq!(decrypted.label, token.label);
    }

    #[test]
    fn test_auth_status_display() {
        let status = AuthStatus::Authenticated {
            label: Some("My App".to_string()),
            age_seconds: 3700,
            expires_in: Some(86000),
        };

        let display = status.display_string();
        assert!(display.contains("Authenticated"));
        assert!(display.contains("My App"));
    }

    #[test]
    #[serial]
    fn file_storage_round_trips_without_plaintext() {
        let _guard = TestAuthDirGuard::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = OpenRouterToken {
            api_key: "sk-test-key-12345".to_string(),
            obtained_at: now,
            expires_at: Some(now + 86400),
            label: Some("Test Token".to_string()),
        };

        save_oauth_token_with_mode(&token, AuthCredentialsStoreMode::File).expect("save token");
        let loaded = load_oauth_token_with_mode(AuthCredentialsStoreMode::File).expect("load token");
        assert_eq!(loaded.as_ref().map(|value| &value.api_key), Some(&token.api_key));

        let stored = fs::read_to_string(OpenRouterTokenStorage::new().current_file_path().expect("token path"))
            .expect("read token file");
        assert!(!stored.contains(&token.api_key));
    }

    #[test]
    #[serial]
    fn default_loader_falls_back_to_shared_file_storage() {
        let _guard = TestAuthDirGuard::new();
        let token = OpenRouterToken {
            api_key: "sk-default-file-token".to_string(),
            obtained_at: 1,
            expires_at: None,
            label: Some("default file fallback".to_string()),
        };

        save_oauth_token_with_mode(&token, AuthCredentialsStoreMode::File).expect("save token");

        let loaded = load_oauth_token()
            .expect("load default token")
            .expect("token should be present");
        assert_eq!(loaded.api_key, token.api_key);
    }

    #[test]
    #[serial]
    fn legacy_file_token_migrates_to_shared_storage() {
        let _guard = TestAuthDirGuard::new();
        let token = OpenRouterToken {
            api_key: "sk-legacy-token".to_string(),
            obtained_at: 1,
            expires_at: None,
            label: Some("legacy".to_string()),
        };
        let encrypted = encrypt_token(&token).expect("encrypt legacy token");
        let legacy_path = get_token_path().expect("legacy token path");
        fs::write(&legacy_path, serde_json::to_vec(&encrypted).expect("serialize legacy token"))
            .expect("write legacy token");

        let loaded = load_oauth_token_with_mode(AuthCredentialsStoreMode::File)
            .expect("load migrated token")
            .expect("token should be present");

        assert_eq!(loaded.api_key, token.api_key);
        assert!(legacy_path.exists(), "legacy token should remain as a rollback source after migration");
        assert!(
            OpenRouterTokenStorage::new()
                .current_file_path()
                .expect("shared token path")
                .exists()
        );
    }

    #[test]
    #[serial]
    #[cfg(unix)]
    fn file_storage_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = TestAuthDirGuard::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let token = OpenRouterToken {
            api_key: "sk-test-key-12345".to_string(),
            obtained_at: now,
            expires_at: Some(now + 86400),
            label: Some("Test Token".to_string()),
        };

        save_oauth_token_with_mode(&token, AuthCredentialsStoreMode::File).expect("save token");

        let metadata = fs::metadata(OpenRouterTokenStorage::new().current_file_path().expect("token path"))
            .expect("read token metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
}
