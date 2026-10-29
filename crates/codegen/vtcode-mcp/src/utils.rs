//! Utility functions for MCP client operations.

use anyhow::{Context, Result, bail};
use chrono::Local;
use hashbrown::HashMap;
use iana_time_zone::get_timezone;
use rmcp::model::{ClientCapabilities, Implementation};
use rmcp_reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::env;
#[cfg(test)]
use std::sync::{LazyLock, Mutex};
use tracing::{debug, warn};
use vtcode_config::mcp::{McpUiAppsTier, McpUiConfig};

use crate::types::{MCP_UI_EXTENSION_ID, MCP_UI_RESOURCE_MIME_TYPE};

#[cfg(test)]
static TEST_ENV_OVERRIDES: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
fn get_test_env_override(key: &str) -> Option<Option<String>> {
    TEST_ENV_OVERRIDES.lock().ok().and_then(|map| map.get(key).cloned())
}

fn read_env_var(key: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(override_value) = get_test_env_override(key) {
        return override_value;
    }

    env::var(key).ok()
}

#[cfg(test)]
pub(crate) fn set_test_env_override(key: &str, value: Option<&str>) {
    if let Ok(mut map) = TEST_ENV_OVERRIDES.lock() {
        drop(map.insert(key.to_owned(), value.map(ToOwned::to_owned)));
    }
}

#[cfg(test)]
pub(crate) fn clear_test_env_override(key: &str) {
    if let Ok(mut map) = TEST_ENV_OVERRIDES.lock() {
        drop(map.remove(key));
    }
}

/// Build the standard Implementation struct for vtcode MCP client.
///
/// This ensures consistent client identification across all MCP connections.
pub(crate) fn build_client_implementation() -> Implementation {
    Implementation::new("vtcode", env!("CARGO_PKG_VERSION"))
}

/// Insert the MCP Apps extension advertisement into client capabilities.
///
/// Follows SEP-1865 capability negotiation:
/// `extensions["io.modelcontextprotocol/ui"] = {"mimeTypes": ["text/html;profile=mcp-app"]}`.
/// A no-op when `advertise` is false; a pre-existing entry is preserved, never
/// replaced.
pub(crate) fn apply_ui_extension_gate(capabilities: &mut ClientCapabilities, advertise: bool) {
    if !advertise {
        return;
    }
    let _ = capabilities
        .extensions
        .get_or_insert_with(BTreeMap::new)
        .entry(MCP_UI_EXTENSION_ID.to_owned())
        .or_insert_with(|| {
            Map::from_iter([(
                "mimeTypes".to_owned(),
                Value::Array(vec![Value::String(MCP_UI_RESOURCE_MIME_TYPE.to_owned())]),
            )])
        });
}

/// Reject the interactive MCP Apps tier until a sandboxed webview host exists.
///
/// Call at MCP entry points before connecting any provider so the failure is
/// loud. Per-provider errors are tolerated by the pool, so gating inside
/// parameter construction would silently degrade to zero providers.
pub(crate) fn reject_unsupported_ui_apps_tier(ui: &McpUiConfig) -> Result<()> {
    if ui.apps == McpUiAppsTier::Interactive {
        bail!(
            "mcp.ui.apps = \"interactive\" is not supported yet: interactive MCP Apps require a sandboxed webview \
             host. Use \"off\" (default) or \"static\" until then."
        );
    }
    Ok(())
}

/// Environment variable for explicit local timezone override.
pub(crate) const LOCAL_TIMEZONE_ENV_VAR: &str = "VTCODE_LOCAL_TIMEZONE";
/// Standard TZ environment variable fallback.
pub(crate) const TZ_ENV_VAR: &str = "TZ";
/// Argument name for timezone injection.
pub(crate) const TIMEZONE_ARGUMENT: &str = "timezone";

/// Ensure a timezone argument is present when required by the schema.
pub(crate) fn ensure_timezone_argument(arguments: &mut Map<String, Value>, requires_timezone: bool) -> Result<()> {
    if !requires_timezone {
        return Ok(());
    }

    let timezone = detect_local_timezone().context("failed to determine a default timezone for MCP tool invocation")?;
    debug!("Injecting local timezone '{timezone}' for MCP tool call");
    let _entry = arguments
        .entry(TIMEZONE_ARGUMENT.to_string())
        .or_insert_with(|| Value::String(timezone));
    Ok(())
}

/// Detect the local timezone using environment variables or system detection.
pub(crate) fn detect_local_timezone() -> Result<String> {
    if let Some(value) = read_env_var(LOCAL_TIMEZONE_ENV_VAR) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Some(value) = read_env_var(TZ_ENV_VAR) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    match get_timezone() {
        Ok(timezone) => Ok(timezone),
        Err(err) => {
            let fallback = Local::now().format("%:z").to_string();
            warn!("Falling back to numeric offset '{fallback}' after failing to resolve IANA timezone: {err}");
            Ok(fallback)
        }
    }
}

/// Check if a JSON schema requires a specific field.
pub(crate) fn schema_requires_field(schema: &Value, field: &str) -> bool {
    match schema {
        Value::Object(map) => {
            if map
                .get("required")
                .and_then(Value::as_array)
                .map(|items| items.iter().any(|item| item.as_str() == Some(field)))
                .unwrap_or(false)
            {
                return true;
            }

            for keyword in ["allOf", "anyOf", "oneOf"] {
                if let Some(subschemas) = map.get(keyword).and_then(Value::as_array)
                    && subschemas.iter().any(|subschema| schema_requires_field(subschema, field))
                {
                    return true;
                }
            }

            if let Some(items) = map.get("items")
                && schema_requires_field(items, field)
            {
                return true;
            }

            if let Some(properties) = map.get("properties").and_then(Value::as_object)
                && let Some(property_schema) = properties.get(field)
                && schema_requires_field(property_schema, field)
            {
                return true;
            }

            false
        }
        _ => false,
    }
}

