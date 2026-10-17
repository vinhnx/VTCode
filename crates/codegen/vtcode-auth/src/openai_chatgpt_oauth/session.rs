//! ChatGPT session model, auth handle, and OAuth flow helpers.

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

use crate::{OpenAIAuthConfig, OpenAIPreferredMethod};

pub use super::super::credentials::AuthCredentialsStoreMode;
use super::super::pkce::PkceChallenge;
use super::refresh::refresh_openai_chatgpt_session_from_snapshot;
use super::{
    OPENAI_AUTH_URL, OPENAI_CALLBACK_PATH, OPENAI_TOKEN_URL, REFRESH_INTERVAL_SECS, REFRESH_SKEW_SECS, now_secs,
    resolve_oauth_client_identity,
};

/// Stored OpenAI ChatGPT subscription session.
///
/// Custom `Debug` redacts all token fields to prevent credential leakage
/// through `tracing::debug!(?session)` or error wrappers.
#[derive(Clone, Serialize, Deserialize)]
pub struct OpenAIChatGptSession {
    /// Exchanged OpenAI bearer token used for normal API calls when available.
    /// If unavailable, VT Code falls back to the OAuth access token.
    pub openai_api_key: String,
    /// OAuth ID token from the sign-in flow.
    pub id_token: String,
    /// OAuth access token from the sign-in flow.
    pub access_token: String,
    /// Refresh token used to renew the session.
    pub refresh_token: String,
    /// ChatGPT workspace/account identifier, if present.
    pub account_id: Option<String>,
    /// Account email, if present.
    pub email: Option<String>,
    /// ChatGPT plan type, if present.
    pub plan: Option<String>,
    /// When the session was originally created.
    pub obtained_at: u64,
    /// When the OAuth/API-key exchange was last refreshed.
    pub refreshed_at: u64,
    /// Access-token expiry, if supplied by the authority.
    pub expires_at: Option<u64>,
}

impl fmt::Debug for OpenAIChatGptSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAIChatGptSession")
            .field("openai_api_key", &"<redacted>")
            .field("id_token", &"<redacted>")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("account_id", &self.account_id)
            .field("email", &self.email)
            .field("plan", &self.plan)
            .field("obtained_at", &self.obtained_at)
            .field("refreshed_at", &self.refreshed_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl OpenAIChatGptSession {
    pub(super) fn is_refresh_due(&self) -> bool {
        let now = now_secs();
        if let Some(expires_at) = self.expires_at
            && now.saturating_add(REFRESH_SKEW_SECS) >= expires_at
        {
            return true;
        }
        now.saturating_sub(self.refreshed_at) >= REFRESH_INTERVAL_SECS
    }
}

/// Host-provided refresher for externally managed ChatGPT auth tokens.
#[async_trait]
pub trait OpenAIChatGptSessionRefresher: Send + Sync {
    async fn refresh_session(&self, current: &OpenAIChatGptSession) -> Result<OpenAIChatGptSession>;
}

#[derive(Clone)]
enum OpenAIChatGptAuthRefreshStrategy {
    Stored {
        storage_mode: AuthCredentialsStoreMode,
    },
    External {
        refresher: Arc<dyn OpenAIChatGptSessionRefresher>,
    },
}

impl fmt::Debug for OpenAIChatGptAuthRefreshStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stored { storage_mode } => f.debug_struct("Stored").field("storage_mode", storage_mode).finish(),
            Self::External { .. } => f.debug_struct("External").finish_non_exhaustive(),
        }
    }
}

/// Runtime auth state shared by OpenAI provider instances.
#[derive(Clone)]
pub struct OpenAIChatGptAuthHandle {
    session: Arc<Mutex<OpenAIChatGptSession>>,
    refresh_gate: Arc<AsyncMutex<()>>,
    auto_refresh: bool,
    refresh_strategy: OpenAIChatGptAuthRefreshStrategy,
}

impl fmt::Debug for OpenAIChatGptAuthHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAIChatGptAuthHandle")
            .field("auto_refresh", &self.auto_refresh)
            .field("refresh_strategy", &self.refresh_strategy)
            .finish()
    }
}

