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
    audience: Option<String>,
    /// Local callback server port.
    pub callback_port: u16,
    /// Browser-flow timeout in seconds.
    flow_timeout_secs: u64,
    /// Credential storage backend for this provider's token.
    #[serde(default)]
    pub credentials_store_mode: AuthCredentialsStoreMode,
    /// Extra query parameters appended to the authorization URL.
    #[serde(default)]
    extra_auth_params: BTreeMap<String, String>,
    /// Extra form fields appended to token exchanges and refreshes.
    #[serde(default)]
    extra_token_params: BTreeMap<String, String>,
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

/// Allowlist-based endpoint check: only `http(s)` URLs with a host.
///
/// Configured OAuth endpoints and the protected resource all pass through
/// here so a `javascript:`/`data:`/shell-metachar URL fails at prepare time,
/// never at `webbrowser::open` or `reqwest` time.
fn validate_endpoint_url(what: &str, provider_name: &str, raw: &str) -> Result<Url> {
    let parsed = Url::parse(raw).with_context(|| format!("MCP provider '{provider_name}' has an invalid {what}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        bail!("MCP provider '{provider_name}' {what} must be an http(s) URL with a host");
    }
    Ok(parsed)
}

impl McpOAuthConfig {
    fn validate(&self, provider_name: &str) -> Result<()> {
        if self.authorization_url.trim().is_empty() {
            bail!("MCP provider '{provider_name}' is missing oauth.authorization_url");
        }
        if self.token_url.trim().is_empty() {
            bail!("MCP provider '{provider_name}' is missing oauth.token_url");
        }
        if self.client_id.trim().is_empty() {
            bail!("MCP provider '{provider_name}' is missing oauth.client_id");
        }
        drop(validate_endpoint_url("oauth.authorization_url", provider_name, &self.authorization_url)?);
        drop(validate_endpoint_url("oauth.token_url", provider_name, &self.token_url)?);
        Ok(())
    }

    fn callback_url(&self) -> String {
        format!("http://localhost:{}/auth/callback", self.callback_port)
    }
}

/// Stored OAuth token for an MCP HTTP provider.
#[derive(Clone, Serialize, Deserialize)]
pub struct McpOAuthToken {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    obtained_at: u64,
    expires_at: Option<u64>,
    #[serde(default)]
    binding: Option<McpOAuthBinding>,
}

impl std::fmt::Debug for McpOAuthToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOAuthToken")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "[REDACTED]"))
            .field("token_type", &self.token_type)
            .field("scope", &self.scope)
            .field("obtained_at", &self.obtained_at)
            .field("expires_at", &self.expires_at)
            .field("binding", &self.binding.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct McpOAuthBinding {
    provider: String,
    authorization_url: String,
    token_url: String,
    client_id: String,
    audience: Option<String>,
    scopes: Vec<String>,
    extra_auth_params: BTreeMap<String, String>,
    extra_token_params: BTreeMap<String, String>,
    resource: String,
}

impl McpOAuthBinding {
    fn new(provider: &str, config: &McpOAuthConfig, resource: &str) -> Result<Self> {
        config.validate(provider)?;
        let resource = validate_endpoint_url("protected resource URL", provider, resource)?;
        Ok(Self {
            provider: provider.to_owned(),
            authorization_url: config.authorization_url.clone(),
            token_url: config.token_url.clone(),
            client_id: config.client_id.clone(),
            audience: config.audience.clone(),
            scopes: config.scopes.clone(),
            extra_auth_params: config.extra_auth_params.clone(),
            extra_token_params: config.extra_token_params.clone(),
            resource: resource.to_string(),
        })
    }
}

impl McpOAuthToken {
    fn is_refresh_due(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| now_secs().saturating_add(REFRESH_SKEW_SECS) >= expires_at)
    }
}

/// Status for an MCP provider's stored OAuth token.
#[derive(Debug, Clone)]
pub enum McpOAuthStatus {
    Authenticated { age_seconds: u64, expires_in: Option<u64> },
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
    binding: Option<McpOAuthBinding>,
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
    success: bool,
    error: Option<String>,
}

/// Service for loading, refreshing, and persisting MCP OAuth tokens.
#[derive(Debug, Clone, Default)]
pub struct McpOAuthService;

impl McpOAuthService {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Prepare the authorization URL without a resource binding.
    ///
    /// Such a flow cannot be completed securely; use `prepare_login_for_resource`
    /// when the token will be used by an HTTP MCP provider.
    pub fn prepare_login(&self, provider_name: &str, config: &McpOAuthConfig) -> Result<McpOAuthPreparedLogin> {
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
            binding: None,
        })
    }

    /// Prepare a login bound to the HTTP resource that will receive the token.
    pub fn prepare_login_for_resource(
        &self,
        provider_name: &str,
        config: &McpOAuthConfig,
        resource: &str,
    ) -> Result<McpOAuthPreparedLogin> {
        let mut prepared = self.prepare_login(provider_name, config)?;
        prepared.binding = Some(McpOAuthBinding::new(provider_name, config, resource)?);
        Ok(prepared)
    }

    pub async fn complete_login(
        &self,
        provider_name: &str,
        config: &McpOAuthConfig,
        prepared: &McpOAuthPreparedLogin,
        code: &str,
    ) -> Result<McpOAuthLoginCompletion> {
        config.validate(provider_name)?;
        let binding = prepared
            .binding
            .as_ref()
            .context("MCP OAuth login requires a protected resource")?;
        if *binding != McpOAuthBinding::new(provider_name, config, &binding.resource)? {
            bail!("MCP OAuth configuration changed during login; start login again");
        }
        let mut token = exchange_code_for_token(config, code, &prepared.pkce).await?;
        token.binding = Some(binding.clone());
        save_token(provider_name, &token, config.credentials_store_mode)?;
        Ok(McpOAuthLoginCompletion {
            name: provider_name.to_string(),
            success: true,
            error: None,
        })
    }

    pub fn status(&self, provider_name: &str, storage_mode: AuthCredentialsStoreMode) -> Result<McpOAuthStatus> {
        let Some(token) = load_token(provider_name, storage_mode)? else {
            return Ok(McpOAuthStatus::NotAuthenticated);
        };
        if token.binding.is_none() {
            return Ok(McpOAuthStatus::NotAuthenticated);
        }
        let now = now_secs();
        Ok(McpOAuthStatus::Authenticated {
            age_seconds: now.saturating_sub(token.obtained_at),
            expires_in: token.expires_at.map(|expires_at| expires_at.saturating_sub(now)),
        })
    }

    /// Report authentication only when the stored binding matches the configured resource.
    pub fn status_for_resource(
        &self,
        provider_name: &str,
        config: &McpOAuthConfig,
        resource: &str,
    ) -> Result<McpOAuthStatus> {
        let binding = McpOAuthBinding::new(provider_name, config, resource)?;
        let Some(token) = load_token(provider_name, config.credentials_store_mode)? else {
            return Ok(McpOAuthStatus::NotAuthenticated);
        };
        if token.binding.as_ref() != Some(&binding) {
            return Ok(McpOAuthStatus::NotAuthenticated);
        }
        let now = now_secs();
        Ok(McpOAuthStatus::Authenticated {
            age_seconds: now.saturating_sub(token.obtained_at),
            expires_in: token.expires_at.map(|expires_at| expires_at.saturating_sub(now)),
        })
    }

    pub fn load_token(
        &self,
        provider_name: &str,
        storage_mode: AuthCredentialsStoreMode,
    ) -> Result<Option<McpOAuthToken>> {
        load_token(provider_name, storage_mode)
    }

    /// Unbound resolution is rejected because it cannot identify the receiving resource.
    #[deprecated(
        since = "0.180.6",
        note = "use `resolve_access_token_for_resource` with the receiving resource"
    )]
    pub async fn resolve_access_token(&self, provider_name: &str, config: &McpOAuthConfig) -> Result<Option<String>> {
        let _ = (provider_name, config);
        bail!("MCP OAuth token resolution requires a protected resource")
    }

    /// Resolve only a token bound to the current authorization and resource configuration.
    pub async fn resolve_access_token_for_resource(
        &self,
        provider_name: &str,
        config: &McpOAuthConfig,
        resource: &str,
    ) -> Result<Option<String>> {
        let binding = McpOAuthBinding::new(provider_name, config, resource)?;
        let Some(mut token) = load_token(provider_name, config.credentials_store_mode)? else {
            return Ok(None);
        };

        if token.binding.as_ref() != Some(&binding) {
            return Ok(None);
        }

        if token.is_refresh_due() {
            if token.refresh_token.is_some() {
                token = refresh_token(config, &token).await?;
                token.binding = Some(binding);
                save_token(provider_name, &token, config.credentials_store_mode)?;
            } else {
                bail!(
                    "Stored MCP OAuth token for '{provider_name}' expired and cannot be refreshed. Run `vtcode mcp login {provider_name}` again."
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

#[expect(
    unused_results,
    reason = "URL query builder methods mutate the serializer and return a fluent reference."
)]
fn build_auth_url(config: &McpOAuthConfig, challenge: &PkceChallenge, state: &str) -> Result<String> {
    let mut url = Url::parse(&config.authorization_url).context("invalid oauth.authorization_url")?;
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
        ("code_verifier".to_string(), challenge.code_verifier.to_string()),
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
        refresh_token: refreshed.refresh_token.or_else(|| current.refresh_token.clone()),
        ..refreshed
    })
}

