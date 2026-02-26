//! MCP client management built on top of the Codex MCP building blocks.
//!
//! This module adapts the reference MCP client, server and type
//! definitions from <https://github.com/openai/codex> to integrate them
//! with VT Code's multi-provider configuration model. The original
//! implementation inside this project had grown organically and mixed a
//! large amount of bookkeeping logic with the lower level rmcp client
//! transport. The rewritten version keeps the VT Code specific surface
//! (allow lists, tool indexing, status reporting) but delegates the
//! actual protocol interaction to a lightweight `RmcpClient` adapter
//! that mirrors Codex' `mcp-client` crate. This dramatically reduces
//! the amount of bespoke glue we have to maintain while aligning the
//! behaviour with the upstream MCP implementations.

use crate::config::mcp::McpClientConfig;

pub mod cli;
mod client;
pub mod connection_pool;
pub mod enhanced_config;
pub mod errors;
mod provider;
mod rmcp_client;
pub mod rmcp_transport;
pub mod schema;
pub mod tool_discovery;
pub mod tool_discovery_cache;
pub mod traits;
pub mod types;
pub mod utils;

pub use client::McpClient;

pub use connection_pool::{
    ConnectionPoolStats, McpConnectionPool, McpPoolError, PooledMcpManager, PooledMcpStats,
};
pub use errors::{
    ErrorCode, McpResult, configuration_error, initialization_timeout, provider_not_found,
    provider_unavailable, schema_invalid, tool_invocation_failed, tool_not_found,
};
pub use provider::McpProvider;
pub(crate) use rmcp_client::RmcpClient;
pub use rmcp_transport::{
    HttpTransport, create_http_transport, create_stdio_transport,
    create_stdio_transport_with_stderr,
};
pub use schema::{validate_against_schema, validate_tool_input};
pub use tool_discovery::{DetailLevel, ToolDiscovery, ToolDiscoveryResult};
pub use traits::{McpElicitationHandler, McpToolExecutor};
pub use types::{
    McpClientStatus, McpElicitationRequest, McpElicitationResponse, McpPromptDetail, McpPromptInfo,
    McpResourceData, McpResourceInfo, McpToolInfo,
};
pub use utils::{
    LOCAL_TIMEZONE_ENV_VAR, TIMEZONE_ARGUMENT, TZ_ENV_VAR, build_headers, detect_local_timezone,
    ensure_timezone_argument, schema_requires_field,
};

use anyhow::{Result, anyhow};
pub use rmcp::model::ElicitationAction;
use std::collections::HashMap;

/// MCP protocol version constants
pub const LATEST_PROTOCOL_VERSION: &str = "2024-11-05";
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[LATEST_PROTOCOL_VERSION];

/// Convert any serializable type to rmcp model type via JSON serialization
pub(crate) fn convert_to_rmcp<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    serde_json::from_value(json).map_err(|err| anyhow!(err))
}

fn create_env_for_mcp_server(
    extra_env: Option<HashMap<String, String>>,
) -> HashMap<String, String> {
    DEFAULT_ENV_VARS
        .iter()
        .filter_map(|var| {
            std::env::var(var)
                .ok()
                .map(|value| (var.to_string(), value))
        })
        .chain(extra_env.unwrap_or_default())
        .collect()
}

/// Validate MCP configuration settings
pub fn validate_mcp_config(config: &McpClientConfig) -> Result<()> {
    // Validate server configuration if enabled
    if config.server.enabled {
        // Validate port range
        if config.server.port == 0 {
            return Err(anyhow::anyhow!(
                "Invalid server port: {}",
                config.server.port
            ));
        }

        // Validate bind address
        if config.server.bind_address.is_empty() {
            return Err(anyhow::anyhow!("Server bind address cannot be empty"));
        }

        // Validate security settings if auth is enabled
        if config.security.auth_enabled && config.security.api_key_env.is_none() {
            return Err(anyhow::anyhow!(
                "API key environment variable must be set when auth is enabled"
            ));
        }
    }

    // Validate timeouts
    if let Some(startup_timeout) = config.startup_timeout_seconds
        && startup_timeout > 300
    {
        // Max 5 minutes
        return Err(anyhow::anyhow!("Startup timeout cannot exceed 300 seconds"));
    }

    if let Some(tool_timeout) = config.tool_timeout_seconds
        && tool_timeout > 3600
    {
        // Max 1 hour
        return Err(anyhow::anyhow!("Tool timeout cannot exceed 3600 seconds"));
    }

    // Validate provider configurations
    for provider in &config.providers {
        if provider.name.is_empty() {
            return Err(anyhow::anyhow!("MCP provider name cannot be empty"));
        }

        // Validate max_concurrent_requests
        if provider.max_concurrent_requests == 0 {
            return Err(anyhow::anyhow!(
                "Max concurrent requests must be greater than 0 for provider '{}'",
                provider.name
            ));
        }
    }

    Ok(())
}