impl OpenAIChatGptAuthHandle {
    pub fn new(
        session: OpenAIChatGptSession,
        auth_config: OpenAIAuthConfig,
        storage_mode: AuthCredentialsStoreMode,
    ) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            refresh_gate: Arc::new(AsyncMutex::new(())),
            auto_refresh: auth_config.auto_refresh,
            refresh_strategy: OpenAIChatGptAuthRefreshStrategy::Stored { storage_mode },
        }
    }

    pub fn new_external(
        session: OpenAIChatGptSession,
        auto_refresh: bool,
        refresher: Arc<dyn OpenAIChatGptSessionRefresher>,
    ) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            refresh_gate: Arc::new(AsyncMutex::new(())),
            auto_refresh,
            refresh_strategy: OpenAIChatGptAuthRefreshStrategy::External { refresher },
        }
    }

    pub fn snapshot(&self) -> Result<OpenAIChatGptSession> {
        self.session
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| anyhow!("openai chatgpt auth mutex poisoned"))
    }

    pub fn current_api_key(&self) -> Result<String> {
        self.snapshot().map(|session| active_api_bearer_token(&session).to_string())
    }

    pub fn provider_label(&self) -> &'static str {
        "OpenAI (ChatGPT)"
    }

    pub async fn refresh_if_needed(&self) -> Result<()> {
        if !self.auto_refresh {
            return Ok(());
        }

        self.refresh_when(|session| session.is_refresh_due()).await
    }

    pub async fn force_refresh(&self) -> Result<()> {
        self.refresh_when(|_| true).await
    }

    async fn refresh_when<P>(&self, should_refresh: P) -> Result<()>
    where
        P: FnOnce(&OpenAIChatGptSession) -> bool,
    {
        let _refresh_guard = self.refresh_gate.lock().await;
        let session = self.snapshot()?;
        if !should_refresh(&session) {
            return Ok(());
        }

        let refreshed = match &self.refresh_strategy {
            OpenAIChatGptAuthRefreshStrategy::Stored { storage_mode } => {
                refresh_openai_chatgpt_session_from_snapshot(&session, *storage_mode).await?
            }
            OpenAIChatGptAuthRefreshStrategy::External { refresher } => refresher.refresh_session(&session).await?,
        };
        self.replace_session(refreshed)
    }

    #[must_use]
    pub(super) fn using_external_tokens(&self) -> bool {
        matches!(self.refresh_strategy, OpenAIChatGptAuthRefreshStrategy::External { .. })
    }

    fn replace_session(&self, session: OpenAIChatGptSession) -> Result<()> {
        let mut guard = self.session.lock().map_err(|_| anyhow!("openai chatgpt auth mutex poisoned"))?;
        *guard = session;
        Ok(())
    }
}

/// OpenAI auth resolution chosen for the current runtime.
///
/// Custom `Debug` redacts the bearer `api_key` to prevent credential leakage.
#[derive(Clone)]
pub enum OpenAIResolvedAuth {
    ApiKey {
        api_key: String,
    },
    ChatGpt {
        api_key: String,
        handle: OpenAIChatGptAuthHandle,
    },
}

impl fmt::Debug for OpenAIResolvedAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey { .. } => f.debug_struct("OpenAIResolvedAuth::ApiKey").finish(),
            Self::ChatGpt { .. } => f.debug_struct("OpenAIResolvedAuth::ChatGpt").finish(),
        }
    }
}

impl OpenAIResolvedAuth {
    pub fn api_key(&self) -> &str {
        match self {
            Self::ApiKey { api_key } => api_key,
            Self::ChatGpt { api_key, .. } => api_key,
        }
    }

    pub fn handle(&self) -> Option<OpenAIChatGptAuthHandle> {
        match self {
            Self::ApiKey { .. } => None,
            Self::ChatGpt { handle, .. } => Some(handle.clone()),
        }
    }

    pub(super) fn using_chatgpt(&self) -> bool {
        matches!(self, Self::ChatGpt { .. })
    }
}

