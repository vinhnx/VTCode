//! AES-256-GCM credential encryption and key derivation.
//!
//! Pure functions — no side effects, no IO. Takes bytes, returns bytes.

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

const ENCRYPTED_CREDENTIAL_VERSION: u8 = 1;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct EncryptedCredential {
    pub(crate) nonce: String,
    pub(crate) ciphertext: String,
    pub(crate) version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) salt: Option<String>,
}

/// Encrypt a plaintext credential string into a portable encrypted payload.
pub(crate) fn encrypt(value: &str) -> Result<EncryptedCredential> {
    let rng = SystemRandom::new();
    let mut salt_bytes = [0_u8; 16];
    rng.fill(&mut salt_bytes)
        .map_err(|_| anyhow!("failed to generate credential salt"))?;
    let salt = STANDARD.encode(salt_bytes);

    let key = derive_key(Some(&salt))?;
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("failed to generate credential nonce"))?;

    let mut ciphertext = value.as_bytes().to_vec();
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce_bytes), Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("failed to encrypt credential"))?;

    Ok(EncryptedCredential {
        nonce: STANDARD.encode(nonce_bytes),
        ciphertext: STANDARD.encode(ciphertext),
        version: ENCRYPTED_CREDENTIAL_VERSION,
        salt: Some(salt),
    })
}

/// Decrypt an [`EncryptedCredential`] back into the plaintext string.
pub(crate) fn decrypt(encrypted: &EncryptedCredential) -> Result<String> {
    if encrypted.version != ENCRYPTED_CREDENTIAL_VERSION {
        return Err(anyhow!("unsupported encrypted credential format"));
    }

    let nonce_bytes = STANDARD.decode(&encrypted.nonce).context("failed to decode credential nonce")?;
    let nonce_array: [u8; NONCE_LEN] =
        nonce_bytes.try_into().map_err(|_| anyhow!("invalid credential nonce length"))?;
    let mut ciphertext = STANDARD
        .decode(&encrypted.ciphertext)
        .context("failed to decode credential ciphertext")?;

    let key = derive_key(encrypted.salt.as_deref())?;
    let plaintext = key
        .open_in_place(Nonce::assume_unique_for_key(nonce_array), Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("failed to decrypt credential"))?;

    String::from_utf8(plaintext.to_vec()).context("failed to parse decrypted credential")
}

/// Derive an AES-256-GCM key from machine+user identity and an optional
/// per-file salt.
fn derive_key(salt: Option<&str>) -> Result<LessSafeKey> {
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

    if let Some(salt) = salt {
        key_material.extend_from_slice(salt.as_bytes());
    }

    let hash = digest(&SHA256, &key_material);
    let key_bytes: &[u8; 32] = hash.as_ref()[..32]
        .try_into()
        .context("credential encryption key was too short")?;
    let unbound =
        UnboundKey::new(&aead::AES_256_GCM, key_bytes).map_err(|_| anyhow!("invalid credential encryption key"))?;
    Ok(LessSafeKey::new(unbound))
}
