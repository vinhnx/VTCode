//! Typed storage boundary for OpenAI ChatGPT sessions.
//!
//! New sessions use the shared [`CredentialStorage`] backends. The legacy
//! OpenAI file format remains readable so existing installations migrate on
//! their next successful load.

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use std::fs;
use std::path::PathBuf;

use crate::credentials::{AuthCredentialsStoreMode, CredentialStorage, ResolvedStoreMode};
use crate::storage_paths::{legacy_auth_file_paths, read_legacy_compatible_file};

use super::openai_chatgpt_oauth::OpenAIChatGptSession;

const STORAGE_SERVICE: &str = "vtcode";
const STORAGE_USER: &str = "openai_chatgpt_session";
const LEGACY_SESSION_FILE: &str = "openai_chatgpt.json";

/// Typed persistence boundary for an OpenAI ChatGPT session.
pub(crate) struct OpenAiSessionStorage {
    backend: CredentialStorage,
}

impl OpenAiSessionStorage {
    pub(crate) fn new() -> Self {
        Self {
            backend: CredentialStorage::new(STORAGE_SERVICE, STORAGE_USER),
        }
    }

    pub(crate) fn save(&self, session: &OpenAIChatGptSession, mode: AuthCredentialsStoreMode) -> Result<()> {
        self.backend
            .store_json(session, mode)
            .context("failed to persist openai session")
    }

    pub(crate) fn load(&self, mode: AuthCredentialsStoreMode) -> Result<Option<OpenAIChatGptSession>> {
        let effective_mode = mode.effective_mode();
        let session = match effective_mode {
            ResolvedStoreMode::Keyring => self
                .backend
                .load_json_exact_with_mode(ResolvedStoreMode::Keyring)?
                .or(self.backend.load_json_exact_with_mode(ResolvedStoreMode::File)?),
            ResolvedStoreMode::File => self
                .backend
                .load_json_exact_with_mode(ResolvedStoreMode::File)?
                .or(self.backend.load_json_exact_with_mode(ResolvedStoreMode::Keyring)?),
        };

        if let Some(session) = session {
            return Ok(Some(session));
        }

        let Some(session) = load_legacy_session()? else {
            return Ok(None);
        };

        self.save(&session, mode).context("failed to migrate legacy openai session")?;
        Ok(Some(session))
    }

    pub(crate) fn clear(&self, mode: AuthCredentialsStoreMode) -> Result<()> {
        let mut errors = Vec::new();
        let effective_mode = mode.effective_mode();
        if let Err(err) = self.backend.clear_exact_with_mode(effective_mode) {
            errors.push(err.to_string());
        }
        if effective_mode == ResolvedStoreMode::File {
            if let Err(err) = clear_legacy_session_file() {
                errors.push(err.to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("failed to clear openai session: {}", errors.join("; ")))
        }
    }

    /// Clear both shared backends and the legacy file regardless of the
    /// configured default mode.
    pub(crate) fn clear_all(&self) -> Result<()> {
        let mut errors = Vec::new();
        for mode in [ResolvedStoreMode::Keyring, ResolvedStoreMode::File] {
            if let Err(err) = self.backend.clear_exact_with_mode(mode) {
                errors.push(err.to_string());
            }
        }
        if let Err(err) = clear_legacy_session_file() {
            errors.push(err.to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("failed to clear openai session: {}", errors.join("; ")))
        }
    }

    #[cfg(test)]
    pub(crate) fn current_file_path(&self) -> Result<PathBuf> {
        self.backend.file_path_for_tests()
    }
}

fn load_legacy_session() -> Result<Option<OpenAIChatGptSession>> {
    for path in legacy_auth_file_paths(LEGACY_SESSION_FILE)? {
        let Some(data) = read_legacy_compatible_file(&path)? else {
            continue;
        };

        let encrypted: LegacyEncryptedSession = serde_json::from_slice(&data)
            .with_context(|| format!("failed to decode legacy openai session file {}", path.display()))?;
        return decrypt_legacy_session(&encrypted).map(Some);
    }
    Ok(None)
}

fn clear_legacy_session_file() -> Result<()> {
    for path in legacy_auth_file_paths(LEGACY_SESSION_FILE)? {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(anyhow!("failed to delete legacy openai session file {}: {err}", path.display())),
        }
    }
    Ok(())
}

pub(crate) fn legacy_session_path() -> Result<PathBuf> {
    legacy_auth_file_paths(LEGACY_SESSION_FILE)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no legacy openai session path available"))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct LegacyEncryptedSession {
    nonce: String,
    ciphertext: String,
    version: u8,
}

pub(crate) fn encrypt_legacy_session(session: &OpenAIChatGptSession) -> Result<LegacyEncryptedSession> {
    let key = derive_legacy_encryption_key()?;
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes).map_err(|_| anyhow!("failed to generate nonce"))?;

    let mut ciphertext = serde_json::to_vec(session).context("failed to serialize openai session for encryption")?;
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce_bytes), Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("failed to encrypt openai session"))?;

    Ok(LegacyEncryptedSession {
        nonce: STANDARD.encode(nonce_bytes),
        ciphertext: STANDARD.encode(ciphertext),
        version: 1,
    })
}

pub(crate) fn decrypt_legacy_session(encrypted: &LegacyEncryptedSession) -> Result<OpenAIChatGptSession> {
    if encrypted.version != 1 {
        bail!("unsupported openai session encryption format");
    }

    let nonce_bytes = STANDARD
        .decode(&encrypted.nonce)
        .context("failed to decode openai session nonce")?;
    let nonce_array: [u8; NONCE_LEN] = nonce_bytes
        .try_into()
        .map_err(|_| anyhow!("invalid openai session nonce length"))?;
    let mut ciphertext = STANDARD
        .decode(&encrypted.ciphertext)
        .context("failed to decode openai session ciphertext")?;

    let key = derive_legacy_encryption_key()?;
    let plaintext = key
        .open_in_place(Nonce::assume_unique_for_key(nonce_array), Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("failed to decrypt openai session"))?;
    serde_json::from_slice(plaintext).context("failed to parse decrypted openai session")
}

fn derive_legacy_encryption_key() -> Result<LessSafeKey> {
    use ring::digest::{SHA256, digest};

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

    key_material.extend_from_slice(b"vtcode-openai-chatgpt-oauth-v1");
    let hash = digest(&SHA256, &key_material);
    let key_bytes: &[u8; 32] = hash
        .as_ref()
        .get(..32)
        .context("openai session encryption key was too short")?
        .try_into()
        .context("openai session encryption key had an invalid length")?;
    let unbound =
        UnboundKey::new(&aead::AES_256_GCM, key_bytes).map_err(|_| anyhow!("invalid openai session encryption key"))?;
    Ok(LessSafeKey::new(unbound))
}