pub(crate) fn active_api_bearer_token(session: &OpenAIChatGptSession) -> &str {
    if session.openai_api_key.trim().is_empty() {
        session.access_token.as_str()
    } else {
        session.openai_api_key.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAIResolvedAuthSource {
    ApiKey,
    ChatGpt,
}

/// Where the ChatGPT session originated — used by CLI/TUI to render accurate
/// status without directly inspecting the filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAIChatGptSessionProvenance {
    /// Session stored in VT Code's own credential storage (full auto-refresh).
    Native,
    /// Session loaded from Codex CLI's `~/.codex/auth.json` (managed by Codex).
    CodexFallback,
}

/// Redacted summary of available OpenAI credentials for CLI/TUI display.
///
/// Does NOT carry token data — only metadata (email, plan, provenance, expiry)
/// so credential values can never leak through `Debug` or logging.
#[derive(Debug, Clone)]
pub struct OpenAICredentialOverview {
    pub api_key_available: bool,
    /// Email from the ChatGPT session's ID token, if available.
    pub chatgpt_email: Option<String>,
    /// Plan type from the ChatGPT session's ID token, if available.
    pub chatgpt_plan: Option<String>,
    /// `true` when a ChatGPT session (native or Codex fallback) is available.
    pub chatgpt_session_present: bool,
    /// Provenance of the ChatGPT session — `None` when no session is available.
    pub chatgpt_session_provenance: Option<OpenAIChatGptSessionProvenance>,
    /// `true` only when Codex's auth.json was **successfully parsed** into a
    /// usable session (not merely that the file exists on disk).
    pub codex_fallback_available: bool,
    pub active_source: Option<OpenAIResolvedAuthSource>,
    pub preferred_method: OpenAIPreferredMethod,
    pub notice: Option<String>,
    pub recommendation: Option<String>,
}

/// Generic auth status reused by slash auth/status output.
#[derive(Debug, Clone)]
pub enum OpenAIChatGptAuthStatus {
    Authenticated {
        label: Option<String>,
        age_seconds: u64,
        expires_in: Option<u64>,
    },
    NotAuthenticated,
}
/// Build the OpenAI ChatGPT OAuth authorization URL.
pub fn get_openai_chatgpt_auth_url(challenge: &PkceChallenge, callback_port: u16, state: &str) -> Result<String> {
    let redirect_uri = format!("http://localhost:{callback_port}{OPENAI_CALLBACK_PATH}");
    let identity = resolve_oauth_client_identity()?;
    let query = [
        ("response_type", "code".to_string()),
        ("client_id", identity.client_id.clone()),
        ("redirect_uri", redirect_uri),
        ("scope", "openid profile email offline_access api.connectors.read api.connectors.invoke".to_string()),
        ("code_challenge", challenge.code_challenge.clone()),
        ("code_challenge_method", challenge.code_challenge_method.clone()),
        ("id_token_add_organizations", "true".to_string()),
        ("codex_cli_simplified_flow", "true".to_string()),
        ("state", state.to_string()),
        ("originator", identity.originator),
    ];

    let encoded = query
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    Ok(format!("{OPENAI_AUTH_URL}?{encoded}"))
}

pub fn generate_openai_oauth_state() -> Result<String> {
    let mut state_bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut state_bytes)
        .map_err(|_| anyhow!("failed to generate openai oauth state"))?;
    Ok(URL_SAFE_NO_PAD.encode(state_bytes))
}

pub fn parse_openai_chatgpt_manual_callback_input(input: &str, expected_state: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("missing authorization callback input");
    }

    let query = if trimmed.contains("://") {
        let url = reqwest::Url::parse(trimmed).context("invalid callback url")?;
        url.query()
            .ok_or_else(|| anyhow!("callback url did not include a query string"))?
            .to_string()
    } else if trimmed.contains('=') {
        trimmed.trim_start_matches('?').to_string()
    } else {
        bail!("paste the full redirect url or query string containing code and state");
    };

    let code = extract_query_value(&query, "code")
        .ok_or_else(|| anyhow!("callback input did not include an authorization code"))?;
    let state = extract_query_value(&query, "state").ok_or_else(|| anyhow!("callback input did not include state"))?;
    if state != expected_state {
        bail!("OAuth error: state mismatch");
    }
    Ok(code)
}
/// Resolve the active OpenAI auth source for the current configuration.
pub fn resolve_openai_auth(
    auth_config: &OpenAIAuthConfig,
    storage_mode: AuthCredentialsStoreMode,
    api_key: Option<String>,
) -> Result<OpenAIResolvedAuth> {
    crate::auth_service::OpenAIAccountAuthService::new(auth_config.clone(), storage_mode).resolve_runtime_auth(api_key)
}

pub fn summarize_openai_credentials(
    auth_config: &OpenAIAuthConfig,
    storage_mode: AuthCredentialsStoreMode,
    api_key: Option<String>,
) -> Result<OpenAICredentialOverview> {
    crate::auth_service::OpenAIAccountAuthService::new(auth_config.clone(), storage_mode).summarize_credentials(api_key)
}
fn extract_query_value(query: &str, key: &str) -> Option<String> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| {
            let (pair_key, pair_value) = pair.split_once('=')?;
            (pair_key == key)
                .then(|| urlencoding::decode(pair_value).ok().map(|value| value.into_owned()))
                .flatten()
        })
        .find(|value| !value.is_empty())
}
