//! OpenRouter OAuth PKCE authentication flow.
//!
//! This module implements the OAuth PKCE flow for OpenRouter, allowing users
//! to authenticate with their OpenRouter account securely.
//!
//! ## Security Model
//!
//! Tokens are encrypted at rest using AES-256-GCM with a machine-derived key.
//! The key is derived from:
//! - Machine hostname
//! - User ID (where available)
//! - A static salt
//!
//! This provides reasonable protection against casual access while remaining
//! portable across the same user's sessions on the same machine.

use anyhow::{Context, Result, anyhow};
use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use super::pkce::PkceChallenge;

/// OpenRouter API endpoints
const OPENROUTER_AUTH_URL: &str = "https://openrouter.ai/auth";
const OPENROUTER_KEYS_URL: &str = "https://openrouter.ai/api/v1/auth/keys";

/// Default callback port for localhost OAuth server
pub const DEFAULT_CALLBACK_PORT: u16 = 8484;

/// Configuration for OpenRouter OAuth authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenRouterOAuthConfig {
    /// Whether to use OAuth instead of API key
    pub use_oauth: bool,
    /// Port for the local callback server
    pub callback_port: u16,
    /// Whether to automatically refresh tokens
    pub auto_refresh: bool,
}

impl Default for OpenRouterOAuthConfig {
    fn default() -> Self {
        Self {
            use_oauth: false,
            callback_port: DEFAULT_CALLBACK_PORT,
            auto_refresh: true,
        }
    }
}

/// Stored OAuth token with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl OpenRouterToken {
    /// Check if the token has expired.
    pub fn is_expired(&self) -> bool {
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

/// Encrypted token wrapper for storage.
#[derive(Debug, Serialize, Deserialize)]
struct EncryptedToken {
    /// Base64-encoded nonce
    nonce: String,
    /// Base64-encoded ciphertext (includes auth tag)
    ciphertext: String,
    /// Version for future format changes
    version: u8,
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
    let callback_url = format!("http://localhost:{}/callback", callback_port);
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
    let body = response
        .text()
        .await
        .context("Failed to read response body")?;

    if !status.is_success() {
        // Parse error response for better messages
        if status.as_u16() == 400 {
            return Err(anyhow!(
                "Invalid code_challenge_method. Ensure you're using the same method (S256) in both steps."
            ));
        } else if status.as_u16() == 403 {
            return Err(anyhow!(
                "Invalid code or code_verifier. The authorization code may have expired."
            ));
        } else if status.as_u16() == 405 {
            return Err(anyhow!(
                "Method not allowed. Ensure you're using POST over HTTPS."
            ));
        }
        return Err(anyhow!("Token exchange failed (HTTP {}): {}", status, body));
    }

    // Parse the response to extract the key
    let response_json: serde_json::Value =
        serde_json::from_str(&body).context("Failed to parse token response")?;

    let api_key = response_json
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Response missing 'key' field"))?
        .to_string();

    Ok(api_key)
}

/// Get the path to the token storage file.
fn get_token_path() -> Result<PathBuf> {
    let vtcode_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join(".vtcode")
        .join("auth");

    fs::create_dir_all(&vtcode_dir).context("Failed to create auth directory")?;

    Ok(vtcode_dir.join("openrouter.json"))
}

/// Derive encryption key from machine-specific data.
fn derive_encryption_key() -> Result<LessSafeKey> {
    use ring::digest::{SHA256, digest};

    // Collect machine-specific entropy
    let mut key_material = Vec::new();

    // Hostname
    if let Ok(hostname) = hostname::get() {
        key_material.extend_from_slice(hostname.as_encoded_bytes());
    }

    // User ID (Unix) or username (cross-platform fallback)
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

    // Static salt (not secret, just ensures consistent key derivation)
    key_material.extend_from_slice(b"vtcode-openrouter-oauth-v1");

    // Hash to get 32-byte key
    let hash = digest(&SHA256, &key_material);
    let key_bytes: &[u8; 32] = hash.as_ref()[..32].try_into().context("Hash too short")?;

    let unbound_key = UnboundKey::new(&aead::AES_256_GCM, key_bytes)
        .map_err(|_| anyhow!("Invalid key length"))?;

    Ok(LessSafeKey::new(unbound_key))
}

/// Encrypt token data for storage.
fn encrypt_token(token: &OpenRouterToken) -> Result<EncryptedToken> {
    let key = derive_encryption_key()?;
    let rng = SystemRandom::new();

    // Generate random nonce
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("Failed to generate nonce"))?;

    // Serialize token to JSON
    let plaintext = serde_json::to_vec(token).context("Failed to serialize token")?;

    // Encrypt (includes authentication tag)
    let mut ciphertext = plaintext;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("Encryption failed"))?;

    use base64::{Engine, engine::general_purpose::STANDARD};

    Ok(EncryptedToken {
        nonce: STANDARD.encode(nonce_bytes),
        ciphertext: STANDARD.encode(&ciphertext),
        version: 1,
    })
}

