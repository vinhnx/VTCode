//! MCP provider transport configuration and deserialization.
//!
//! Extracted from the `mcp` module so the transport wire-decoding lives behind
//! a strict interface: [`McpProviderConfig`] is the only public entry point and
//! [`McpProviderConfigWire`] is private to this module. Callers never see the
//! flat wire shape — they construct or deserialize [`McpProviderConfig`] and
//! pattern-match on [`McpTransportConfig`].
//!
//! The manual [`Deserialize`] impl avoids the `#[serde(flatten)]` +
//! `#[serde(untagged)]` map-buffering overhead: every transport field is
//! decoded in a single pass and the transport enum is constructed afterwards,
//! preserving the untagged Stdio-then-Http precedence. See the regression tests
//! at the bottom of this file for the exact dispatch contract.
//!
//! # Validation semantics (intentional guardrail)
//!
//! Because every recognized field is decoded against its declared type in that
//! single pass, a malformed *known* field is rejected even when it belongs to
//! the transport variant that was not selected. The previous `#[serde(untagged)]`
//! path trial-deserialized Stdio first and silently ignored irrelevant HTTP
//! fields (and vice-versa), so a stray wrong-typed `endpoint` on a valid Stdio
//! provider used to parse. Under the flat wire it now errors. This stricter
//! behavior is an intentional interface guardrail — surfacing malformed known
//! configuration early — and is not a behavior-preserving no-op relative to the
//! derived path. Forward compatibility for *unknown* fields is unaffected: the
//! wire ignores any field it does not declare.

use crate::env_helpers::default_enabled;
use hashbrown::HashMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use vtcode_auth::McpOAuthConfig;

/// Transport configuration for MCP providers
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::large_enum_variant,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum McpTransportConfig {
    /// Standard I/O transport (stdio)
    Stdio(McpStdioServerConfig),
    /// HTTP transport
    Http(McpHttpServerConfig),
}

/// Configuration for stdio-based MCP servers
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct McpStdioServerConfig {
    /// Command to execute
    pub command: String,

    /// Command arguments
    pub args: Vec<String>,

    /// Working directory for the command
    #[serde(default)]
    pub working_directory: Option<String>,
}

/// Pinned MCP protocol versions (single source of truth for version strings).
///
/// The typed rmcp counterparts live in `vtcode-mcp` (`stable_protocol_version`,
/// `legacy_fallback_protocol_version`); keep both sides aligned when the spec
/// publishes a new stable revision.
pub const MCP_STABLE_PROTOCOL_VERSION: &str = "2025-11-25";
pub const MCP_LEGACY_PROTOCOL_VERSION: &str = "2024-11-05";

/// Handshake strategy for HTTP-based MCP servers.
///
/// `Legacy` performs the `initialize` / `notifications/initialized` handshake
/// directly (rmcp `ClientLifecycleMode::Initialize`). `Auto` first probes
/// `server/discover` and falls back to the legacy handshake when the peer
/// reports a legacy server or stays silent past the discover timeout (rmcp
/// `ClientLifecycleMode::Auto`). Legacy is the default: it avoids the
/// discover-timeout penalty against legacy-only servers.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpHttpHandshakeMode {
    #[default]
    Legacy,
    Auto,
}

/// Configuration for HTTP-based MCP servers
///
/// Note: HTTP transport is partially implemented. Basic connectivity testing is supported,
/// but full streamable HTTP MCP server support requires additional implementation
/// using Server-Sent Events (SSE) or WebSocket connections.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpHttpServerConfig {
    /// Server endpoint URL
    pub endpoint: String,

    /// API key environment variable name
    #[serde(default)]
    pub api_key_env: Option<String>,

    /// Optional OAuth configuration for providers that issue bearer tokens dynamically.
    #[serde(default)]
    pub oauth: Option<McpOAuthConfig>,

    /// Protocol version
    #[serde(default = "default_mcp_protocol_version")]
    pub protocol_version: String,

    /// Handshake strategy (`legacy` direct initialize, or `auto` discover
    /// with legacy fallback). Defaults to `legacy`.
    #[serde(default)]
    pub handshake: McpHttpHandshakeMode,

    /// Headers to include in requests
    #[serde(default, alias = "headers")]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeMap<String, String>"))]
    pub http_headers: HashMap<String, String>,

    /// Headers whose values are sourced from environment variables
    /// (`{ header-name = "ENV_VAR" }`). Empty values are ignored.
    #[serde(default)]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeMap<String, String>"))]
    pub env_http_headers: HashMap<String, String>,
}

