//! Token exchange, refresh, storage wrappers, and refresh-lock helpers.

use anyhow::{Context, Result, anyhow, bail};
use fs2::FileExt;
use reqwest::Client;
use serde::Deserialize;
use std::fs;
use std::fs::OpenOptions;

use crate::openai_refresh_policy::{RefreshFailureAction, classify_refresh_failure};
use crate::openai_session_storage::OpenAiSessionStorage;
use crate::storage_paths::auth_storage_dir;

pub use super::super::credentials::AuthCredentialsStoreMode;
use super::super::pkce::PkceChallenge;
use super::jwt::{parse_jwt_claims, parse_jwt_exp};
use super::session::{OpenAIChatGptAuthStatus, OpenAIChatGptSession, active_api_bearer_token};
use super::{
    MAX_ERROR_BODY_BYTES, OPENAI_CALLBACK_PATH, OPENAI_REFRESH_LOCK_FILE, OPENAI_TOKEN_URL, now_secs,
    resolve_oauth_client_identity,
};

/// Exchange an authorization code for OAuth tokens.
pub async fn exchange_openai_chatgpt_code_for_tokens(
    code: &str,
    challenge: &PkceChallenge,
    callback_port: u16,
) -> Result<OpenAIChatGptSession> {
    let redirect_uri = format!("http://localhost:{callback_port}{OPENAI_CALLBACK_PATH}");
    let identity = resolve_oauth_client_identity()?;
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&identity.client_id),
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
pub fn save_openai_chatgpt_session(session: &OpenAIChatGptSession) -> Result<()> {
    save_openai_chatgpt_session_with_mode(session, AuthCredentialsStoreMode::default())
}

pub fn save_openai_chatgpt_session_with_mode(
    session: &OpenAIChatGptSession,
    mode: AuthCredentialsStoreMode,
) -> Result<()> {
    OpenAiSessionStorage::new().save(session, mode)
}

pub fn load_openai_chatgpt_session() -> Result<Option<OpenAIChatGptSession>> {
    OpenAiSessionStorage::new().load(AuthCredentialsStoreMode::Keyring)
}

pub fn load_openai_chatgpt_session_with_mode(mode: AuthCredentialsStoreMode) -> Result<Option<OpenAIChatGptSession>> {
    OpenAiSessionStorage::new().load(mode)
}

pub fn clear_openai_chatgpt_session() -> Result<()> {
    OpenAiSessionStorage::new().clear_all()
}

pub fn clear_openai_chatgpt_session_with_mode(mode: AuthCredentialsStoreMode) -> Result<()> {
    OpenAiSessionStorage::new().clear(mode)
}

pub fn get_openai_chatgpt_auth_status() -> Result<OpenAIChatGptAuthStatus> {
    get_openai_chatgpt_auth_status_with_mode(AuthCredentialsStoreMode::default())
}

pub fn get_openai_chatgpt_auth_status_with_mode(mode: AuthCredentialsStoreMode) -> Result<OpenAIChatGptAuthStatus> {
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
        expires_in: session.expires_at.map(|expires_at| expires_at.saturating_sub(now)),
    })
}

pub async fn refresh_openai_chatgpt_session_with_mode(mode: AuthCredentialsStoreMode) -> Result<OpenAIChatGptSession> {
    let session = load_openai_chatgpt_session_with_mode(mode)?.ok_or_else(|| anyhow!("Run vtcode login openai"))?;
    refresh_openai_chatgpt_session_from_snapshot(&session, mode).await
}

