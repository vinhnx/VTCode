//! OpenAI ChatGPT subscription OAuth flow and secure session storage.
//!
//! This module mirrors the Codex CLI login flow closely enough for VT Code:
//! - OAuth authorization-code flow with PKCE
//! - refresh-token exchange
//! - token exchange for an OpenAI API-key-style bearer token
//! - secure storage in keyring or encrypted file storage

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD, engine::general_purpose::URL_SAFE_NO_PAD};
use fs2::FileExt;
use reqwest::Client;
use ring::aead::{self, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::storage_paths::auth_storage_dir;
use crate::{OpenAIAuthConfig, OpenAIPreferredMethod};

pub use super::credentials::AuthCredentialsStoreMode;
use super::pkce::PkceChallenge;

const OPENAI_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_ORIGINATOR: &str = "codex_cli_rs";
const OPENAI_CALLBACK_PATH: &str = "/auth/callback";
const OPENAI_STORAGE_SERVICE: &str = "vtcode";
const OPENAI_STORAGE_USER: &str = "openai_chatgpt_session";
const OPENAI_SESSION_FILE: &str = "openai_chatgpt.json";
const OPENAI_REFRESH_LOCK_FILE: &str = "openai_chatgpt.refresh.lock";
const REFRESH_INTERVAL_SECS: u64 = 8 * 60;
const REFRESH_SKEW_SECS: u64 = 60;

/// Stored OpenAI ChatGPT subscription session.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl OpenAIChatGptSession {
    pub fn is_refresh_due(&self) -> bool {
        let now = now_secs();
        if let Some(expires_at) = self.expires_at
            && now.saturating_add(REFRESH_SKEW_SECS) >= expires_at
        {
            return true;
        }
        now.saturating_sub(self.refreshed_at) >= REFRESH_INTERVAL_SECS
    }
}

/// Runtime auth state shared by OpenAI provider instances.
#[derive(Clone)]
pub struct OpenAIChatGptAuthHandle {
    session: Arc<Mutex<OpenAIChatGptSession>>,
    auth_config: OpenAIAuthConfig,
    storage_mode: AuthCredentialsStoreMode,
}

impl fmt::Debug for OpenAIChatGptAuthHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAIChatGptAuthHandle")
            .field("auth_config", &self.auth_config)
            .field("storage_mode", &self.storage_mode)
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
            auth_config,
            storage_mode,
        }
    }

    pub fn snapshot(&self) -> Result<OpenAIChatGptSession> {
        self.session
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| anyhow!("openai chatgpt auth mutex poisoned"))
    }

    pub fn current_api_key(&self) -> Result<String> {
        self.snapshot()
            .map(|session| active_api_bearer_token(&session).to_string())
    }

    pub fn provider_label(&self) -> &'static str {
        "OpenAI (ChatGPT)"
    }

    pub async fn refresh_if_needed(&self) -> Result<()> {
        if !self.auth_config.auto_refresh {
            return Ok(());
        }

        let needs_refresh = self.snapshot()?.is_refresh_due();
        if needs_refresh {
            self.force_refresh().await?;
        }
        Ok(())
    }

    pub async fn force_refresh(&self) -> Result<()> {
        let session = self.snapshot()?;
        let refreshed =
            refresh_openai_chatgpt_session_from_snapshot(&session, self.storage_mode).await?;
        self.replace_session(refreshed)
    }

    fn replace_session(&self, session: OpenAIChatGptSession) -> Result<()> {
        let mut guard = self
            .session
            .lock()
            .map_err(|_| anyhow!("openai chatgpt auth mutex poisoned"))?;
        *guard = session;
        Ok(())
    }
}

/// OpenAI auth resolution chosen for the current runtime.
#[derive(Debug, Clone)]
pub enum OpenAIResolvedAuth {
    ApiKey {
        api_key: String,
    },
    ChatGpt {
        api_key: String,
        handle: OpenAIChatGptAuthHandle,
    },
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

    pub fn using_chatgpt(&self) -> bool {
        matches!(self, Self::ChatGpt { .. })
    }
}