/// Build HTTP headers from static and environment-based configuration.
pub(crate) fn build_headers(
    static_headers: &HashMap<String, String>,
    env_headers: &HashMap<String, String>,
) -> HeaderMap {
    let mut map = HeaderMap::new();

    for (key, value) in static_headers {
        match HeaderName::from_bytes(key.as_bytes()) {
            Ok(name) => match HeaderValue::from_str(value) {
                Ok(header_value) => {
                    drop(map.insert(name, header_value));
                }
                Err(err) => {
                    warn!(
                        header = key.as_str(),
                        error = %err,
                        "Skipping MCP HTTP header with invalid value"
                    );
                }
            },
            Err(err) => {
                warn!(
                    header = key.as_str(),
                    error = %err,
                    "Skipping MCP HTTP header with invalid name"
                );
            }
        }
    }

    for (key, env_var) in env_headers {
        match read_env_var(env_var) {
            Some(value) if !value.trim().is_empty() => match HeaderName::from_bytes(key.as_bytes()) {
                Ok(name) => match HeaderValue::from_str(&value) {
                    Ok(header_value) => {
                        drop(map.insert(name, header_value));
                    }
                    Err(err) => {
                        warn!(
                            header = key.as_str(),
                            env_var = env_var.as_str(),
                            error = %err,
                            "Skipping MCP HTTP header from environment with invalid value"
                        );
                    }
                },
                Err(err) => {
                    warn!(
                        header = key.as_str(),
                        env_var = env_var.as_str(),
                        error = %err,
                        "Skipping MCP HTTP header from environment with invalid name"
                    );
                }
            },
            Some(_) => {
                debug!(
                    header = key.as_str(),
                    env_var = env_var.as_str(),
                    "Skipping MCP HTTP header from environment because the value is empty"
                );
            }
            None => {
                debug!(
                    header = key.as_str(),
                    env_var = env_var.as_str(),
                    "Skipping MCP HTTP header from environment because the variable is unset"
                );
            }
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::{apply_ui_extension_gate, reject_unsupported_ui_apps_tier};
    use rmcp::model::ClientCapabilities;
    use serde_json::Value;
    use vtcode_config::mcp::{McpUiAppsTier, McpUiConfig};

    #[test]
    fn ui_extension_gate_advertises_exact_settings_object() {
        let mut capabilities = ClientCapabilities::default();
        apply_ui_extension_gate(&mut capabilities, true);

        let extensions = capabilities.extensions.as_ref().expect("extensions advertised");
        let settings = extensions.get("io.modelcontextprotocol/ui").expect("ui extension");
        assert_eq!(
            settings.get("mimeTypes"),
            Some(&Value::Array(vec![Value::String("text/html;profile=mcp-app".to_string())]))
        );
        assert_eq!(settings.len(), 1);

        // The advertisement survives an initialize-payload round trip.
        let payload = serde_json::to_value(&capabilities).expect("serialize capabilities");
        assert_eq!(
            payload["extensions"]["io.modelcontextprotocol/ui"]["mimeTypes"],
            serde_json::json!(["text/html;profile=mcp-app"])
        );
    }

    #[test]
    fn ui_extension_gate_is_noop_when_not_advertised() {
        let mut capabilities = ClientCapabilities::default();
        apply_ui_extension_gate(&mut capabilities, false);
        assert_eq!(capabilities.extensions, None);

        // A pre-existing entry is preserved, never removed.
        let mut capabilities = ClientCapabilities::default();
        apply_ui_extension_gate(&mut capabilities, true);
        apply_ui_extension_gate(&mut capabilities, false);
        assert!(capabilities.extensions.is_some());
    }

    #[test]
    fn ui_extension_gate_preserves_preexisting_settings() {
        use std::collections::BTreeMap;

        let mut capabilities = ClientCapabilities::default();
        capabilities.extensions = Some(BTreeMap::from([(
            "io.modelcontextprotocol/ui".to_string(),
            serde_json::Map::from_iter([
                ("mimeTypes".to_string(), serde_json::json!(["text/html;profile=mcp-app"])),
                ("future".to_string(), Value::Bool(true)),
            ]),
        )]));
        apply_ui_extension_gate(&mut capabilities, true);

        let extensions = capabilities.extensions.as_ref().expect("extensions");
        let settings = extensions.get("io.modelcontextprotocol/ui").expect("ui extension");
        assert_eq!(settings.get("future"), Some(&Value::Bool(true)));
        assert_eq!(settings.len(), 2);
    }

    #[test]
    fn ui_apps_tier_rejection_fires_only_on_interactive() {
        for tier in [McpUiAppsTier::Off, McpUiAppsTier::Static] {
            let ui = McpUiConfig { apps: tier, ..McpUiConfig::default() };
            reject_unsupported_ui_apps_tier(&ui).expect("supported tier");
        }
        let ui = McpUiConfig {
            apps: McpUiAppsTier::Interactive,
            ..McpUiConfig::default()
        };
        let err = reject_unsupported_ui_apps_tier(&ui).expect_err("interactive rejected");
        assert!(err.to_string().contains("interactive"), "{err:?}");
    }
}
