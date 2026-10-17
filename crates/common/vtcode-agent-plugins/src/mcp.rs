use std::collections::HashMap;

use crate::errors::PluginError;

const SUPPORTED_SCHEMAS: &[&str] = &["https://agent-plugins.org/schemas/1.0.0/mcp.schema.json"];

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub schema: String,
    pub servers: HashMap<String, ServerConfig>,
    pub unknown_fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ServerConfig {
    Stdio(StdioServerConfig),
    StreamableHttp(HttpServerConfig),
    Sse(SseServerConfig),
}

#[derive(Debug, Clone)]
pub struct StdioServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpServerConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SseServerConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
}

impl McpConfig {
    pub fn parse(content: &str) -> Result<Self, PluginError> {
        let value: serde_json::Value = serde_json::from_str(content).map_err(PluginError::Json)?;
        let map = value
            .as_object()
            .ok_or_else(|| PluginError::InvalidMcp("mcp.json must be a JSON object".into()))?;

        let mut unknown = Vec::new();
        let mut config = McpConfig {
            schema: String::new(),
            servers: HashMap::new(),
            unknown_fields: Vec::new(),
        };

        for (key, val) in map {
            match key.as_str() {
                "$schema" => {
                    let s = val
                        .as_str()
                        .ok_or_else(|| PluginError::InvalidMcp("$schema must be a string".into()))?;
                    config.schema = s.to_string();
                }
                "mcpServers" => {
                    let servers_map = val
                        .as_object()
                        .ok_or_else(|| PluginError::InvalidMcp("mcpServers must be an object".into()))?;
                    for (name, server_val) in servers_map {
                        match parse_server_config(name, server_val) {
                            Ok(server) => {
                                drop(config.servers.insert(name.clone(), server));
                            }
                            // Per the Agent Plugins spec, an invalid individual
                            // server entry is skipped while valid peers continue
                            // loading. Both missing-field and wrong-type errors
                            // are entry-local failures — one bad server must not
                            // disable every MCP server in the plugin.
                            Err(ServerConfigError::MissingField(msg)) => {
                                tracing::warn!(
                                    server = name,
                                    error = msg,
                                    "skipping MCP server entry with missing required fields"
                                );
                            }
                            Err(ServerConfigError::WrongType(msg)) => {
                                tracing::warn!(
                                    server = name,
                                    error = msg,
                                    "skipping MCP server entry with wrong-typed fields"
                                );
                            }
                        }
                    }
                }
                _ => unknown.push(key.clone()),
            }
        }

        if config.schema.is_empty() {
            return Err(PluginError::InvalidMcp("missing required field: $schema".into()));
        }
        if !SUPPORTED_SCHEMAS.contains(&config.schema.as_str()) {
            return Err(PluginError::InvalidMcp(format!(
                "unsupported $schema '{}' (expected {})",
                config.schema,
                SUPPORTED_SCHEMAS.join(" or ")
            )));
        }

        config.unknown_fields = unknown;
        Ok(config)
    }
}

/// Distinguishes missing required fields from wrong-typed fields during
/// MCP server config parsing. Both are entry-local failures: the individual
/// server is skipped while valid peers continue loading (Agent Plugins spec).
/// The distinction improves diagnostic messages so users can tell whether a
/// field was absent or had the wrong JSON type.
enum ServerConfigError {
    MissingField(String),
    WrongType(String),
}

/// Extract a required string field, distinguishing absent from wrong-typed.
fn require_string_field<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    name: &'a str,
    field: &str,
) -> Result<&'a str, ServerConfigError> {
    match obj.get(field) {
        Some(serde_json::Value::String(s)) => Ok(s),
        Some(_) => Err(ServerConfigError::WrongType(format!("server '{name}' field '{field}' must be a string"))),
        None => Err(ServerConfigError::MissingField(format!("server '{name}' missing required field: {field}"))),
    }
}

fn parse_server_config(name: &str, value: &serde_json::Value) -> Result<ServerConfig, ServerConfigError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ServerConfigError::WrongType(format!("server '{}' must be an object", name)))?;

    let type_val = require_string_field(obj, name, "type")?;

    match type_val {
        "stdio" => {
            reject_unexpected_fields(obj, name, &["type", "command", "args", "env", "cwd"])?;
            let command = require_string_field(obj, name, "command")?.to_string();
            if let Err(reason) = crate::expansion::validate_command_token(&command) {
                return Err(ServerConfigError::WrongType(format!("server '{name}' has invalid command: {reason}")));
            }

            let args = obj
                .get("args")
                .map(|v| expect_string_array(v, &format!("server '{name}' args")))
                .transpose()?
                .unwrap_or_default();

            let env = obj
                .get("env")
                .map(|v| expect_string_map(v, &format!("server '{name}' env")))
                .transpose()?
                .unwrap_or_default();
            for key in env.keys() {
                if crate::expansion::is_reserved_env_key(key) {
                    return Err(ServerConfigError::WrongType(format!(
                        "server '{name}' env must not set reserved '{key}'"
                    )));
                }
                if key.contains('\0') || key.contains('\n') || key.contains('\r') {
                    return Err(ServerConfigError::WrongType(format!("server '{name}' has invalid env key")));
                }
            }

            let cwd = obj
                .get("cwd")
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or_else(|| ServerConfigError::WrongType(format!("server '{name}' cwd must be a string")))
                })
                .transpose()?;
            if let Some(ref raw_cwd) = cwd {
                if let Err(reason) = crate::expansion::validate_cwd_form(raw_cwd) {
                    return Err(ServerConfigError::WrongType(format!("server '{name}' has invalid cwd: {reason}")));
                }
            }

            Ok(ServerConfig::Stdio(StdioServerConfig { command, args, env, cwd }))
        }
        "streamable-http" => {
            reject_unexpected_fields(obj, name, &["type", "url", "headers"])?;
            let url = require_string_field(obj, name, "url")?.to_string();
            validate_remote_url(&url)
                .map_err(|reason| ServerConfigError::WrongType(format!("server '{name}' has invalid url: {reason}")))?;

            let headers = obj
                .get("headers")
                .map(|v| expect_string_map(v, &format!("server '{name}' headers")))
                .transpose()?
                .unwrap_or_default();
            validate_headers(&headers, name)?;

            Ok(ServerConfig::StreamableHttp(HttpServerConfig { url, headers }))
        }
        "sse" => {
            reject_unexpected_fields(obj, name, &["type", "url", "headers"])?;
            let url = require_string_field(obj, name, "url")?.to_string();
            validate_remote_url(&url)
                .map_err(|reason| ServerConfigError::WrongType(format!("server '{name}' has invalid url: {reason}")))?;

            let headers = obj
                .get("headers")
                .map(|v| expect_string_map(v, &format!("server '{name}' headers")))
                .transpose()?
                .unwrap_or_default();
            validate_headers(&headers, name)?;

            Ok(ServerConfig::Sse(SseServerConfig { url, headers }))
        }
        _ => Err(ServerConfigError::WrongType(format!("server '{}' has unsupported type: {}", name, type_val))),
    }
}

