//! OpenAI ChatGPT subscription OAuth flow and secure session storage.
//!
//! This module implements an OAuth 2.0 PKCE authorization-code flow for ChatGPT
//! subscription auth, mirroring the flow used by [openai/codex]. By default VT
//! Code reuses the Codex CLI's **public PKCE OAuth client identity** (no client
//! secret — the ID is not a secret by OAuth 2.1 design). This is an **unofficial
//! compatibility mechanism**: OpenAI has not documented or guaranteed third-party
//! reuse of this client identity, and a public client ID is not authorization
//! to reuse another tool's OAuth registration. This allows ChatGPT subscription
//! login to work without the Codex CLI installed.
//! Organizations with their own OpenAI-issued client can override via
//! `VTCODE_OPENAI_OAUTH_CLIENT_ID` / `VTCODE_OPENAI_OAUTH_ORIGINATOR`.
//!
//! - OAuth authorization-code flow with PKCE
//! - refresh-token exchange
//! - token exchange for an OpenAI API-key-style bearer token
//! - secure storage in keyring or encrypted file storage
//!
//! Based on patterns from [openai/codex] (Apache-2.0). Copyright 2025 OpenAI.
//! See the repository `THIRD-PARTY-NOTICES` file for full attribution.
//!
//! [openai/codex]: https://github.com/openai/codex

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fs2::FileExt;
use reqwest::Client;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::fs::OpenOptions;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

use crate::storage_paths::auth_storage_dir;
use crate::{OpenAIAuthConfig, OpenAIPreferredMethod};

pub use super::credentials::AuthCredentialsStoreMode;
use super::pkce::PkceChallenge;
#[cfg(test)]
use crate::openai_refresh_policy::extract_error_code;
use crate::openai_refresh_policy::{RefreshFailureAction, classify_refresh_failure};
use crate::openai_session_storage::OpenAiSessionStorage;
#[cfg(test)]
use crate::openai_session_storage::{
    decrypt_legacy_session as decrypt_session, encrypt_legacy_session as encrypt_session,
    legacy_session_path as get_session_path,
};

const OPENAI_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Default OAuth client identity.
///
/// This is the **Codex CLI's public PKCE OAuth client ID**. VT Code reuses
/// Codex's public client identity (a PKCE public client with no client secret
/// — the ID is not a secret by OAuth 2.1 design) as an **unofficial
/// compatibility mechanism**. OpenAI has not documented or guaranteed
/// third-party reuse of this identity, and a public client ID is not
/// authorization to reuse another tool's OAuth registration. This lets VT
/// Code perform ChatGPT subscription login without requiring the Codex CLI
/// to be installed.
///
/// Organizations with their own OpenAI-issued OAuth client can override this
/// via the `VTCODE_OPENAI_OAUTH_CLIENT_ID` environment variable.
///
/// See `docs/guides/oauth-authentication.md` for the full explanation.
const DEFAULT_OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// Default originator sent to OpenAI's authorization endpoint.
///
/// This matches the Codex CLI's originator because the default client ID is
/// Codex's. Override with `VTCODE_OPENAI_OAUTH_ORIGINATOR` when using a custom
/// client ID.
const DEFAULT_OPENAI_ORIGINATOR: &str = "codex_cli_rs";
/// Maximum bytes read from a token-endpoint error response body for
/// classification. Prevents unbounded reads from a misbehaving or hostile
/// endpoint while still capturing standard OAuth 2.0 error JSON.
const MAX_ERROR_BODY_BYTES: usize = 8 * 1024;
const OPENAI_CALLBACK_PATH: &str = "/auth/callback";
const OPENAI_REFRESH_LOCK_FILE: &str = "openai_chatgpt.refresh.lock";
const REFRESH_INTERVAL_SECS: u64 = 8 * 60;
const REFRESH_SKEW_SECS: u64 = 60;

/// Resolved OAuth client identity (client ID + originator).
///
/// Both fields must be consistent: when a custom client ID is provided via
/// `VTCODE_OPENAI_OAUTH_CLIENT_ID`, the originator must also be overridden
/// via `VTCODE_OPENAI_OAUTH_ORIGINATOR`. Sending a custom client ID with
/// Codex's `codex_cli_rs` originator (or vice versa) would be inconsistent
/// and is rejected.
///
/// `Debug` is safe to derive: the client ID is a public PKCE client
/// identifier (not a secret by OAuth 2.1 design), and the originator is
/// a public identifier string.
#[derive(Debug)]
struct OAuthClientIdentity {
    client_id: String,
    originator: String,
}

