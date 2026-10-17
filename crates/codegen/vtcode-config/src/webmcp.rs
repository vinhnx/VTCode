use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::PathBuf;
use vtcode_commons::validation::is_valid_origin;

const MIN_FRAME_BYTES: usize = 1024;
const DEFAULT_REMOTE_MCP_PROXY_TOKEN_ENV: &str = "VTCODE_WEBMCP_MCP_PROXY_TOKEN";
const DEFAULT_REMOTE_MCP_MAX_RESULTS: usize = 20;
const DEFAULT_REMOTE_MCP_MAX_SCAN_FILES: usize = 256;
const DEFAULT_REMOTE_MCP_MAX_SCAN_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_REMOTE_MCP_SESSION_TTL_SECS: u64 = 300;

/// Configuration for the opt-in VT Code WebMCP bridge.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WebmcpConfig {
    /// Opt-in marker for WebMCP integrations; listener startup still requires an explicit CLI or TUI command.
    #[serde(default)]
    pub enabled: bool,
    /// Literal loopback bind host.
    #[serde(default = "default_host")]
    pub host: String,
    /// Bind port. Zero asks the OS for an available port.
    #[serde(default)]
    pub port: u16,
    /// Exact browser origins allowed to pair.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Explicit roots available to headless mode; the current bridge serves one root per process.
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
    /// One-time pairing lifetime and authenticated-session inactivity lease.
    #[serde(default = "default_pairing_ttl_secs")]
    pub pairing_ttl_secs: u64,
    /// Maximum JSON WebSocket frame size.
    #[serde(default = "default_max_frame_bytes")]
    pub max_frame_bytes: usize,
    /// Maximum concurrent bridge operations.
    #[serde(default = "default_max_in_flight_requests")]
    pub max_in_flight_requests: usize,
    /// Explicit opt-in OpenAI-compatible remote MCP transport.
    #[serde(default)]
    pub remote_mcp: RemoteMcpConfig,
}

/// Configuration for the read-only OpenAI-compatible MCP transport.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct RemoteMcpConfig {
    /// Enable the remote MCP endpoints in `webmcp serve`.
    pub enabled: bool,
    /// Canonical externally reachable HTTPS URL, normally ending in `/sse/`.
    pub public_url: Option<String>,
    /// External OAuth authorization server URL used by the proxy/identity provider.
    #[serde(alias = "authorization_server_url")]
    pub authorization_server: Option<String>,
    /// Environment variable containing the bearer token injected by the proxy.
    pub proxy_token_env: String,
    /// Optional HTTPS/HTTP prefix used to build citation URLs for file IDs.
    pub citation_url_prefix: Option<String>,
    /// Separate allowlist for supplied MCP `Origin` headers. Missing Origin is accepted.
    pub allowed_origins: Vec<String>,
    /// Maximum results returned by `search`.
    pub max_results: usize,
    /// Maximum visible files inspected by `search`.
    pub max_scan_files: usize,
    /// Maximum UTF-8 content bytes inspected by `search`.
    pub max_scan_bytes: usize,
    /// In-memory legacy SSE session inactivity lifetime.
    pub session_ttl_secs: u64,
}

impl Default for RemoteMcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            public_url: None,
            authorization_server: None,
            proxy_token_env: default_remote_mcp_proxy_token_env(),
            citation_url_prefix: None,
            allowed_origins: Vec::new(),
            max_results: default_remote_mcp_max_results(),
            max_scan_files: default_remote_mcp_max_scan_files(),
            max_scan_bytes: default_remote_mcp_max_scan_bytes(),
            session_ttl_secs: default_remote_mcp_session_ttl_secs(),
        }
    }
}

impl RemoteMcpConfig {
    /// Validate remote MCP URLs, limits, and the proxy-token environment name.
    pub fn validate(&self) -> Result<()> {
        if self.proxy_token_env.trim().is_empty() || !is_valid_env_name(&self.proxy_token_env) {
            bail!("webmcp.remote_mcp.proxy_token_env must be a valid environment variable name");
        }
        if let Some(public_url) = self.public_url.as_deref()
            && !is_valid_https_url(public_url)
        {
            bail!("webmcp.remote_mcp.public_url must be an absolute HTTPS URL without credentials, query, or fragment");
        }
        if let Some(authorization_server) = self.authorization_server.as_deref()
            && !is_valid_https_url(authorization_server)
        {
            bail!(
                "webmcp.remote_mcp.authorization_server must be an absolute HTTPS URL without credentials, query, or fragment"
            );
        }
        if self.enabled {
            if self.public_url.is_none() {
                bail!("webmcp.remote_mcp.public_url is required when remote MCP is enabled");
            }
            if self.authorization_server.is_none() {
                bail!("webmcp.remote_mcp.authorization_server is required when remote MCP is enabled");
            }
        }
        if let Some(citation_url_prefix) = self.citation_url_prefix.as_deref()
            && !is_valid_citation_url_prefix(citation_url_prefix)
        {
            bail!(
                "webmcp.remote_mcp.citation_url_prefix must be an absolute HTTP(S) URL without credentials, query, or fragment"
            );
        }
        if self.allowed_origins.iter().any(|origin| !is_valid_origin(origin)) {
            bail!("webmcp.remote_mcp.allowed_origins must contain explicit origins such as https://client.example");
        }
        if self.max_results == 0 || self.max_results > 100 {
            bail!("webmcp.remote_mcp.max_results must be between 1 and 100");
        }
        if self.max_scan_files == 0 || self.max_scan_files > 4096 {
            bail!("webmcp.remote_mcp.max_scan_files must be between 1 and 4096");
        }
        if self.max_scan_bytes == 0 || self.max_scan_bytes > 64 * 1024 * 1024 {
            bail!("webmcp.remote_mcp.max_scan_bytes must be between 1 and 67108864");
        }
        if self.session_ttl_secs == 0 || self.session_ttl_secs > 3600 {
            bail!("webmcp.remote_mcp.session_ttl_secs must be between 1 and 3600");
        }
        Ok(())
    }
}