fn reject_unexpected_fields(
    obj: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    allowed: &[&str],
) -> Result<(), ServerConfigError> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ServerConfigError::WrongType(format!("server '{name}' has unsupported field '{key}'")));
        }
    }
    Ok(())
}

fn validate_remote_url(url: &str) -> Result<(), String> {
    // Schemes are case-insensitive per RFC; compare case-folded prefixes.
    let (scheme, rest) = if url.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://")) {
        ("https", url.get(8..).unwrap_or_default())
    } else if url.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://")) {
        ("http", url.get(7..).unwrap_or_default())
    } else {
        return Err("url must be absolute http or https".into());
    };
    if rest.is_empty() {
        return Err("url must have a host".into());
    }
    if url.contains('#') {
        return Err("url must not contain a fragment".into());
    }
    // Authority is up to the first /, ?, or end. `/` and `?` are ASCII so the
    // byte index is always a char boundary; use `get` to stay clippy-clean.
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let Some(authority) = rest.get(..authority_end) else {
        return Err("url must have a host".into());
    };
    if authority.is_empty() {
        return Err("url must have a host".into());
    }
    if authority.contains('@') {
        return Err("url must not contain user information".into());
    }
    // Host without port and without IPv6 brackets.
    let host_port = authority;
    let host = if let Some(stripped) = host_port.strip_prefix('[') {
        let Some(close) = stripped.find(']') else {
            return Err("url has malformed IPv6 host".into());
        };
        let Some(inside) = stripped.get(..close) else {
            return Err("url has malformed IPv6 host".into());
        };
        let Some(after) = stripped.get(close + 1..) else {
            return Err("url has malformed host".into());
        };
        if !after.is_empty() && !after.starts_with(':') {
            return Err("url has malformed host".into());
        }
        inside
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    if host.is_empty() {
        return Err("url must have a host".into());
    }
    if scheme == "http" && !is_loopback_host(host) {
        return Err("http url must use localhost or a loopback IP".into());
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" {
        return true;
    }
    if is_loopback_ipv4(&lower) {
        return true;
    }
    // IPv6 loopback is exactly ::1 (compressed) or its expanded form.
    if lower == "::1" || lower == "0:0:0:0:0:0:0:1" {
        return true;
    }
    false
}

fn is_loopback_ipv4(host: &str) -> bool {
    let mut parts = host.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if first != "127" {
        return false;
    }
    for _ in 0..3 {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        if part.parse::<u8>().is_err() {
            return false;
        }
    }
    parts.next().is_none()
}

fn validate_headers(headers: &HashMap<String, String>, server: &str) -> Result<(), ServerConfigError> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (name, value) in headers {
        if !is_valid_header_name(name) {
            return Err(ServerConfigError::WrongType(format!("server '{server}' has invalid header name '{name}'")));
        }
        if !is_valid_header_value(value) {
            return Err(ServerConfigError::WrongType(format!(
                "server '{server}' has invalid header value for '{name}'"
            )));
        }
        let folded = name.to_ascii_lowercase();
        if !seen.insert(folded) {
            return Err(ServerConfigError::WrongType(format!("server '{server}' has duplicate header '{name}'")));
        }
    }
    Ok(())
}

fn is_valid_header_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
    })
}

fn is_valid_header_value(value: &str) -> bool {
    // Visible package data: reject control bytes that would split the header.
    !value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
}

fn expect_string_array(value: &serde_json::Value, context: &str) -> Result<Vec<String>, ServerConfigError> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or_else(|| ServerConfigError::WrongType(format!("{context} must contain only strings")))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|| Err(ServerConfigError::WrongType(format!("{context} must be an array"))))
}

fn expect_string_map(value: &serde_json::Value, context: &str) -> Result<HashMap<String, String>, ServerConfigError> {
    value
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| {
                    v.as_str()
                        .map(|s| (k.clone(), s.to_string()))
                        .ok_or_else(|| ServerConfigError::WrongType(format!("{context}.{k} must be a string")))
                })
                .collect::<Result<HashMap<_, _>, _>>()
        })
        .unwrap_or_else(|| Err(ServerConfigError::WrongType(format!("{context} must be an object"))))
}