impl Default for McpHttpServerConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            api_key_env: None,
            oauth: None,
            protocol_version: default_mcp_protocol_version(),
            handshake: McpHttpHandshakeMode::default(),
            http_headers: HashMap::new(),
            env_http_headers: HashMap::new(),
        }
    }
}

/// Configuration for a single MCP provider
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize)]
pub struct McpProviderConfig {
    /// Provider name (used for identification)
    pub name: String,

    /// Transport configuration
    #[serde(flatten)]
    pub transport: McpTransportConfig,

    /// Provider-specific environment variables
    #[serde(default)]
    #[cfg_attr(feature = "schema", schemars(with = "BTreeMap<String, String>"))]
    pub env: HashMap<String, String>,

    /// Whether this provider is enabled
    #[serde(default = "default_provider_enabled")]
    pub enabled: bool,

    /// Maximum number of concurrent requests to this provider
    #[serde(default = "default_provider_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Startup timeout in milliseconds for this provider
    #[serde(default)]
    pub startup_timeout_ms: Option<u64>,
}

impl<'de> Deserialize<'de> for McpProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize every provider + transport field in a single pass.
        // `#[serde(flatten)]` + `#[serde(untagged)]` on `transport` would force
        // Serde to buffer the whole provider object into a `Map<String, Value>`
        // and then trial-deserialize each transport variant. A provider has only
        // two mutually-exclusive transport forms — stdio (`command` + `args`) or
        // HTTP (`endpoint`) — so the fields are decoded directly and the
        // transport enum is constructed afterwards, matching the untagged
        // Stdio-then-Http precedence.
        //
        // Note: because every declared field is type-checked in this pass, a
        // malformed known field is rejected even on the non-selected variant
        // (stricter than the old untagged trial-deserialize, which ignored
        // irrelevant fields). See the module-level "Validation semantics" note.
        let wire = McpProviderConfigWire::deserialize(deserializer)?;

        let transport = if let (Some(command), Some(args)) = (wire.command, wire.args) {
            McpTransportConfig::Stdio(McpStdioServerConfig {
                command,
                args,
                working_directory: wire.working_directory,
            })
        } else if let Some(endpoint) = wire.endpoint {
            McpTransportConfig::Http(McpHttpServerConfig {
                endpoint,
                api_key_env: wire.api_key_env,
                oauth: wire.oauth,
                protocol_version: wire.protocol_version,
                handshake: wire.handshake,
                http_headers: wire.http_headers,
                env_http_headers: wire.env_http_headers,
            })
        } else {
            return Err(serde::de::Error::custom(
                "MCP provider must specify either a stdio `command` (with `args`) or an HTTP `endpoint`",
            ));
        };

        Ok(McpProviderConfig {
            name: wire.name,
            transport,
            env: wire.env,
            enabled: wire.enabled,
            max_concurrent_requests: wire.max_concurrent_requests,
            startup_timeout_ms: wire.startup_timeout_ms,
        })
    }
}