fn active_api_bearer_token(session: &OpenAIChatGptSession) -> &str {
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

#[derive(Debug, Clone)]
pub struct OpenAICredentialOverview {
    pub api_key_available: bool,
    pub chatgpt_session: Option<OpenAIChatGptSession>,
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
pub fn get_openai_chatgpt_auth_url(
    challenge: &PkceChallenge,
    callback_port: u16,
    state: &str,
) -> String {
    let redirect_uri = format!("http://localhost:{callback_port}{OPENAI_CALLBACK_PATH}");
    let query = [
        ("response_type", "code".to_string()),
        ("client_id", OPENAI_CLIENT_ID.to_string()),
        ("redirect_uri", redirect_uri),
        (
            "scope",
            "openid profile email offline_access api.connectors.read api.connectors.invoke"
                .to_string(),
        ),
        ("code_challenge", challenge.code_challenge.clone()),
        (
            "code_challenge_method",
            challenge.code_challenge_method.clone(),
        ),
        ("id_token_add_organizations", "true".to_string()),
        ("codex_cli_simplified_flow", "true".to_string()),
        ("state", state.to_string()),
        ("originator", OPENAI_ORIGINATOR.to_string()),
    ];

    let encoded = query
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{OPENAI_AUTH_URL}?{encoded}")
}

pub fn generate_openai_oauth_state() -> Result<String> {
    let mut state_bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut state_bytes)
        .map_err(|_| anyhow!("failed to generate openai oauth state"))?;
    Ok(URL_SAFE_NO_PAD.encode(state_bytes))
}

pub fn parse_openai_chatgpt_manual_callback_input(
    input: &str,
    expected_state: &str,
) -> Result<String> {
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
    let state = extract_query_value(&query, "state")
        .ok_or_else(|| anyhow!("callback input did not include state"))?;
    if state != expected_state {
        bail!("OAuth error: state mismatch");
    }
    Ok(code)
}

/// Exchange an authorization code for OAuth tokens.
pub async fn exchange_openai_chatgpt_code_for_tokens(
    code: &str,
    challenge: &PkceChallenge,
    callback_port: u16,
) -> Result<OpenAIChatGptSession> {
    let redirect_uri = format!("http://localhost:{callback_port}{OPENAI_CALLBACK_PATH}");
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(OPENAI_CLIENT_ID),
        urlencoding::encode(&challenge.code_verifier),
    );

    let token_response: OpenAITokenResponse = Client::new()
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .context("failed to exchange openai authorization code")?
        .error_for_status()
        .context("openai authorization-code exchange failed")?
        .json()
        .await
        .context("failed to parse openai authorization-code response")?;

    build_session_from_token_response(token_response).await
}

/// Resolve the active OpenAI auth source for the current configuration.
pub fn resolve_openai_auth(
    auth_config: &OpenAIAuthConfig,
    storage_mode: AuthCredentialsStoreMode,
    api_key: Option<String>,
) -> Result<OpenAIResolvedAuth> {
    let session = load_openai_chatgpt_session_with_mode(storage_mode)?;
    match auth_config.preferred_method {
        OpenAIPreferredMethod::Chatgpt => {
            let session = session.ok_or_else(|| anyhow!("Run vtcode login openai"))?;
            let handle =
                OpenAIChatGptAuthHandle::new(session.clone(), auth_config.clone(), storage_mode);
            Ok(OpenAIResolvedAuth::ChatGpt {
                api_key: active_api_bearer_token(&session).to_string(),
                handle,
            })
        }
        OpenAIPreferredMethod::ApiKey => {
            let api_key = api_key.ok_or_else(|| anyhow!("OpenAI API key not found"))?;
            Ok(OpenAIResolvedAuth::ApiKey { api_key })
        }
        OpenAIPreferredMethod::Auto => {
            if let Some(session) = session {
                let handle = OpenAIChatGptAuthHandle::new(
                    session.clone(),
                    auth_config.clone(),
                    storage_mode,
                );
                Ok(OpenAIResolvedAuth::ChatGpt {
                    api_key: active_api_bearer_token(&session).to_string(),
                    handle,
                })
            } else {
                let api_key = api_key.ok_or_else(|| anyhow!("OpenAI API key not found"))?;
                Ok(OpenAIResolvedAuth::ApiKey { api_key })
            }
        }
    }
}

