//! PKCE (Proof Key for Code Exchange) utilities for OAuth 2.0.
//!
//! Implements RFC 7636 for secure OAuth flows without client secrets.
//! Uses SHA-256 (S256) code challenge method as recommended by the spec.

use anyhow::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::rand::{SecureRandom, SystemRandom};
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
/// use vtcode_auth::generate_pkce_challenge;
///
/// let challenge = generate_pkce_challenge().unwrap();
/// println!("Verifier: {}", challenge.code_verifier);
/// println!("Challenge: {}", challenge.code_challenge);
/// ```
pub fn generate_pkce_challenge() -> Result<PkceChallenge> {
    let code_verifier = generate_code_verifier()?;
    PkceChallenge::from_verifier(code_verifier)
}

/// Generate a cryptographically random code verifier per RFC 7636 §4.1.
///
/// Uses `ring::rand::SystemRandom` (backed by the OS CSPRNG) instead of a
/// user-space PRNG to ensure ≥128 bits of entropy as required by the spec.
fn generate_code_verifier() -> Result<String> {
    let rng = SystemRandom::new();
    let charset_len = CODE_VERIFIER_CHARSET.len() as u8;
    let max_valid = (256u16 - 256u16 % charset_len as u16) as u8;
    let mut verifier = String::with_capacity(CODE_VERIFIER_LENGTH);
    let mut buf = [0u8; 1];

    while verifier.len() < CODE_VERIFIER_LENGTH {
        rng.fill(&mut buf)
            .map_err(|_| anyhow::anyhow!("failed to read from OS random source"))?;
        // Rejection sampling to avoid modulo bias.
        if buf[0] < max_valid {
            let idx = (buf[0] % charset_len) as usize;
            verifier.push(CODE_VERIFIER_CHARSET[idx] as char);
        }
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