/// Resolve the OAuth client identity from environment variables.
///
/// ## Invariant
///
/// The client ID and originator form a **coherent pair**. One-sided overrides
/// are rejected to prevent mixed identities (e.g. a custom client ID paired
/// with Codex's `codex_cli_rs` originator).
///
/// - Both `VTCODE_OPENAI_OAUTH_CLIENT_ID` and `VTCODE_OPENAI_OAUTH_ORIGINATOR`
///   set and non-blank → use the custom pair.
/// - Neither set → use the complete Codex default pair.
/// - Only one set → return a configuration error with an actionable message.
///   The caller must surface this so the user can fix the environment before
///   any OAuth request is sent.
///
/// All four flow stages (authorization URL, code exchange, refresh, token
/// exchange) call this resolver, so the same coherent pair is used throughout.
fn resolve_oauth_client_identity() -> Result<OAuthClientIdentity> {
    let custom_client_id = std::env::var("VTCODE_OPENAI_OAUTH_CLIENT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let custom_originator = std::env::var("VTCODE_OPENAI_OAUTH_ORIGINATOR")
        .ok()
        .filter(|v| !v.trim().is_empty());

    match (custom_client_id, custom_originator) {
        (Some(id), Some(originator)) => Ok(OAuthClientIdentity { client_id: id, originator }),
        (Some(_), None) => bail!(
            "VTCODE_OPENAI_OAUTH_CLIENT_ID is set but VTCODE_OPENAI_OAUTH_ORIGINATOR is not. \
             The client ID and originator must be overridden together to form a coherent OAuth \
             identity. Set VTCODE_OPENAI_OAUTH_ORIGINATOR to match your custom client ID, \
             or unset VTCODE_OPENAI_OAUTH_CLIENT_ID to use the default Codex identity."
        ),
        (None, Some(_)) => bail!(
            "VTCODE_OPENAI_OAUTH_ORIGINATOR is set but VTCODE_OPENAI_OAUTH_CLIENT_ID is not. \
             The client ID and originator must be overridden together to form a coherent OAuth \
             identity. Set VTCODE_OPENAI_OAUTH_CLIENT_ID to match your custom originator, \
             or unset VTCODE_OPENAI_OAUTH_ORIGINATOR to use the default Codex identity."
        ),
        (None, None) => Ok(OAuthClientIdentity {
            client_id: DEFAULT_OPENAI_CLIENT_ID.to_string(),
            originator: DEFAULT_OPENAI_ORIGINATOR.to_string(),
        }),
    }
}

mod jwt;
mod refresh;
mod session;

pub(crate) use jwt::{parse_jwt_claims, parse_jwt_exp};
pub use refresh::{
    clear_openai_chatgpt_session, clear_openai_chatgpt_session_with_mode, exchange_openai_chatgpt_code_for_tokens,
    get_openai_chatgpt_auth_status, get_openai_chatgpt_auth_status_with_mode, load_openai_chatgpt_session,
    load_openai_chatgpt_session_with_mode, refresh_openai_chatgpt_session_with_mode, save_openai_chatgpt_session,
    save_openai_chatgpt_session_with_mode,
};
pub use session::{
    OpenAIChatGptAuthHandle, OpenAIChatGptAuthStatus, OpenAIChatGptSession, OpenAIChatGptSessionProvenance,
    OpenAIChatGptSessionRefresher, OpenAICredentialOverview, OpenAIResolvedAuth, OpenAIResolvedAuthSource,
    generate_openai_oauth_state, get_openai_chatgpt_auth_url, parse_openai_chatgpt_manual_callback_input,
    resolve_openai_auth, summarize_openai_credentials,
};

#[cfg(test)]
pub(crate) use refresh::{
    OpenAIRefreshResponse, acquire_refresh_lock, classify_refresh_status_error, merge_refresh_response,
};
#[cfg(test)]
pub(crate) use session::active_api_bearer_token;

/// Current Unix time in seconds, saturating to `0` before the epoch.
pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