pub fn summarize_openai_credentials(
    auth_config: &OpenAIAuthConfig,
    storage_mode: AuthCredentialsStoreMode,
    api_key: Option<String>,
) -> Result<OpenAICredentialOverview> {
    let chatgpt_session = load_openai_chatgpt_session_with_mode(storage_mode)?;
    let api_key_available = api_key
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let active_source = match auth_config.preferred_method {
        OpenAIPreferredMethod::Chatgpt => chatgpt_session
            .as_ref()
            .map(|_| OpenAIResolvedAuthSource::ChatGpt),
        OpenAIPreferredMethod::ApiKey => {
            api_key_available.then_some(OpenAIResolvedAuthSource::ApiKey)
        }
        OpenAIPreferredMethod::Auto => {
            if chatgpt_session.is_some() {
                Some(OpenAIResolvedAuthSource::ChatGpt)
            } else if api_key_available {
                Some(OpenAIResolvedAuthSource::ApiKey)
            } else {
                None
            }
        }
    };

    let (notice, recommendation) = if api_key_available && chatgpt_session.is_some() {
        let active_label = match active_source {
            Some(OpenAIResolvedAuthSource::ChatGpt) => "ChatGPT subscription",
            Some(OpenAIResolvedAuthSource::ApiKey) => "OPENAI_API_KEY",
            None => "neither credential",
        };
        let recommendation = match active_source {
            Some(OpenAIResolvedAuthSource::ChatGpt) => {
                "Next step: keep the current priority, run /logout openai to rely on API-key auth only, or set [auth.openai].preferred_method = \"api_key\"."
            }
            Some(OpenAIResolvedAuthSource::ApiKey) => {
                "Next step: keep the current priority, remove OPENAI_API_KEY if ChatGPT should win, or set [auth.openai].preferred_method = \"chatgpt\"."
            }
            None => {
                "Next step: choose a single preferred source or set [auth.openai].preferred_method explicitly."
            }
        };
        (
            Some(format!(
                "Both ChatGPT subscription auth and OPENAI_API_KEY are available. VT Code is using {active_label} because auth.openai.preferred_method = {}.",
                auth_config.preferred_method.as_str()
            )),
            Some(recommendation.to_string()),
        )
    } else {
        (None, None)
    };

    Ok(OpenAICredentialOverview {
        api_key_available,
        chatgpt_session,
        active_source,
        preferred_method: auth_config.preferred_method,
        notice,
        recommendation,
    })
}

pub fn save_openai_chatgpt_session(session: &OpenAIChatGptSession) -> Result<()> {
    save_openai_chatgpt_session_with_mode(session, AuthCredentialsStoreMode::default())
}

pub fn save_openai_chatgpt_session_with_mode(
    session: &OpenAIChatGptSession,
    mode: AuthCredentialsStoreMode,
) -> Result<()> {
    let serialized =
        serde_json::to_string(session).context("failed to serialize openai session")?;
    match mode.effective_mode() {
        AuthCredentialsStoreMode::Keyring => {
            persist_session_to_keyring_or_file(session, &serialized)?
        }
        AuthCredentialsStoreMode::File => save_session_to_file(session)?,
        AuthCredentialsStoreMode::Auto => unreachable!(),
    }
    Ok(())
}

pub fn load_openai_chatgpt_session() -> Result<Option<OpenAIChatGptSession>> {
    load_preferred_openai_chatgpt_session(AuthCredentialsStoreMode::Keyring)
}

pub fn load_openai_chatgpt_session_with_mode(
    mode: AuthCredentialsStoreMode,
) -> Result<Option<OpenAIChatGptSession>> {
    load_preferred_openai_chatgpt_session(mode.effective_mode())
}

pub fn clear_openai_chatgpt_session() -> Result<()> {
    clear_session_from_all_stores()
}

pub fn clear_openai_chatgpt_session_with_mode(mode: AuthCredentialsStoreMode) -> Result<()> {
    match mode.effective_mode() {
        AuthCredentialsStoreMode::Keyring => clear_session_from_keyring(),
        AuthCredentialsStoreMode::File => clear_session_from_file(),
        AuthCredentialsStoreMode::Auto => unreachable!(),
    }
}

pub fn get_openai_chatgpt_auth_status() -> Result<OpenAIChatGptAuthStatus> {
    get_openai_chatgpt_auth_status_with_mode(AuthCredentialsStoreMode::default())
}

pub fn get_openai_chatgpt_auth_status_with_mode(
    mode: AuthCredentialsStoreMode,
) -> Result<OpenAIChatGptAuthStatus> {
    let Some(session) = load_openai_chatgpt_session_with_mode(mode)? else {
        return Ok(OpenAIChatGptAuthStatus::NotAuthenticated);
    };
    let now = now_secs();
    Ok(OpenAIChatGptAuthStatus::Authenticated {
        label: session
            .email
            .clone()
            .or_else(|| session.plan.clone())
            .or_else(|| session.account_id.clone()),
        age_seconds: now.saturating_sub(session.obtained_at),
        expires_in: session
            .expires_at
            .map(|expires_at| expires_at.saturating_sub(now)),
    })
}