/// Decrypt stored token data.
fn decrypt_token(encrypted: &EncryptedToken) -> Result<OpenRouterToken> {
    if encrypted.version != 1 {
        return Err(anyhow!(
            "Unsupported token format version: {}",
            encrypted.version
        ));
    }

    use base64::{Engine, engine::general_purpose::STANDARD};

    let key = derive_encryption_key()?;

    let nonce_bytes: [u8; NONCE_LEN] = STANDARD
        .decode(&encrypted.nonce)
        .context("Invalid nonce encoding")?
        .try_into()
        .map_err(|_| anyhow!("Invalid nonce length"))?;

    let mut ciphertext = STANDARD
        .decode(&encrypted.ciphertext)
        .context("Invalid ciphertext encoding")?;

    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let plaintext = key
        .open_in_place(nonce, Aad::empty(), &mut ciphertext)
        .map_err(|_| {
            anyhow!("Decryption failed - token may be corrupted or from different machine")
        })?;

    serde_json::from_slice(plaintext).context("Failed to deserialize token")
}

/// Save an OAuth token to encrypted storage.
pub fn save_oauth_token(token: &OpenRouterToken) -> Result<()> {
    let path = get_token_path()?;
    let encrypted = encrypt_token(token)?;
    let json =
        serde_json::to_string_pretty(&encrypted).context("Failed to serialize encrypted token")?;

    fs::write(&path, json).context("Failed to write token file")?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, perms).context("Failed to set token file permissions")?;
    }

    tracing::info!("OAuth token saved to {}", path.display());
    Ok(())
}

/// Load an OAuth token from encrypted storage.
///
/// Returns `None` if no token exists or the token has expired.
pub fn load_oauth_token() -> Result<Option<OpenRouterToken>> {
    let path = get_token_path()?;

    if !path.exists() {
        return Ok(None);
    }

    let json = fs::read_to_string(&path).context("Failed to read token file")?;
    let encrypted: EncryptedToken =
        serde_json::from_str(&json).context("Failed to parse token file")?;

    let token = decrypt_token(&encrypted)?;

    // Check expiry
    if token.is_expired() {
        tracing::warn!("OAuth token has expired, removing...");
        clear_oauth_token()?;
        return Ok(None);
    }

    Ok(Some(token))
}

/// Clear the stored OAuth token.
pub fn clear_oauth_token() -> Result<()> {
    let path = get_token_path()?;

    if path.exists() {
        fs::remove_file(&path).context("Failed to remove token file")?;
        tracing::info!("OAuth token cleared");
    }

    Ok(())
}

/// Get the current OAuth authentication status.
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
    pub fn display_string(&self) -> String {
        match self {
            AuthStatus::Authenticated {
                label,
                age_seconds,
                expires_in,
            } => {
                let label_str = label
                    .as_ref()
                    .map(|l| format!(" ({})", l))
                    .unwrap_or_default();
                let age_str = humanize_duration(*age_seconds);
                let expiry_str = expires_in
                    .map(|e| format!(", expires in {}", humanize_duration(e)))
                    .unwrap_or_default();
                format!(
                    "Authenticated{}, obtained {}{}",
                    label_str, age_str, expiry_str
                )
            }
            AuthStatus::NotAuthenticated => "Not authenticated".to_string(),
        }
    }
}

/// Convert seconds to human-readable duration.
fn humanize_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s ago", seconds)
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