/// Flat wire shape for [`McpProviderConfig`] (see [`McpProviderConfig::deserialize`]).
///
/// Every transport field is declared directly so no intermediate map is
/// buffered. Fields belonging to the unused transport variant simply stay at
/// their `Option`/default values and are ignored when constructing the
/// [`McpTransportConfig`].
#[derive(Deserialize)]
struct McpProviderConfigWire {
    name: String,
    // stdio transport
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    working_directory: Option<String>,
    // http transport
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    api_key_env: Option<String>,
    #[serde(default)]
    oauth: Option<McpOAuthConfig>,
    #[serde(default = "default_mcp_protocol_version")]
    protocol_version: String,
    #[serde(default)]
    handshake: McpHttpHandshakeMode,
    #[serde(default, alias = "headers")]
    http_headers: HashMap<String, String>,
    #[serde(default)]
    env_http_headers: HashMap<String, String>,
    // provider-level
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default = "default_provider_enabled")]
    enabled: bool,
    #[serde(default = "default_provider_max_concurrent")]
    max_concurrent_requests: usize,
    #[serde(default)]
    startup_timeout_ms: Option<u64>,
}

impl Default for McpProviderConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig::default()),
            env: HashMap::new(),
            enabled: default_provider_enabled(),
            max_concurrent_requests: default_provider_max_concurrent(),
            startup_timeout_ms: None,
        }
    }
}

fn default_provider_enabled() -> bool {
    default_enabled()
}

fn default_provider_max_concurrent() -> usize {
    3
}