pub async fn refresh_openai_chatgpt_session_from_refresh_token(
    refresh_token: &str,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<OpenAIChatGptSession> {
    let _lock = acquire_refresh_lock().await?;
    refresh_openai_chatgpt_session_without_lock(refresh_token, storage_mode).await
}

pub async fn refresh_openai_chatgpt_session_with_mode(
    mode: AuthCredentialsStoreMode,
) -> Result<OpenAIChatGptSession> {
    let session = load_openai_chatgpt_session_with_mode(mode)?
        .ok_or_else(|| anyhow!("Run vtcode login openai"))?;
    refresh_openai_chatgpt_session_from_snapshot(&session, mode).await
}

async fn refresh_openai_chatgpt_session_from_snapshot(
    session: &OpenAIChatGptSession,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<OpenAIChatGptSession> {
    let _lock = acquire_refresh_lock().await?;
    if let Some(current) = load_openai_chatgpt_session_with_mode(storage_mode)?
        && session_has_newer_refresh_state(&current, session)
    {
        return Ok(current);
    }
    refresh_openai_chatgpt_session_without_lock(&session.refresh_token, storage_mode).await
}

async fn refresh_openai_chatgpt_session_without_lock(
    refresh_token: &str,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<OpenAIChatGptSession> {
    let response = Client::new()
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&client_id={}&refresh_token={}",
            urlencoding::encode(OPENAI_CLIENT_ID),
            urlencoding::encode(refresh_token),
        ))
        .send()
        .await
        .context("failed to refresh openai chatgpt token")?;
    response
        .error_for_status_ref()
        .map_err(classify_refresh_error)?;
    let token_response: OpenAITokenResponse = response
        .json()
        .await
        .context("failed to parse openai refresh response")?;

    let session = build_session_from_token_response(token_response).await?;
    save_openai_chatgpt_session_with_mode(&session, storage_mode)?;
    Ok(session)
}

async fn build_session_from_token_response(
    token_response: OpenAITokenResponse,
) -> Result<OpenAIChatGptSession> {
    let id_claims = parse_jwt_claims(&token_response.id_token)?;
    let access_claims = parse_jwt_claims(&token_response.access_token).ok();
    let api_key = match exchange_openai_chatgpt_api_key(&token_response.id_token).await {
        Ok(api_key) => api_key,
        Err(err) => {
            tracing::warn!(
                "openai api-key exchange unavailable, falling back to oauth access token: {err}"
            );
            String::new()
        }
    };
    let now = now_secs();
    Ok(OpenAIChatGptSession {
        openai_api_key: api_key,
        id_token: token_response.id_token,
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        account_id: access_claims
            .as_ref()
            .and_then(|claims| claims.account_id.clone())
            .or(id_claims.account_id),
        email: id_claims.email.or_else(|| {
            access_claims
                .as_ref()
                .and_then(|claims| claims.email.clone())
        }),
        plan: access_claims
            .as_ref()
            .and_then(|claims| claims.plan.clone())
            .or(id_claims.plan),
        obtained_at: now,
        refreshed_at: now,
        expires_at: token_response
            .expires_in
            .map(|secs| now.saturating_add(secs)),
    })
}

async fn exchange_openai_chatgpt_api_key(id_token: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct ExchangeResponse {
        access_token: String,
    }

    let exchange: ExchangeResponse = Client::new()
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type={}&client_id={}&requested_token={}&subject_token={}&subject_token_type={}",
            urlencoding::encode("urn:ietf:params:oauth:grant-type:token-exchange"),
            urlencoding::encode(OPENAI_CLIENT_ID),
            urlencoding::encode("openai-api-key"),
            urlencoding::encode(id_token),
            urlencoding::encode("urn:ietf:params:oauth:token-type:id_token"),
        ))
        .send()
        .await
        .context("failed to exchange openai id token for api key")?
        .error_for_status()
        .context("openai api-key exchange failed")?
        .json()
        .await
        .context("failed to parse openai api-key exchange response")?;

    Ok(exchange.access_token)
}

#[derive(Debug, Deserialize)]
struct OpenAITokenResponse {
    id_token: String,
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    #[serde(default)]
    email: Option<String>,
    #[serde(rename = "https://api.openai.com/profile", default)]
    profile: Option<ProfileClaims>,
    #[serde(rename = "https://api.openai.com/auth", default)]
    auth: Option<AuthClaims>,
}