impl Default for WebmcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_host(),
            port: 0,
            allowed_origins: Vec::new(),
            allowed_roots: Vec::new(),
            pairing_ttl_secs: default_pairing_ttl_secs(),
            max_frame_bytes: default_max_frame_bytes(),
            max_in_flight_requests: default_max_in_flight_requests(),
            remote_mcp: RemoteMcpConfig::default(),
        }
    }
}

impl WebmcpConfig {
    /// Validate limits and origin syntax without touching the filesystem.
    pub fn validate(&self) -> Result<()> {
        if self.host.trim().is_empty() {
            bail!("webmcp.host must not be empty");
        }
        let address = self
            .host
            .parse::<IpAddr>()
            .context("webmcp.host must be a literal IP address")?;
        if !address.is_loopback() {
            bail!("webmcp.host must be a loopback address; use a TLS-terminating reverse proxy for remote access");
        }
        if self.pairing_ttl_secs == 0 || self.pairing_ttl_secs > 3600 {
            bail!("webmcp.pairing_ttl_secs must be between 1 and 3600");
        }
        if self.max_frame_bytes < MIN_FRAME_BYTES || self.max_frame_bytes > 16 * 1024 * 1024 {
            bail!("webmcp.max_frame_bytes must be between {MIN_FRAME_BYTES} and 16777216");
        }
        if self.max_in_flight_requests == 0 || self.max_in_flight_requests > 64 {
            bail!("webmcp.max_in_flight_requests must be between 1 and 64");
        }
        if self.allowed_origins.iter().any(|origin| !is_valid_origin(origin)) {
            bail!("webmcp.allowed_origins must contain explicit origins such as https://example.test");
        }
        if self.allowed_roots.iter().any(|root| root.as_os_str().is_empty()) {
            bail!("webmcp.allowed_roots must not contain empty paths");
        }
        self.remote_mcp.validate().context("invalid webmcp.remote_mcp configuration")?;
        Ok(())
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn is_valid_https_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    url == url.trim()
        && !url.chars().any(char::is_whitespace)
        && parsed.scheme() == "https"
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

fn is_valid_citation_url_prefix(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    url == url.trim()
        && !url.chars().any(char::is_whitespace)
        && matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

fn is_valid_env_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

const fn default_pairing_ttl_secs() -> u64 {
    300
}

const fn default_max_frame_bytes() -> usize {
    1_048_576
}

const fn default_max_in_flight_requests() -> usize {
    8
}

fn default_remote_mcp_proxy_token_env() -> String {
    DEFAULT_REMOTE_MCP_PROXY_TOKEN_ENV.to_string()
}

const fn default_remote_mcp_max_results() -> usize {
    DEFAULT_REMOTE_MCP_MAX_RESULTS
}

const fn default_remote_mcp_max_scan_files() -> usize {
    DEFAULT_REMOTE_MCP_MAX_SCAN_FILES
}

const fn default_remote_mcp_max_scan_bytes() -> usize {
    DEFAULT_REMOTE_MCP_MAX_SCAN_BYTES
}

const fn default_remote_mcp_session_ttl_secs() -> u64 {
    DEFAULT_REMOTE_MCP_SESSION_TTL_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_webmcp_disabled_and_loopback_only() {
        let config = WebmcpConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 0);
        assert!(config.allowed_origins.is_empty());
        assert!(!config.remote_mcp.enabled);
        assert_eq!(config.remote_mcp.max_results, 20);
        assert_eq!(config.remote_mcp.max_scan_files, 256);
        assert_eq!(config.remote_mcp.max_scan_bytes, 16 * 1024 * 1024);
        config.validate().expect("defaults should validate");
    }

    #[test]
    fn invalid_limits_and_origins_are_rejected() {
        let config = WebmcpConfig { max_in_flight_requests: 0, ..Default::default() };
        assert!(config.validate().is_err());
        let config = WebmcpConfig {
            allowed_origins: vec!["*".to_string()],
            ..Default::default()
        };
        assert!(config.validate().is_err());
        for origin in [
            "https://*",
            "ftp://example.test",
            "https://example.test/path",
            "https://user@example.test",
        ] {
            let config = WebmcpConfig {
                allowed_origins: vec![origin.to_string()],
                ..Default::default()
            };
            assert!(config.validate().is_err(), "accepted {origin}");
        }
        let config = WebmcpConfig { host: "0.0.0.0".to_string(), ..Default::default() };
        assert!(config.validate().is_err());
    }

    #[test]
    fn remote_mcp_requires_https_metadata_and_valid_bounds() {
        let config = WebmcpConfig {
            remote_mcp: RemoteMcpConfig {
                enabled: true,
                public_url: Some("http://mcp.example.test/sse/".to_string()),
                authorization_server: Some("https://auth.example.test".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = WebmcpConfig {
            remote_mcp: RemoteMcpConfig {
                enabled: true,
                public_url: Some("https://mcp.example.test/sse/".to_string()),
                authorization_server: Some("https://auth.example.test".to_string()),
                allowed_origins: vec!["https://client.example.test".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        config.validate().expect("valid remote MCP config");

        let config = WebmcpConfig {
            remote_mcp: RemoteMcpConfig { max_scan_bytes: 0, ..Default::default() },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