async fn send_token_request(token_url: &str, form: &[(String, String)]) -> Result<McpOAuthToken> {
    let response = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to create MCP OAuth client")?
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(form)
        .send()
        .await
        .map_err(|error| anyhow!("failed to send MCP OAuth request: {}", error.without_url()))?;
    let status = response.status();
    let body = response.text().await.context("failed to read MCP OAuth response body")?;

    if !status.is_success() {
        bail!("MCP OAuth request failed (HTTP {status})");
    }

    let payload: TokenResponse = serde_json::from_str(&body).context("failed to parse MCP OAuth token response")?;
    let now = now_secs();
    Ok(McpOAuthToken {
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        token_type: payload.token_type,
        scope: payload.scope,
        obtained_at: now,
        expires_at: payload.expires_in.map(|secs| now.saturating_add(secs)),
        binding: None,
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

fn save_token(provider_name: &str, token: &McpOAuthToken, storage_mode: AuthCredentialsStoreMode) -> Result<()> {
    let serialized = serde_json::to_string(token).context("failed to serialize MCP OAuth token")?;
    token_storage(provider_name).store_with_mode(&serialized, storage_mode)
}

fn load_token(provider_name: &str, storage_mode: AuthCredentialsStoreMode) -> Result<Option<McpOAuthToken>> {
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
    let encoded_provider = URL_SAFE_NO_PAD.encode(provider_name.as_bytes());
    CredentialStorage::new("vtcode", format!("mcp_oauth:v2:{encoded_provider}"))
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
            let previous =
                crate::storage_paths::auth_storage_dir_override_for_tests().expect("read previous auth dir override");
            crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(temp_dir.path().to_path_buf()))
                .expect("set auth dir override");
            Self { previous, temp_dir: Some(temp_dir) }
        }
    }

    impl Drop for TestAuthDirGuard {
        fn drop(&mut self) {
            crate::storage_paths::set_auth_storage_dir_override_for_tests(self.previous.clone())
                .expect("restore auth dir override");
            if let Some(temp_dir) = self.temp_dir.take() {
                drop(temp_dir.close());
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
    fn validate_rejects_dangerous_endpoint_schemes() {
        let mut config = sample_config();
        for (field, url) in [
            ("authorization_url", "javascript:alert(1)"),
            ("authorization_url", "data:text/html,<script>alert(1)</script>"),
            ("token_url", "file:///etc/passwd"),
            ("token_url", "vbscript:msgbox(1)"),
        ] {
            let mut bad = config.clone();
            if field == "authorization_url" {
                bad.authorization_url = url.to_string();
            } else {
                bad.token_url = url.to_string();
            }
            let error = bad.validate("demo").expect_err("dangerous scheme must fail validation");
            assert!(format!("{error:#}").contains("must be an http(s) URL"), "unexpected error for {url}: {error:#}");
        }
        config.token_url = "http://127.0.0.1:1/token".to_string();
        config.validate("demo").expect("loopback http stays allowed");
    }

    #[test]
    fn prepare_login_for_resource_rejects_dangerous_resource_scheme() {
        let service = McpOAuthService::new();
        let error = service
            .prepare_login_for_resource("demo", &sample_config(), "javascript:alert(1)")
            .expect_err("dangerous resource scheme must fail");
        assert!(format!("{error:#}").contains("must be an http(s) URL"), "unexpected error: {error:#}");
    }

    #[test]
    fn prepare_login_builds_expected_auth_url() {
        let service = McpOAuthService::new();
        let prepared = service.prepare_login("demo", &sample_config()).expect("prepare login");

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

    #[tokio::test]
    async fn login_completion_rejects_changed_configuration_before_exchange() {
        let service = McpOAuthService::new();
        let config = sample_config();
        let prepared = service
            .prepare_login_for_resource("demo", &config, "https://mcp.example/api")
            .expect("prepare");
        let mut changed = config.clone();
        changed.token_url = "http://127.0.0.1:1/unexpected".to_string();
        let error = service
            .complete_login("demo", &changed, &prepared, "code")
            .await
            .expect_err("reject changed config");
        assert!(format!("{error:#}").contains("configuration changed during login"));
        let unbound = service.prepare_login("demo", &config).expect("legacy prepare");
        let error = service
            .complete_login("demo", &config, &unbound, "code")
            .await
            .expect_err("reject unbound login");
        assert!(format!("{error:#}").contains("requires a protected resource"));
    }

    #[tokio::test]
    #[serial]
    async fn bound_login_exchanges_and_resolves_for_the_same_resource() {
        use std::io::{Read, Write};
        let _guard = TestAuthDirGuard::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("timeout");
            let mut buffer = [0_u8; 4096];
            let count = stream.read(&mut buffer).expect("request");
            assert!(String::from_utf8_lossy(&buffer[..count]).starts_with("POST /token HTTP/1.1"));
            let body = r#"{"access_token":"fixture-access","token_type":"Bearer","expires_in":3600}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("response");
        });
        let mut config = sample_config();
        config.token_url = format!("http://{address}/token");
        let service = McpOAuthService::new();
        let resource = "https://mcp.example/resource";
        let prepared = service.prepare_login_for_resource("demo", &config, resource).expect("prepare");
        let completion = service
            .complete_login("demo", &config, &prepared, "fixture-code")
            .await
            .expect("complete");
        assert_eq!(completion.name, "demo");
        assert_eq!(
            service
                .resolve_access_token_for_resource("demo", &config, resource)
                .await
                .expect("resolve")
                .as_deref(),
            Some("fixture-access")
        );
        server.join().expect("server");
    }

    #[tokio::test]
    async fn token_exchange_does_not_follow_redirects_or_echo_error_bodies() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 4096];
            let _count = stream.read(&mut buffer).expect("request");
            let body = "sensitive-reflected-value";
            write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("response");
        });
        let error = send_token_request(&format!("http://{address}/token"), &[("code".into(), "secret".into())])
            .await
            .expect_err("redirect must fail");
        let message = format!("{error:#}");
        assert!(message.contains("307"), "{message}");
        assert!(!message.contains("sensitive-reflected-value"));
        server.join().expect("server");
    }

    #[test]
    #[serial]
    fn status_reflects_stored_token() {
        let _guard = TestAuthDirGuard::new();
        let service = McpOAuthService::new();
        let storage_mode = AuthCredentialsStoreMode::File;
        assert!(matches!(service.status("demo", storage_mode).expect("status"), McpOAuthStatus::NotAuthenticated));

        save_token(
            "demo",
            &McpOAuthToken {
                access_token: "access".to_string(),
                refresh_token: Some("refresh".to_string()),
                token_type: Some("Bearer".to_string()),
                scope: Some("mcp:read".to_string()),
                obtained_at: now_secs(),
                expires_at: Some(now_secs() + 3600),
                binding: Some(
                    McpOAuthBinding::new("demo", &sample_config(), "https://mcp.example/api").expect("binding"),
                ),
            },
            storage_mode,
        )
        .expect("save token");

        let status = service.status("demo", storage_mode).expect("status");
        assert!(matches!(status, McpOAuthStatus::Authenticated { expires_in: Some(_), .. }));
    }

    #[tokio::test]
    #[serial]
    async fn token_resolution_requires_matching_resource_and_oauth_identity() {
        let _guard = TestAuthDirGuard::new();
        let service = McpOAuthService::new();
        let config = sample_config();
        let resource = "https://mcp.example/api";
        let binding = McpOAuthBinding::new("demo", &config, resource).expect("binding");
        let mut token = McpOAuthToken {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            token_type: None,
            scope: None,
            obtained_at: now_secs(),
            expires_at: None,
            binding: Some(binding),
        };
        let debug = format!("{token:?}");
        assert!(!debug.contains("\"access\""));
        assert!(!debug.contains("\"refresh\""));
        save_token("demo", &token, config.credentials_store_mode).expect("store token");
        assert_eq!(
            service
                .resolve_access_token_for_resource("demo", &config, resource)
                .await
                .expect("resolve")
                .as_deref(),
            Some("access")
        );
        // A mismatched expired token must be rejected before any refresh request.
        token.expires_at = Some(1);
        save_token("demo", &token, config.credentials_store_mode).expect("store expired token");
        let mut changed = config.clone();
        changed.token_url = "https://other.example/token".to_string();
        for (candidate, endpoint) in [(&changed, resource), (&config, "https://other.example/api")] {
            assert_eq!(
                service
                    .resolve_access_token_for_resource("demo", candidate, endpoint)
                    .await
                    .expect("resolve"),
                None
            );
            assert!(matches!(
                service.status_for_resource("demo", candidate, endpoint).expect("status"),
                McpOAuthStatus::NotAuthenticated
            ));
        }
        let mut changed_client = config.clone();
        changed_client.client_id = "other-client".to_string();
        assert_eq!(
            service
                .resolve_access_token_for_resource("demo", &changed_client, resource)
                .await
                .expect("resolve"),
            None
        );
        let mut changed_auth = config.clone();
        changed_auth.authorization_url = "https://other.example/authorize".to_string();
        assert_eq!(
            service
                .resolve_access_token_for_resource("demo", &changed_auth, resource)
                .await
                .expect("resolve"),
            None
        );
        assert_eq!(
            service
                .resolve_access_token_for_resource("demo_", &config, resource)
                .await
                .expect("resolve"),
            None
        );
        token.binding = None;
        save_token("demo", &token, config.credentials_store_mode).expect("store unbound token");
        assert_eq!(
            service
                .resolve_access_token_for_resource("demo", &config, resource)
                .await
                .expect("resolve"),
            None
        );
    }

    #[test]
    #[serial]
    fn legacy_and_colliding_provider_names_are_not_reused() {
        let _guard = TestAuthDirGuard::new();
        let legacy = CredentialStorage::new("vtcode", "mcp_oauth_a_b");
        legacy
            .store_with_mode("old-unbound-token", AuthCredentialsStoreMode::File)
            .expect("legacy token");
        assert!(load_token("a/b", AuthCredentialsStoreMode::File).expect("first load").is_none());
        assert!(
            load_token("a_b", AuthCredentialsStoreMode::File)
                .expect("second load")
                .is_none()
        );
        let token = McpOAuthToken {
            access_token: "first".to_string(),
            refresh_token: None,
            token_type: None,
            scope: None,
            obtained_at: now_secs(),
            expires_at: None,
            binding: None,
        };
        save_token("a/b", &token, AuthCredentialsStoreMode::File).expect("first token");
        assert!(
            load_token("a_b", AuthCredentialsStoreMode::File)
                .expect("second load")
                .is_none()
        );
        clear_token("a_b", AuthCredentialsStoreMode::File).expect("clear second");
        assert_eq!(
            load_token("a/b", AuthCredentialsStoreMode::File)
                .expect("first load")
                .expect("first token")
                .access_token,
            "first"
        );
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
                binding: None,
            },
            storage_mode,
        )
        .expect("save token");

        drop(service.logout("demo", storage_mode).expect("logout"));
        assert!(load_token("demo", storage_mode).expect("load").is_none());
    }
}