#[derive(Debug, Deserialize)]
struct ProfileClaims {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthClaims {
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
    #[serde(default)]
    chatgpt_account_id: Option<String>,
}

#[derive(Debug)]
struct ParsedIdTokenClaims {
    email: Option<String>,
    account_id: Option<String>,
    plan: Option<String>,
}

fn parse_jwt_claims(jwt: &str) -> Result<ParsedIdTokenClaims> {
    let mut parts = jwt.split('.');
    let (_, payload_b64, _) = match (parts.next(), parts.next(), parts.next()) {
        (Some(header), Some(payload), Some(signature))
            if !header.is_empty() && !payload.is_empty() && !signature.is_empty() =>
        {
            (header, payload, signature)
        }
        _ => bail!("invalid openai id token"),
    };

    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .context("failed to decode openai id token payload")?;
    let claims: IdTokenClaims =
        serde_json::from_slice(&payload).context("failed to parse openai id token payload")?;

    Ok(ParsedIdTokenClaims {
        email: claims
            .email
            .or_else(|| claims.profile.and_then(|profile| profile.email)),
        account_id: claims
            .auth
            .as_ref()
            .and_then(|auth| auth.chatgpt_account_id.clone()),
        plan: claims.auth.and_then(|auth| auth.chatgpt_plan_type),
    })
}

fn extract_query_value(query: &str, key: &str) -> Option<String> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| {
            let (pair_key, pair_value) = pair.split_once('=')?;
            (pair_key == key)
                .then(|| {
                    urlencoding::decode(pair_value)
                        .ok()
                        .map(|value| value.into_owned())
                })
                .flatten()
        })
        .find(|value| !value.is_empty())
}

fn session_has_newer_refresh_state(
    current: &OpenAIChatGptSession,
    previous: &OpenAIChatGptSession,
) -> bool {
    current.refresh_token != previous.refresh_token
        || current.refreshed_at > previous.refreshed_at
        || current.obtained_at > previous.obtained_at
}

struct RefreshLockGuard {
    file: fs::File,
}

impl Drop for RefreshLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

async fn acquire_refresh_lock() -> Result<RefreshLockGuard> {
    let path = auth_storage_dir()?.join(OPENAI_REFRESH_LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("failed to open openai refresh lock {}", path.display()))?;
    let file = tokio::task::spawn_blocking(move || {
        file.lock_exclusive()
            .context("failed to acquire openai refresh lock")?;
        Ok::<_, anyhow::Error>(file)
    })
    .await
    .context("openai refresh lock task failed")??;
    Ok(RefreshLockGuard { file })
}

fn classify_refresh_error(err: reqwest::Error) -> anyhow::Error {
    let status = err.status();
    let message = err.to_string();
    if status.is_some_and(|status| status == reqwest::StatusCode::BAD_REQUEST)
        && (message.contains("invalid_grant") || message.contains("refresh_token"))
    {
        if let Err(clear_err) = clear_session_from_all_stores() {
            tracing::warn!(
                "failed to clear expired openai chatgpt session across all stores: {clear_err}"
            );
        }
        anyhow!("Your ChatGPT session expired. Run `vtcode login openai` again.")
    } else {
        anyhow!(message)
    }
}

fn clear_session_from_all_stores() -> Result<()> {
    let mut errors = Vec::new();

    if let Err(err) = clear_session_from_keyring() {
        errors.push(err.to_string());
    }
    if let Err(err) = clear_session_from_file() {
        errors.push(err.to_string());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "failed to clear openai session from all stores: {}",
            errors.join("; ")
        ))
    }
}

fn save_session_to_keyring(serialized: &str) -> Result<()> {
    let entry = keyring::Entry::new(OPENAI_STORAGE_SERVICE, OPENAI_STORAGE_USER)
        .context("failed to access keyring for openai session")?;
    entry
        .set_password(serialized)
        .context("failed to store openai session in keyring")?;
    Ok(())
}

fn persist_session_to_keyring_or_file(
    session: &OpenAIChatGptSession,
    serialized: &str,
) -> Result<()> {
    match save_session_to_keyring(serialized) {
        Ok(()) => match load_session_from_keyring_decoded() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => {
                tracing::warn!(
                    "openai session keyring write did not round-trip; falling back to encrypted file storage"
                );
                save_session_to_file(session)
            }
            Err(err) => {
                tracing::warn!(
                    "openai session keyring verification failed, falling back to encrypted file storage: {err}"
                );
                save_session_to_file(session)
            }
        },
        Err(err) => {
            tracing::warn!(
                "failed to persist openai session in keyring, falling back to encrypted file storage: {err}"
            );
            save_session_to_file(session)
                .context("failed to persist openai session after keyring fallback")
        }
    }
}

fn decode_session_from_keyring(serialized: String) -> Result<OpenAIChatGptSession> {
    serde_json::from_str(&serialized).context("failed to decode openai session")
}

fn load_session_from_keyring_decoded() -> Result<Option<OpenAIChatGptSession>> {
    load_session_from_keyring()?
        .map(decode_session_from_keyring)
        .transpose()
}

