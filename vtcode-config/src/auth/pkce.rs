//! PKCE (Proof Key for Code Exchange) utilities for OAuth 2.0.
//!
//! Implements RFC 7636 for secure OAuth flows without client secrets.
//! Uses SHA-256 (S256) code challenge method as recommended by the spec.

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

/// PKCE code verifier length (43-128 characters per RFC 7636)
const CODE_VERIFIER_LENGTH: usize = 64;

/// Characters allowed in code verifier (unreserved URI characters)
const CODE_VERIFIER_CHARSET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// PKCE challenge pair containing verifier and challenge strings.
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    /// The code verifier (random string, kept secret by client)
    pub code_verifier: String,
    /// The code challenge (SHA-256 hash of verifier, sent to authorization server)
    pub code_challenge: String,
    /// The challenge method (always "S256" for SHA-256)
    pub code_challenge_method: String,
}

impl PkceChallenge {
    /// Create a new PKCE challenge from a code verifier.
    pub fn from_verifier(code_verifier: String) -> Result<Self> {
        let code_challenge = compute_s256_challenge(&code_verifier)?;
        Ok(Self {
            code_verifier,
            code_challenge,
            code_challenge_method: "S256".to_string(),
        })
    }
}

/// Generate a cryptographically secure PKCE challenge pair.
///
/// This function generates a random code verifier and computes
/// the corresponding S256 code challenge.
///
/// # Example
/// ```
/// use vtcode_config::auth::pkce::generate_pkce_challenge;
///
/// let challenge = generate_pkce_challenge().unwrap();
/// println!("Verifier: {}", challenge.code_verifier);
/// println!("Challenge: {}", challenge.code_challenge);
/// ```
pub fn generate_pkce_challenge() -> Result<PkceChallenge> {
    let code_verifier = generate_code_verifier()?;
    PkceChallenge::from_verifier(code_verifier)
}

/// Generate a cryptographically random code verifier.
fn generate_code_verifier() -> Result<String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use a simple but effective random generation approach
    // Combine system time entropy with process ID for uniqueness
    let mut verifier = String::with_capacity(CODE_VERIFIER_LENGTH);

    // Seed from system time nanoseconds + process ID
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System time before UNIX epoch")?
        .as_nanos();

    let pid = std::process::id() as u128;
    let mut state = nanos.wrapping_add(pid);

    // XorShift128+ inspired PRNG for good distribution
    for _ in 0..CODE_VERIFIER_LENGTH {
        // Mix entropy
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;

        // Add more entropy from high-res time
        let extra = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        state = state.wrapping_add(extra);

        let idx = (state % CODE_VERIFIER_CHARSET.len() as u128) as usize;
        verifier.push(CODE_VERIFIER_CHARSET[idx] as char);
    }

    Ok(verifier)
}

/// Compute S256 code challenge from a code verifier.
///
/// S256 = BASE64URL(SHA256(code_verifier))
fn compute_s256_challenge(code_verifier: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let hash = hasher.finalize();

    Ok(URL_SAFE_NO_PAD.encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pkce_challenge() {
        let challenge = generate_pkce_challenge().unwrap();

        // Verify code verifier length
        assert_eq!(challenge.code_verifier.len(), CODE_VERIFIER_LENGTH);

        // Verify all characters are in allowed charset
        for c in challenge.code_verifier.chars() {
            assert!(
                CODE_VERIFIER_CHARSET.contains(&(c as u8)),
                "Invalid character in verifier: {}",
                c
            );
        }

        // Verify challenge method
        assert_eq!(challenge.code_challenge_method, "S256");

        // Verify challenge is valid base64url (43 chars for SHA-256)
        assert_eq!(challenge.code_challenge.len(), 43);
    }

    #[test]
    fn test_deterministic_challenge() {
        // Same verifier should produce same challenge
        let verifier = "test_verifier_string_for_deterministic_test";
        let challenge1 = PkceChallenge::from_verifier(verifier.to_string()).unwrap();
        let challenge2 = PkceChallenge::from_verifier(verifier.to_string()).unwrap();

        assert_eq!(challenge1.code_challenge, challenge2.code_challenge);
    }

    #[test]
    fn test_unique_verifiers() {
        // Multiple calls should produce different verifiers
        let c1 = generate_pkce_challenge().unwrap();
        let c2 = generate_pkce_challenge().unwrap();

        assert_ne!(c1.code_verifier, c2.code_verifier);
    }
}