pub(super) async fn refresh_openai_chatgpt_session_from_snapshot(
    session: &OpenAIChatGptSession,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<OpenAIChatGptSession> {
    let _lock = acquire_refresh_lock().await?;
    if let Some(current) = load_openai_chatgpt_session_with_mode(storage_mode)?
        && session_has_newer_refresh_state(&current, session)
    {
        return Ok(current);
    }
    refresh_openai_chatgpt_session_without_lock(session, storage_mode).await
}

/// Refresh the ChatGPT session using the stored refresh token.
///
/// The response is parsed as [`OpenAIRefreshResponse`] with independently
/// optional fields — OpenAI's token endpoint may omit unchanged fields.
/// Omitted fields preserve the current session's values. This matches the
/// behavior of `openai/codex`'s `RefreshResponse` + `persist_tokens`.
async fn refresh_openai_chatgpt_session_without_lock(
    current: &OpenAIChatGptSession,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<OpenAIChatGptSession> {
    let identity = resolve_oauth_client_identity()?;
    let response = Client::new()
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&client_id={}&refresh_token={}",
            urlencoding::encode(&identity.client_id),
            urlencoding::encode(&current.refresh_token),
        ))
        .send()
        .await
        .context("failed to refresh openai chatgpt token")?;

    // Check for HTTP errors. Unlike error_for_status_ref(), we capture the
    // response body to classify token-endpoint errors (e.g. invalid_grant,
    // refresh_token_expired) that reqwest's status-only error would miss.
    if !response.status().is_success() {
        let status = response.status();
        // Read a bounded body for error classification — never log it raw.
        let body_text = read_bounded_text(response, MAX_ERROR_BODY_BYTES).await;
        return Err(classify_refresh_status_error(status, &body_text));
    }

    let refresh_response: OpenAIRefreshResponse =
        response.json().await.context("failed to parse openai refresh response")?;

    let session = merge_refresh_response(current, refresh_response).await?;
    // Guard against a blank access_token — the primary bearer credential.
    // This protects the minimal-session refresh helper (which starts with
    // blank token fields) from persisting a session with blank tokens when
    // the token endpoint returns a partial response that omits access_token.
    if session.access_token.trim().is_empty() {
        bail!("openai token refresh returned no access token — the session cannot be used");
    }
    save_openai_chatgpt_session_with_mode(&session, storage_mode)?;
    Ok(session)
}

/// Merge a partial refresh response into the current session, preserving
/// omitted fields. Only re-exchanges the API key when a new `id_token` is
/// present; otherwise keeps the previous exchanged key.
pub(crate) async fn merge_refresh_response(
    current: &OpenAIChatGptSession,
    resp: OpenAIRefreshResponse,
) -> Result<OpenAIChatGptSession> {
    let now = now_secs();
    // Track which fields were present before moving them out of resp.
    // Treat blank-string values as absent — some token endpoints return
    // empty strings for omitted fields rather than leaving them out.
    let has_new_id_token = resp.id_token.as_deref().is_some_and(|v| !v.trim().is_empty());
    let has_new_access_token = resp.access_token.as_deref().is_some_and(|v| !v.trim().is_empty());
    let new_id_token = resp
        .id_token
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| current.id_token.clone());
    let new_access_token = resp
        .access_token
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| current.access_token.clone());
    let new_refresh_token = resp
        .refresh_token
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| current.refresh_token.clone());

    // Re-exchange the API key only when a new id_token was provided.
    let openai_api_key = if has_new_id_token {
        match exchange_openai_chatgpt_api_key(&new_id_token).await {
            Ok(api_key) => api_key,
            Err(err) => {
                tracing::warn!("openai api-key exchange unavailable, falling back to previous key: {err}");
                current.openai_api_key.clone()
            }
        }
    } else {
        current.openai_api_key.clone()
    };

    // Recompute expiry: prefer expires_in from the response, then try to parse
    // exp from the new access_token JWT. For a **changed** access token without
    // expires_in or a parseable exp, set None — do NOT inherit the previous
    // token's expiry, which belongs to a different token.
    //
    // Key distinction: `has_new_access_token` means the field was present and
    // non-blank, NOT that the token value changed. If the endpoint repeats the
    // same opaque access token without expires_in, the old expiry is still
    // valid and must be preserved.
    let access_token_changed = has_new_access_token && new_access_token != current.access_token;
    let expires_at = if let Some(secs) = resp.expires_in {
        Some(now.saturating_add(secs))
    } else if access_token_changed {
        parse_jwt_exp(&new_access_token)
    } else {
        current.expires_at
    };

    // Update email/plan/account_id only when a new id_token was provided.
    let (email, plan, account_id) = if has_new_id_token {
        let id_claims = parse_jwt_claims(&new_id_token)?;
        let access_claims = parse_jwt_claims(&new_access_token).ok();
        let email = id_claims.email.clone();
        let plan = access_claims.as_ref().and_then(|c| c.plan.clone()).or(id_claims.plan);
        let account_id = access_claims
            .as_ref()
            .and_then(|c| c.account_id.clone())
            .or(id_claims.account_id);
        (email, plan, account_id)
    } else {
        (current.email.clone(), current.plan.clone(), current.account_id.clone())
    };

    Ok(OpenAIChatGptSession {
        openai_api_key,
        id_token: new_id_token,
        access_token: new_access_token,
        refresh_token: new_refresh_token,
        account_id,
        email,
        plan,
        // Preserve the original obtained_at — only refreshed_at advances.
        obtained_at: current.obtained_at,
        refreshed_at: now,
        expires_at,
    })
}

