//! OAuth support for HTTP MCP providers.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::{Client, Url};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::credentials::{AuthCredentialsStoreMode, CredentialStorage};
use crate::pkce::{PkceChallenge, generate_pkce_challenge};

const DEFAULT_CALLBACK_PORT: u16 = 8768;
const DEFAULT_FLOW_TIMEOUT_SECS: u64 = 300;
const REFRESH_SKEW_SECS: u64 = 60;

/// Configuration for OAuth-enabled MCP HTTP providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct McpOAuthConfig {
    /// OAuth authorization endpoint.
    pub authorization_url: String,
    /// OAuth token endpoint.
    pub token_url: String,
    /// OAuth client identifier.
    pub client_id: String,
    /// Requested scopes.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Optional audience/resource hint sent with the auth and token requests.
    #[serde(default)]
    pub audience: Option<String>,
    /// Local callback server port.
    pub callback_port: u16,
    /// Browser-flow timeout in seconds.
    pub flow_timeout_secs: u64,
    /// Credential storage backend for this provider's token.
    #[serde(default)]
    pub credentials_store_mode: AuthCredentialsStoreMode,
    /// Extra query parameters appended to the authorization URL.
    #[serde(default)]
    pub extra_auth_params: BTreeMap<String, String>,
    /// Extra form fields appended to token exchanges and refreshes.
    #[serde(default)]
    pub extra_token_params: BTreeMap<String, String>,
}

impl Default for McpOAuthConfig {
    fn default() -> Self {
        Self {
            authorization_url: String::new(),
            token_url: String::new(),
            client_id: String::new(),
            scopes: Vec::new(),
            audience: None,
            callback_port: DEFAULT_CALLBACK_PORT,
            flow_timeout_secs: DEFAULT_FLOW_TIMEOUT_SECS,
            credentials_store_mode: AuthCredentialsStoreMode::default(),
            extra_auth_params: BTreeMap::new(),
            extra_token_params: BTreeMap::new(),
        }
    }
}

impl McpOAuthConfig {
    pub fn validate(&self, provider_name: &str) -> Result<()> {
        if self.authorization_url.trim().is_empty() {
            bail!(
                "MCP provider '{}' is missing oauth.authorization_url",
                provider_name
            );
        }
        if self.token_url.trim().is_empty() {
            bail!(
                "MCP provider '{}' is missing oauth.token_url",
                provider_name
            );
        }
        if self.client_id.trim().is_empty() {
            bail!(
                "MCP provider '{}' is missing oauth.client_id",
                provider_name
            );
        }
        Ok(())
    }

    fn callback_url(&self) -> String {
        format!("http://localhost:{}/auth/callback", self.callback_port)
    }
}

/// Stored OAuth token for an MCP HTTP provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub obtained_at: u64,
    pub expires_at: Option<u64>,
}

impl McpOAuthToken {
    pub fn is_refresh_due(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| now_secs().saturating_add(REFRESH_SKEW_SECS) >= expires_at)
    }
}

/// Status for an MCP provider's stored OAuth token.
#[derive(Debug, Clone)]
pub enum McpOAuthStatus {
    Authenticated {
        age_seconds: u64,
        expires_in: Option<u64>,
    },
    NotAuthenticated,
}

/// Prepared browser-login flow for an MCP OAuth provider.
#[derive(Debug, Clone)]
pub struct McpOAuthPreparedLogin {
    pub auth_url: String,
    pub callback_port: u16,
    pub timeout_secs: u64,
    pkce: PkceChallenge,
    state: String,
}

impl McpOAuthPreparedLogin {
    #[must_use]
    pub fn expected_state(&self) -> &str {
        &self.state
    }
}

/// Completion payload kept intentionally close to Codex app-server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthLoginCompletion {
    pub name: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Service for loading, refreshing, and persisting MCP OAuth tokens.
#[derive(Debug, Clone, Default)]
pub struct McpOAuthService;

impl McpOAuthService {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn prepare_login(
        &self,
        provider_name: &str,
        config: &McpOAuthConfig,
    ) -> Result<McpOAuthPreparedLogin> {
        config.validate(provider_name)?;
        let pkce = generate_pkce_challenge()?;
        let state = generate_state()?;
        let auth_url = build_auth_url(config, &pkce, &state)?;
        Ok(McpOAuthPreparedLogin {
            auth_url,
            callback_port: config.callback_port,
            timeout_secs: config.flow_timeout_secs,
            pkce,
            state,
        })
    }

    pub async fn complete_login(
        &self,
        provider_name: &str,
        config: &McpOAuthConfig,
        prepared: &McpOAuthPreparedLogin,
        code: &str,
    ) -> Result<McpOAuthLoginCompletion> {
        config.validate(provider_name)?;
        let token = exchange_code_for_token(config, code, &prepared.pkce).await?;
        save_token(provider_name, &token, config.credentials_store_mode)?;
        Ok(McpOAuthLoginCompletion {
            name: provider_name.to_string(),
            success: true,
            error: None,
        })
    }