fn default_mcp_protocol_version() -> String {
    // Default to the current stable spec revision, not the draft. Draft
    // revisions are in-progress and not ready for consumption
    // (https://modelcontextprotocol.io/docs/draft/learn/versioning.md);
    // explicit opt-in to a draft remains possible via config and is still
    // clamped to the last stable version on the legacy handshake path.
    MCP_STABLE_PROTOCOL_VERSION.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_provider_config_http_transport_from_toml() {
        // HTTP provider, discriminated by `endpoint`. Locks in the flat-wire
        // deserialize path (no `#[serde(flatten)]` + `#[serde(untagged)]`
        // buffering) for the HTTP transport branch.
        let toml_str = r#"
name = "deepwiki"
enabled = true
endpoint = "https://mcp.deepwiki.com/mcp"
protocol_version = "2024-11-05"
max_concurrent_requests = 3

[http_headers]
Authorization = "Bearer token"
"#;
        let provider: McpProviderConfig = toml::from_str(toml_str).expect("http provider must parse");

        assert_eq!(provider.name, "deepwiki");
        assert!(provider.enabled);
        assert_eq!(provider.max_concurrent_requests, 3);
        match provider.transport {
            McpTransportConfig::Http(http) => {
                assert_eq!(http.endpoint, "https://mcp.deepwiki.com/mcp");
                assert_eq!(http.protocol_version, "2024-11-05");
                assert_eq!(http.http_headers.get("Authorization"), Some(&"Bearer token".to_string()));
            }
            McpTransportConfig::Stdio(_) => panic!("expected HTTP transport"),
        }
    }

    #[test]
    fn test_mcp_provider_config_http_minimal_fields_from_toml() {
        // Minimal HTTP provider (only `name`, `enabled`, `endpoint`) — the
        // shape repository `vtcode.toml` provider entries use. Locks in the
        // serde defaults for every omitted field so config loading cannot
        // silently start requiring them. Oracles are hardcoded, not the
        // `default_*` helpers, so a helper regression cannot mask itself.
        let toml_str = r#"
name = "cve-now"
enabled = true
endpoint = "https://mcp.cve.now/mcp"
"#;
        let provider: McpProviderConfig = toml::from_str(toml_str).expect("minimal http provider must parse");

        assert_eq!(provider.name, "cve-now");
        assert!(provider.enabled);
        match provider.transport {
            McpTransportConfig::Http(http) => {
                assert_eq!(http.endpoint, "https://mcp.cve.now/mcp");
                assert_eq!(http.protocol_version, "2025-11-25");
                assert_eq!(http.handshake, McpHttpHandshakeMode::Legacy);
                assert!(http.oauth.is_none());
                assert!(http.http_headers.is_empty());
                assert!(http.env_http_headers.is_empty());
            }
            McpTransportConfig::Stdio(_) => panic!("expected HTTP transport"),
        }
    }

    #[test]
    fn test_mcp_provider_config_stdio_transport_from_toml() {
        let toml_str = r#"
name = "time"
command = "uvx"
args = ["mcp-server-time"]
working_directory = "/tmp"
"#;
        let provider: McpProviderConfig = toml::from_str(toml_str).expect("stdio provider must parse");
        match provider.transport {
            McpTransportConfig::Stdio(stdio) => {
                assert_eq!(stdio.command, "uvx");
                assert_eq!(stdio.args, vec!["mcp-server-time"]);
                assert_eq!(stdio.working_directory.as_deref(), Some("/tmp"));
            }
            McpTransportConfig::Http(_) => panic!("expected stdio transport"),
        }
    }

    #[test]
    fn test_mcp_provider_config_stdio_wins_when_both_transports_present() {
        // Replicates the untagged Stdio-then-Http precedence: `command` + `args`
        // select stdio even when `endpoint` is also set.
        let toml_str = r#"
name = "mixed"
command = "uvx"
args = ["mcp-server-time"]
endpoint = "https://example.com/mcp"
"#;
        let provider: McpProviderConfig = toml::from_str(toml_str).expect("mixed provider must parse");
        assert!(matches!(provider.transport, McpTransportConfig::Stdio(_)));
    }

    #[test]
    fn test_mcp_provider_config_http_fallback_when_command_lacks_args() {
        // `command` without `args` cannot form a stdio transport, so the
        // presence of `endpoint` selects HTTP (matches untagged fallback).
        let toml_str = r#"
name = "fallback"
command = "uvx"
endpoint = "https://example.com/mcp"
"#;
        let provider: McpProviderConfig = toml::from_str(toml_str).expect("fallback provider must parse");
        assert!(matches!(provider.transport, McpTransportConfig::Http(_)));
    }

    #[test]
    fn test_mcp_provider_config_rejects_missing_transport() {
        let toml_str = "name = \"bare\"";
        let result: Result<McpProviderConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "provider without command/endpoint must error");
    }

    #[test]
    fn test_mcp_provider_config_rejects_malformed_known_field_on_non_selected_transport() {
        // Intentional strict guardrail: a wrong-typed *known* field is rejected
        // even when it belongs to the transport variant that was not selected.
        // The old `#[serde(untagged)]` path would select Stdio (valid
        // `command` + `args`) and silently ignore the malformed `endpoint`; the
        // flat wire type-checks every declared field in one pass, so this now
        // errors. See the module-level "Validation semantics" note.
        let toml_str = r#"
name = "strict"
command = "uvx"
args = ["mcp-server-time"]
endpoint = 42
"#;
        let result: Result<McpProviderConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "malformed known field on non-selected transport must error under the flat wire");
    }

    #[test]
    fn test_mcp_http_handshake_defaults_to_legacy() {
        let config = McpHttpServerConfig::default();
        assert_eq!(config.handshake, McpHttpHandshakeMode::Legacy);

        let toml_str = r#"
name = "plain"
endpoint = "https://example.com/mcp"
"#;
        let provider: McpProviderConfig = toml::from_str(toml_str).expect("http provider must parse");
        match provider.transport {
            McpTransportConfig::Http(http) => assert_eq!(http.handshake, McpHttpHandshakeMode::Legacy),
            McpTransportConfig::Stdio(_) => panic!("expected HTTP transport"),
        }
    }

    #[test]
    fn test_mcp_http_handshake_parses_auto() {
        let toml_str = r#"
name = "modern"
endpoint = "https://example.com/mcp"
handshake = "auto"
"#;
        let provider: McpProviderConfig = toml::from_str(toml_str).expect("http provider must parse");
        match provider.transport {
            McpTransportConfig::Http(http) => assert_eq!(http.handshake, McpHttpHandshakeMode::Auto),
            McpTransportConfig::Stdio(_) => panic!("expected HTTP transport"),
        }
    }
}