async fn build_session_from_token_response(token_response: OpenAITokenResponse) -> Result<OpenAIChatGptSession> {
    // Validate that the token response contains usable credentials.
    if token_response.access_token.trim().is_empty() {
        bail!("openai authorization-code response did not include a usable access token");
    }
    if token_response.refresh_token.trim().is_empty() {
        bail!("openai authorization-code response did not include a usable refresh token");
    }
    let id_claims = parse_jwt_claims(&token_response.id_token)?;
    let access_claims = parse_jwt_claims(&token_response.access_token).ok();
    let api_key = match exchange_openai_chatgpt_api_key(&token_response.id_token).await {
        Ok(api_key) => api_key,
        Err(err) => {
            tracing::warn!("openai api-key exchange unavailable, falling back to oauth access token: {err}");
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
        email: id_claims
            .email
            .or_else(|| access_claims.as_ref().and_then(|claims| claims.email.clone())),
        plan: access_claims.as_ref().and_then(|claims| claims.plan.clone()).or(id_claims.plan),
        obtained_at: now,
        refreshed_at: now,
        expires_at: token_response.expires_in.map(|secs| now.saturating_add(secs)),
    })
}

async fn exchange_openai_chatgpt_api_key(id_token: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct ExchangeResponse {
        access_token: String,
    }

    let identity = resolve_oauth_client_identity()?;
    let exchange: ExchangeResponse = Client::new()
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type={}&client_id={}&requested_token={}&subject_token={}&subject_token_type={}",
            urlencoding::encode("urn:ietf:params:oauth:grant-type:token-exchange"),
            urlencoding::encode(&identity.client_id),
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
#[derive(Deserialize)]
struct OpenAITokenResponse {
    id_token: String,
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Refresh-token grant response — all token fields are independently optional
/// because OpenAI's token endpoint may omit unchanged fields (matching the
/// behavior observed in `openai/codex`'s `RefreshResponse`). Omitted fields
/// preserve the previous session's values during merge.
#[derive(Deserialize)]
pub(crate) struct OpenAIRefreshResponse {
    #[serde(default)]
    pub(crate) id_token: Option<String>,
    #[serde(default)]
    pub(crate) access_token: Option<String>,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) expires_in: Option<u64>,
}
fn session_has_newer_refresh_state(current: &OpenAIChatGptSession, previous: &OpenAIChatGptSession) -> bool {
    current.refresh_token != previous.refresh_token
        || current.refreshed_at > previous.refreshed_at
        || current.obtained_at > previous.obtained_at
}

pub(crate) struct RefreshLockGuard {
    file: fs::File,
}

impl Drop for RefreshLockGuard {
    fn drop(&mut self) {
        drop(FileExt::unlock(&self.file));
    }
}

pub(crate) async fn acquire_refresh_lock() -> Result<RefreshLockGuard> {
    let path = auth_storage_dir()?.join(OPENAI_REFRESH_LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .context("failed to open openai refresh lock")?;
    let file = tokio::task::spawn_blocking(move || {
        file.lock_exclusive().context("failed to acquire openai refresh lock")?;
        Ok::<_, anyhow::Error>(file)
    })
    .await
    .context("openai refresh lock task failed")??;
    Ok(RefreshLockGuard { file })
}

/// Read at most `max_bytes` from an HTTP response body as a string.
///
/// Reads the response in chunks and stops once `max_bytes` have been
/// accumulated, preventing unbounded memory allocation from a misbehaving or
/// hostile endpoint. Invalid UTF-8 sequences are replaced (lossy) since we
/// only use the text for best-effort error classification, never for display
/// or logging.
async fn read_bounded_text(mut response: reqwest::Response, max_bytes: usize) -> String {
    let mut buf = Vec::with_capacity(max_bytes.min(8 * 1024));
    while buf.len() < max_bytes {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = max_bytes - buf.len();
                if chunk.len() <= remaining {
                    buf.extend_from_slice(&chunk);
                } else {
                    buf.extend_from_slice(chunk.get(..remaining).unwrap_or_default());
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

pub(crate) fn classify_refresh_status_error(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    let failure = classify_refresh_failure(status, body);
    if failure.action() == RefreshFailureAction::ClearStoredSession {
        if let Err(clear_err) = clear_session_from_all_stores() {
            tracing::warn!("failed to clear expired openai chatgpt session across all stores: {clear_err}");
        }
    }
    failure.into_error()
}

fn clear_session_from_all_stores() -> Result<()> {
    OpenAiSessionStorage::new().clear_all()
}