#[cfg(unix)]
const DEFAULT_ENV_VARS: &[&str] = &[
    "HOME",
    "LOGNAME",
    "PATH",
    "SHELL",
    "USER",
    "__CF_USER_TEXT_ENCODING",
    "LANG",
    "LC_ALL",
    "TERM",
    "TMPDIR",
    "TZ",
];

#[cfg(windows)]
const DEFAULT_ENV_VARS: &[&str] = &[
    // Core path resolution
    "PATH",
    "PATHEXT",
    // Shell and system roots
    "COMSPEC",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    // User context and profiles
    "USERNAME",
    "USERDOMAIN",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    // Program locations
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "PROGRAMDATA",
    // App data and caches
    "LOCALAPPDATA",
    "APPDATA",
    // Temp locations
    "TEMP",
    "TMP",
    // Common shells/pwsh hints
    "POWERSHELL",
    "PWSH",
];

// Helper functions for file-based tool discovery

/// Sanitize a string for use in a filename
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Format a tool description as Markdown
fn format_tool_markdown(tool: &McpToolInfo) -> String {
    let mut content = String::new();

    content.push_str(&format!("# {}\n\n", tool.name));
    content.push_str(&format!("**Provider**: {}\n\n", tool.provider));
    content.push_str("## Description\n\n");
    content.push_str(&tool.description);
    content.push_str("\n\n");

    content.push_str("## Input Schema\n\n");
    content.push_str("```json\n");
    content.push_str(
        &serde_json::to_string_pretty(&tool.input_schema)
            .unwrap_or_else(|_| tool.input_schema.to_string()),
    );
    content.push_str("\n```\n\n");

    // Extract required fields if present
    if let Some(obj) = tool.input_schema.as_object() {
        if let Some(required) = obj.get("required").and_then(|v| v.as_array())
            && !required.is_empty()
        {
            content.push_str("## Required Parameters\n\n");
            for req in required {
                if let Some(name) = req.as_str() {
                    content.push_str(&format!("- `{}`\n", name));
                }
            }
            content.push('\n');
        }

        // Extract properties descriptions
        if let Some(props) = obj.get("properties").and_then(|v| v.as_object())
            && !props.is_empty()
        {
            content.push_str("## Parameters\n\n");
            for (param_name, param_schema) in props {
                let param_type = param_schema
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("any");
                let param_desc = param_schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                content.push_str(&format!("### `{}`\n\n", param_name));
                content.push_str(&format!("- **Type**: {}\n", param_type));
                if !param_desc.is_empty() {
                    content.push_str(&format!("- **Description**: {}\n", param_desc));
                }
                content.push('\n');
            }
        }
    }

    content.push_str("---\n");
    content.push_str("*Generated automatically for dynamic context discovery.*\n");

    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::mcp::{McpProviderConfig, McpStdioServerConfig, McpTransportConfig};
    use crate::mcp::rmcp_client::{
        build_elicitation_validator, directory_to_file_uri, validate_elicitation_payload,
    };
    use serde_json::{Map, Value, json};

    // Re-export rmcp types for tests
    use rmcp::model::{
        ClientCapabilities, Implementation, InitializeRequestParams, RootsCapabilities,
    };

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests provide well-formed UTF-8 values and restore the
            // original value (if any) before dropping the guard, matching the
            // documented requirements for manipulating the process
            // environment.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(ref original) = self.original {
                // SAFETY: Restores the previous UTF-8 environment value that
                // existed when the guard was created.
                unsafe {
                    std::env::set_var(self.key, original);
                }
            } else {
                // SAFETY: Removing the variable is safe because the guard is
                // the only code path mutating it during the test's lifetime.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn schema_detection_handles_required_entries() {
        let schema = json!({
            "type": "object",
            "required": [TIMEZONE_ARGUMENT],
            "properties": {
                TIMEZONE_ARGUMENT: { "type": "string" }
            }
        });

        assert!(schema_requires_field(&schema, TIMEZONE_ARGUMENT));
        assert!(!schema_requires_field(&schema, "location"));
    }

    #[test]
    fn ensure_timezone_injects_from_override_env() {
        let _guard = EnvGuard::set(LOCAL_TIMEZONE_ENV_VAR, "Etc/UTC");
        let mut arguments = Map::new();

        ensure_timezone_argument(&mut arguments, true).unwrap();

        assert_eq!(
            arguments.get(TIMEZONE_ARGUMENT).and_then(Value::as_str),
            Some("Etc/UTC")
        );
    }

    #[test]
    fn ensure_timezone_does_not_override_existing_value() {
        let mut arguments = Map::new();
        arguments.insert(
            TIMEZONE_ARGUMENT.to_string(),
            Value::String("America/New_York".to_owned()),
        );

        ensure_timezone_argument(&mut arguments, true).unwrap();

        assert_eq!(
            arguments.get(TIMEZONE_ARGUMENT).and_then(Value::as_str),
            Some("America/New_York")
        );
    }

    #[tokio::test]
    async fn convert_to_rmcp_round_trip() {
        let params = InitializeRequestParams {
            capabilities: ClientCapabilities {
                roots: Some(RootsCapabilities {
                    list_changed: Some(true),
                }),
                ..Default::default()
            },
            client_info: Implementation {
                name: "vtcode".to_owned(),
                version: "1.0".to_owned(),
                title: None,
                icons: None,
                website_url: None,
            },
            protocol_version: rmcp::model::ProtocolVersion::V_2024_11_05,
            meta: None,
        };

        let converted: rmcp::model::InitializeRequestParams =
            convert_to_rmcp(params.clone()).unwrap();
        // Verify the conversion succeeded by checking the name
        assert_eq!(converted.client_info.name, "vtcode");
        assert_eq!(converted.client_info.version, "1.0");
    }

    #[test]
    fn validate_elicitation_payload_rejects_invalid_content() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });
        let validator =
            build_elicitation_validator("test", &schema).expect("schema should compile");

        let result = validate_elicitation_payload(
            "test",
            Some(&validator),
            &ElicitationAction::Accept,
            Some(&json!({ "name": 42 })),
        );

        assert!(result.is_err());
    }

    #[test]
    fn validate_elicitation_payload_accepts_valid_content() {
        let schema = json!({
            "type": "object",
            "properties": {
                "email": { "type": "string", "format": "email" }
            },
            "required": ["email"]
        });
        let validator =
            build_elicitation_validator("test", &schema).expect("schema should compile");

        let result = validate_elicitation_payload(
            "test",
            Some(&validator),
            &ElicitationAction::Accept,
            Some(&json!({ "email": "user@example.com" })),
        );

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn provider_max_concurrency_defaults_to_one() {
        let config = McpProviderConfig {
            name: "test".into(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                command: "cat".into(),
                args: vec![],
                working_directory: None,
            }),
            env: HashMap::new(),
            enabled: true,
            max_concurrent_requests: 0,
            startup_timeout_ms: None,
        };

        let provider = McpProvider::connect(config, None).await.unwrap();
        assert_eq!(provider.semaphore.available_permits(), 1);
    }

    #[test]
    fn directory_to_file_uri_generates_file_scheme() {
        let temp_dir = std::env::temp_dir();
        let uri = directory_to_file_uri(temp_dir.as_path())
            .expect("should create file uri for temp directory");
        assert!(uri.starts_with("file://"));
    }
}