fn load_preferred_openai_chatgpt_session(
    mode: AuthCredentialsStoreMode,
) -> Result<Option<OpenAIChatGptSession>> {
    match mode {
        AuthCredentialsStoreMode::Keyring => match load_session_from_keyring_decoded() {
            Ok(Some(session)) => Ok(Some(session)),
            Ok(None) => load_session_from_file(),
            Err(err) => {
                tracing::warn!(
                    "failed to load openai session from keyring, falling back to encrypted file: {err}"
                );
                load_session_from_file()
            }
        },
        AuthCredentialsStoreMode::File => {
            if let Some(session) = load_session_from_file()? {
                return Ok(Some(session));
            }
            load_session_from_keyring_decoded()
        }
        AuthCredentialsStoreMode::Auto => unreachable!(),
    }
}

fn load_session_from_keyring() -> Result<Option<String>> {
    let entry = match keyring::Entry::new(OPENAI_STORAGE_SERVICE, OPENAI_STORAGE_USER) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };

    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(anyhow!("failed to read openai session from keyring: {err}")),
    }
}

fn clear_session_from_keyring() -> Result<()> {
    let entry = match keyring::Entry::new(OPENAI_STORAGE_SERVICE, OPENAI_STORAGE_USER) {
        Ok(entry) => entry,
        Err(_) => return Ok(()),
    };

    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(anyhow!(
            "failed to clear openai session keyring entry: {err}"
        )),
    }
}

fn save_session_to_file(session: &OpenAIChatGptSession) -> Result<()> {
    let encrypted = encrypt_session(session)?;
    let path = get_session_path()?;
    fs::write(&path, serde_json::to_vec_pretty(&encrypted)?)
        .context("failed to persist openai session file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .context("failed to set openai session file permissions")?;
    }
    Ok(())
}

fn load_session_from_file() -> Result<Option<OpenAIChatGptSession>> {
    let path = get_session_path()?;
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(anyhow!("failed to read openai session file: {err}")),
    };

    let encrypted: EncryptedSession =
        serde_json::from_slice(&data).context("failed to decode openai session file")?;
    Ok(Some(decrypt_session(&encrypted)?))
}

fn clear_session_from_file() -> Result<()> {
    let path = get_session_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(anyhow!("failed to delete openai session file: {err}")),
    }
}

fn get_session_path() -> Result<PathBuf> {
    Ok(auth_storage_dir()?.join(OPENAI_SESSION_FILE))
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedSession {
    nonce: String,
    ciphertext: String,
    version: u8,
}

fn encrypt_session(session: &OpenAIChatGptSession) -> Result<EncryptedSession> {
    let key = derive_encryption_key()?;
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow!("failed to generate nonce"))?;

    let mut ciphertext =
        serde_json::to_vec(session).context("failed to serialize openai session for encryption")?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
        .map_err(|_| anyhow!("failed to encrypt openai session"))?;

    Ok(EncryptedSession {
        nonce: STANDARD.encode(nonce_bytes),
        ciphertext: STANDARD.encode(ciphertext),
        version: 1,
    })
}

fn decrypt_session(encrypted: &EncryptedSession) -> Result<OpenAIChatGptSession> {
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

    let key = derive_encryption_key()?;
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_array),
            Aad::empty(),
            &mut ciphertext,
        )
        .map_err(|_| anyhow!("failed to decrypt openai session"))?;
    serde_json::from_slice(plaintext).context("failed to parse decrypted openai session")
}