    pub fn status(
        &self,
        provider_name: &str,
        storage_mode: AuthCredentialsStoreMode,
    ) -> Result<McpOAuthStatus> {
        let Some(token) = load_token(provider_name, storage_mode)? else {
            return Ok(McpOAuthStatus::NotAuthenticated);
        };
        let now = now_secs();
        Ok(McpOAuthStatus::Authenticated {
            age_seconds: now.saturating_sub(token.obtained_at),
            expires_in: token
                .expires_at
                .map(|expires_at| expires_at.saturating_sub(now)),
        })
    }

    pub fn load_token(
        &self,
        provider_name: &str,
        storage_mode: AuthCredentialsStoreMode,
    ) -> Result<Option<McpOAuthToken>> {
        load_token(provider_name, storage_mode)
    }

    pub async fn resolve_access_token(
        &self,
        provider_name: &str,
        config: &McpOAuthConfig,
    ) -> Result<Option<String>> {
        let Some(mut token) = load_token(provider_name, config.credentials_store_mode)? else {
            return Ok(None);
        };

        if token.is_refresh_due() {
            if token.refresh_token.is_some() {
                token = refresh_token(config, &token).await?;
                save_token(provider_name, &token, config.credentials_store_mode)?;
            } else {
                bail!(
                    "Stored MCP OAuth token for '{}' expired and cannot be refreshed. Run `vtcode mcp login {}` again.",
                    provider_name,
                    provider_name
                );
            }
        }

        Ok(Some(token.access_token))
    }

    pub fn logout(
        &self,
        provider_name: &str,
        storage_mode: AuthCredentialsStoreMode,
    ) -> Result<McpOAuthLoginCompletion> {
        clear_token(provider_name, storage_mode)?;
        Ok(McpOAuthLoginCompletion {
            name: provider_name.to_string(),
            success: true,
            error: None,
        })
    }
}

fn build_auth_url(
    config: &McpOAuthConfig,
    challenge: &PkceChallenge,
    state: &str,
) -> Result<String> {
    let mut url =
        Url::parse(&config.authorization_url).context("invalid oauth.authorization_url")?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &config.client_id);
        query.append_pair("redirect_uri", &config.callback_url());
        query.append_pair("code_challenge", &challenge.code_challenge);
        query.append_pair("code_challenge_method", &challenge.code_challenge_method);
        query.append_pair("state", state);
        if !config.scopes.is_empty() {
            query.append_pair("scope", &config.scopes.join(" "));
        }
        if let Some(audience) = config.audience.as_deref()
            && !audience.trim().is_empty()
        {
            query.append_pair("audience", audience);
        }
        for (key, value) in &config.extra_auth_params {
            if !key.trim().is_empty() {
                query.append_pair(key, value);
            }
        }
    }
    Ok(url.to_string())
}

async fn exchange_code_for_token(
    config: &McpOAuthConfig,
    code: &str,
    challenge: &PkceChallenge,
) -> Result<McpOAuthToken> {
    let mut form = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("client_id".to_string(), config.client_id.clone()),
        ("code".to_string(), code.to_string()),
        ("redirect_uri".to_string(), config.callback_url()),
        (
            "code_verifier".to_string(),
            challenge.code_verifier.to_string(),
        ),
    ];
    if let Some(audience) = config.audience.as_deref()
        && !audience.trim().is_empty()
    {
        form.push(("audience".to_string(), audience.to_string()));
    }
    form.extend(
        config
            .extra_token_params
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    send_token_request(&config.token_url, &form).await
}

async fn refresh_token(config: &McpOAuthConfig, current: &McpOAuthToken) -> Result<McpOAuthToken> {
    let refresh_token = current
        .refresh_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("Stored MCP OAuth token does not include a refresh token"))?;
    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("client_id".to_string(), config.client_id.clone()),
        ("refresh_token".to_string(), refresh_token.to_string()),
    ];
    if let Some(audience) = config.audience.as_deref()
        && !audience.trim().is_empty()
    {
        form.push(("audience".to_string(), audience.to_string()));
    }
    form.extend(
        config
            .extra_token_params
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );

    let refreshed = send_token_request(&config.token_url, &form).await?;
    Ok(McpOAuthToken {
        refresh_token: refreshed
            .refresh_token
            .or_else(|| current.refresh_token.clone()),
        ..refreshed
    })
}