fn derive_encryption_key() -> Result<LessSafeKey> {
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
    let key_bytes: &[u8; 32] = hash.as_ref()[..32]
        .try_into()
        .context("openai session encryption key was too short")?;
    let unbound = UnboundKey::new(&aead::AES_256_GCM, key_bytes)
        .map_err(|_| anyhow!("invalid openai session encryption key"))?;
    Ok(LessSafeKey::new(unbound))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use serial_test::serial;

    struct TestAuthDirGuard {
        temp_dir: Option<TempDir>,
        previous: Option<PathBuf>,
    }

    impl TestAuthDirGuard {
        fn new() -> Self {
            let temp_dir = TempDir::new().expect("create temp auth dir");
            let previous = crate::storage_paths::auth_storage_dir_override_for_tests()
                .expect("read auth dir override");
            crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(
                temp_dir.path().to_path_buf(),
            ))
            .expect("set temp auth dir override");
            Self {
                temp_dir: Some(temp_dir),
                previous,
            }
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

    fn sample_session() -> OpenAIChatGptSession {
        OpenAIChatGptSession {
            openai_api_key: "api-key".to_string(),
            id_token: "aGVhZGVy.eyJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjXzEyMyIsImNoYXRncHRfcGxhbl90eXBlIjoicGx1cyJ9fQ.sig".to_string(),
            access_token: "oauth-access".to_string(),
            refresh_token: "refresh-token".to_string(),
            account_id: Some("acc_123".to_string()),
            email: Some("test@example.com".to_string()),
            plan: Some("plus".to_string()),
            obtained_at: 10,
            refreshed_at: 10,
            expires_at: Some(now_secs() + 3600),
        }
    }

    #[test]
    fn auth_url_contains_expected_openai_parameters() {
        let challenge = PkceChallenge {
            code_verifier: "verifier".to_string(),
            code_challenge: "challenge".to_string(),
            code_challenge_method: "S256".to_string(),
        };

        let url = get_openai_chatgpt_auth_url(&challenge, 1455, "test-state");
        assert!(url.starts_with(OPENAI_AUTH_URL));
        assert!(url.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
        assert!(url.contains("state=test-state"));
    }

    #[test]
    fn parse_jwt_claims_extracts_openai_claims() {
        let claims = parse_jwt_claims(
            "aGVhZGVy.eyJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjXzEyMyIsImNoYXRncHRfcGxhbl90eXBlIjoicGx1cyJ9fQ.sig",
        )
        .expect("claims");
        assert_eq!(claims.email.as_deref(), Some("test@example.com"));
        assert_eq!(claims.account_id.as_deref(), Some("acc_123"));
        assert_eq!(claims.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn session_refresh_due_uses_expiry_and_age() {
        let mut session = sample_session();
        let now = now_secs();
        session.obtained_at = now;
        session.refreshed_at = now;
        session.expires_at = Some(now + 3600);
        assert!(!session.is_refresh_due());
        session.expires_at = Some(now);
        assert!(session.is_refresh_due());
    }

    #[test]
    #[serial]
    fn resolve_openai_auth_prefers_chatgpt_in_auto_mode() {
        let _guard = TestAuthDirGuard::new();
        let session = sample_session();
        save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File)
            .expect("save session");
        let resolved = resolve_openai_auth(
            &OpenAIAuthConfig::default(),
            AuthCredentialsStoreMode::File,
            Some("api-key".to_string()),
        )
        .expect("resolved auth");
        assert!(resolved.using_chatgpt());
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
    }

    #[test]
    #[serial]
    fn resolve_openai_auth_auto_falls_back_to_api_key_without_session() {
        let _guard = TestAuthDirGuard::new();
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
        let resolved = resolve_openai_auth(
            &OpenAIAuthConfig::default(),
            AuthCredentialsStoreMode::File,
            Some("api-key".to_string()),
        )
        .expect("resolved auth");
        assert!(matches!(resolved, OpenAIResolvedAuth::ApiKey { .. }));
    }

    #[test]
    #[serial]
    fn resolve_openai_auth_api_key_mode_ignores_stored_chatgpt_session() {
        let _guard = TestAuthDirGuard::new();
        let session = sample_session();
        save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File)
            .expect("save session");
        let resolved = resolve_openai_auth(
            &OpenAIAuthConfig {
                preferred_method: OpenAIPreferredMethod::ApiKey,
                ..OpenAIAuthConfig::default()
            },
            AuthCredentialsStoreMode::File,
            Some("api-key".to_string()),
        )
        .expect("resolved auth");
        assert!(matches!(resolved, OpenAIResolvedAuth::ApiKey { .. }));
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
    }

    #[test]
    #[serial]
    fn resolve_openai_auth_chatgpt_mode_requires_stored_session() {
        let _guard = TestAuthDirGuard::new();
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
        let error = resolve_openai_auth(
            &OpenAIAuthConfig {
                preferred_method: OpenAIPreferredMethod::Chatgpt,
                ..OpenAIAuthConfig::default()
            },
            AuthCredentialsStoreMode::File,
            Some("api-key".to_string()),
        )
        .expect_err("chatgpt mode should require a stored session");
        assert!(error.to_string().contains("vtcode login openai"));
    }

    #[test]
    #[serial]
    fn summarize_openai_credentials_reports_dual_source_notice() {
        let _guard = TestAuthDirGuard::new();
        let session = sample_session();
        save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File)
            .expect("save session");
        let overview = summarize_openai_credentials(
            &OpenAIAuthConfig::default(),
            AuthCredentialsStoreMode::File,
            Some("api-key".to_string()),
        )
        .expect("overview");
        assert_eq!(
            overview.active_source,
            Some(OpenAIResolvedAuthSource::ChatGpt)
        );
        assert!(overview.notice.is_some());
        assert!(overview.recommendation.is_some());
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
    }

    #[test]
    #[serial]
    fn summarize_openai_credentials_respects_api_key_preference() {
        let _guard = TestAuthDirGuard::new();
        let session = sample_session();
        save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File)
            .expect("save session");
        let overview = summarize_openai_credentials(
            &OpenAIAuthConfig {
                preferred_method: OpenAIPreferredMethod::ApiKey,
                ..OpenAIAuthConfig::default()
            },
            AuthCredentialsStoreMode::File,
            Some("api-key".to_string()),
        )
        .expect("overview");
        assert_eq!(
            overview.active_source,
            Some(OpenAIResolvedAuthSource::ApiKey)
        );
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
    }

    #[test]
    fn encrypted_file_round_trip_restores_session() {
        let session = sample_session();
        let encrypted = encrypt_session(&session).expect("encrypt");
        let decrypted = decrypt_session(&encrypted).expect("decrypt");
        assert_eq!(decrypted.account_id, session.account_id);
        assert_eq!(decrypted.email, session.email);
        assert_eq!(decrypted.plan, session.plan);
    }

    #[test]
    #[serial]
    fn default_loader_falls_back_to_file_session() {
        let _guard = TestAuthDirGuard::new();
        let session = sample_session();
        save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File)
            .expect("save session");

        let loaded = load_openai_chatgpt_session()
            .expect("load session")
            .expect("stored session should be found");

        assert_eq!(loaded.account_id, session.account_id);
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
    }

    #[test]
    #[serial]
    fn keyring_mode_loader_falls_back_to_file_session() {
        let _guard = TestAuthDirGuard::new();
        let session = sample_session();
        save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File)
            .expect("save session");

        let loaded = load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring)
            .expect("load session")
            .expect("stored session should be found");

        assert_eq!(loaded.email, session.email);
        clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("clear session");
    }

    #[test]
    #[serial]
    fn clear_openai_chatgpt_session_removes_file_and_keyring_sessions() {
        let _guard = TestAuthDirGuard::new();
        let session = sample_session();
        save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File)
            .expect("save file session");

        if save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::Keyring)
            .is_err()
        {
            clear_openai_chatgpt_session().expect("clear session");
            assert!(
                load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
                    .expect("load file session")
                    .is_none()
            );
            return;
        }

        clear_openai_chatgpt_session().expect("clear session");
        assert!(
            load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
                .expect("load file session")
                .is_none()
        );
        assert!(
            load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring)
                .expect("load keyring session")
                .is_none()
        );
    }

    #[test]
    fn active_api_bearer_token_falls_back_to_access_token() {
        let mut session = sample_session();
        session.openai_api_key.clear();

        assert_eq!(active_api_bearer_token(&session), "oauth-access");
    }

    #[test]
    fn parse_manual_callback_input_accepts_full_redirect_url() {
        let code = parse_openai_chatgpt_manual_callback_input(
            "http://localhost:1455/auth/callback?code=auth-code&state=test-state",
            "test-state",
        )
        .expect("manual input should parse");
        assert_eq!(code, "auth-code");
    }

    #[test]
    fn parse_manual_callback_input_accepts_query_string() {
        let code = parse_openai_chatgpt_manual_callback_input(
            "code=auth-code&state=test-state",
            "test-state",
        )
        .expect("manual input should parse");
        assert_eq!(code, "auth-code");
    }

    #[test]
    fn parse_manual_callback_input_rejects_bare_code() {
        let error = parse_openai_chatgpt_manual_callback_input("auth-code", "test-state")
            .expect_err("bare code should be rejected");
        assert!(
            error
                .to_string()
                .contains("full redirect url or query string")
        );
    }

    #[test]
    fn parse_manual_callback_input_rejects_state_mismatch() {
        let error = parse_openai_chatgpt_manual_callback_input(
            "code=auth-code&state=wrong-state",
            "test-state",
        )
        .expect_err("state mismatch should fail");
        assert!(error.to_string().contains("state mismatch"));
    }

    #[tokio::test]
    #[serial]
    async fn refresh_lock_serializes_parallel_acquisition() {
        let _guard = TestAuthDirGuard::new();
        let first = tokio::spawn(async {
            let _lock = acquire_refresh_lock().await.expect("first lock");
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let start = std::time::Instant::now();
        let second = tokio::spawn(async {
            let _lock = acquire_refresh_lock().await.expect("second lock");
        });

        first.await.expect("first task");
        second.await.expect("second task");
        assert!(start.elapsed() >= std::time::Duration::from_millis(100));
    }
}