async fn send_token_request(token_url: &str, form: &[(String, String)]) -> Result<McpOAuthToken> {
    let response = Client::new()
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(form)
        .send()
        .await
        .with_context(|| format!("failed to send MCP OAuth request to {token_url}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read MCP OAuth response body")?;

    if !status.is_success() {
        bail!("MCP OAuth request failed (HTTP {}): {}", status, body);
    }

    let payload: TokenResponse =
        serde_json::from_str(&body).context("failed to parse MCP OAuth token response")?;
    let now = now_secs();
    Ok(McpOAuthToken {
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        token_type: payload.token_type,
        scope: payload.scope,
        obtained_at: now,
        expires_at: payload.expires_in.map(|secs| now.saturating_add(secs)),
    })
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

fn generate_state() -> Result<String> {
    let mut state_bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut state_bytes)
        .map_err(|_| anyhow!("failed to generate MCP OAuth state"))?;
    Ok(URL_SAFE_NO_PAD.encode(state_bytes))
}

fn save_token(
    provider_name: &str,
    token: &McpOAuthToken,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<()> {
    let serialized = serde_json::to_string(token).context("failed to serialize MCP OAuth token")?;
    token_storage(provider_name).store_with_mode(&serialized, storage_mode)
}

fn load_token(
    provider_name: &str,
    storage_mode: AuthCredentialsStoreMode,
) -> Result<Option<McpOAuthToken>> {
    let Some(serialized) = token_storage(provider_name).load_with_mode(storage_mode)? else {
        return Ok(None);
    };
    serde_json::from_str(&serialized)
        .context("failed to parse stored MCP OAuth token")
        .map(Some)
}

fn clear_token(provider_name: &str, storage_mode: AuthCredentialsStoreMode) -> Result<()> {
    token_storage(provider_name).clear_with_mode(storage_mode)
}

fn token_storage(provider_name: &str) -> CredentialStorage {
    let normalized_provider = provider_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    CredentialStorage::new("vtcode", format!("mcp_oauth_{normalized_provider}"))
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
    use std::path::PathBuf;

    struct TestAuthDirGuard {
        previous: Option<PathBuf>,
        temp_dir: Option<TempDir>,
    }

    impl TestAuthDirGuard {
        fn new() -> Self {
            let temp_dir = TempDir::new().expect("temp dir");
            let previous = crate::storage_paths::auth_storage_dir_override_for_tests()
                .expect("read previous auth dir override");
            crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(
                temp_dir.path().to_path_buf(),
            ))
            .expect("set auth dir override");
            Self {
                previous,
                temp_dir: Some(temp_dir),
            }
        }
    }

    impl Drop for TestAuthDirGuard {
        fn drop(&mut self) {
            crate::storage_paths::set_auth_storage_dir_override_for_tests(self.previous.clone())
                .expect("restore auth dir override");
            if let Some(temp_dir) = self.temp_dir.take() {
                let _ = temp_dir.close();
            }
        }
    }

    fn sample_config() -> McpOAuthConfig {
        McpOAuthConfig {
            authorization_url: "https://example.com/oauth/authorize".to_string(),
            token_url: "https://example.com/oauth/token".to_string(),
            client_id: "client-123".to_string(),
            scopes: vec!["mcp:read".to_string(), "mcp:write".to_string()],
            audience: Some("mcp-api".to_string()),
            callback_port: 8123,
            flow_timeout_secs: 120,
            credentials_store_mode: AuthCredentialsStoreMode::File,
            extra_auth_params: BTreeMap::from([("prompt".to_string(), "consent".to_string())]),
            extra_token_params: BTreeMap::new(),
        }
    }

    #[test]
    fn prepare_login_builds_expected_auth_url() {
        let service = McpOAuthService::new();
        let prepared = service
            .prepare_login("demo", &sample_config())
            .expect("prepare login");

        assert!(prepared.auth_url.contains("response_type=code"));
        assert!(prepared.auth_url.contains("client_id=client-123"));
        assert!(prepared.auth_url.contains("scope=mcp%3Aread+mcp%3Awrite"));
        assert!(prepared.auth_url.contains("audience=mcp-api"));
        assert!(prepared.auth_url.contains("prompt=consent"));
        assert!(prepared.auth_url.contains("code_challenge="));
        assert!(prepared.auth_url.contains("state="));
        assert_eq!(prepared.callback_port, 8123);
        assert_eq!(prepared.timeout_secs, 120);
    }

    #[test]
    #[serial]
    fn status_reflects_stored_token() {
        let _guard = TestAuthDirGuard::new();
        let service = McpOAuthService::new();
        let storage_mode = AuthCredentialsStoreMode::File;
        assert!(matches!(
            service.status("demo", storage_mode).expect("status"),
            McpOAuthStatus::NotAuthenticated
        ));

        save_token(
            "demo",
            &McpOAuthToken {
                access_token: "access".to_string(),
                refresh_token: Some("refresh".to_string()),
                token_type: Some("Bearer".to_string()),
                scope: Some("mcp:read".to_string()),
                obtained_at: now_secs(),
                expires_at: Some(now_secs() + 3600),
            },
            storage_mode,
        )
        .expect("save token");

        let status = service.status("demo", storage_mode).expect("status");
        assert!(matches!(
            status,
            McpOAuthStatus::Authenticated {
                expires_in: Some(_),
                ..
            }
        ));
    }

    #[test]
    #[serial]
    fn logout_clears_stored_token() {
        let _guard = TestAuthDirGuard::new();
        let service = McpOAuthService::new();
        let storage_mode = AuthCredentialsStoreMode::File;
        save_token(
            "demo",
            &McpOAuthToken {
                access_token: "access".to_string(),
                refresh_token: None,
                token_type: Some("Bearer".to_string()),
                scope: None,
                obtained_at: now_secs(),
                expires_at: None,
            },
            storage_mode,
        )
        .expect("save token");

        service.logout("demo", storage_mode).expect("logout");
        assert!(load_token("demo", storage_mode).expect("load").is_none());
    }
}
