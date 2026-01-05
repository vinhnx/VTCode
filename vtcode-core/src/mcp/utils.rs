//! Utility functions for MCP client operations.

use anyhow::{Context, Result};
use chrono::Local;
use iana_time_zone::get_timezone;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::env;
use tracing::{debug, warn};

/// Environment variable for explicit local timezone override.
pub const LOCAL_TIMEZONE_ENV_VAR: &str = "VTCODE_LOCAL_TIMEZONE";
/// Standard TZ environment variable fallback.
pub const TZ_ENV_VAR: &str = "TZ";
/// Argument name for timezone injection.
pub const TIMEZONE_ARGUMENT: &str = "timezone";

/// Ensure a timezone argument is present when required by the schema.
pub fn ensure_timezone_argument(
    arguments: &mut Map<String, Value>,
    requires_timezone: bool,
) -> Result<()> {
    if !requires_timezone || arguments.contains_key(TIMEZONE_ARGUMENT) {
        return Ok(());
    }

    let timezone = detect_local_timezone()
        .context("failed to determine a default timezone for MCP tool invocation")?;
    debug!("Injecting local timezone '{timezone}' for MCP tool call");
    arguments.insert(TIMEZONE_ARGUMENT.to_string(), Value::String(timezone));
    Ok(())
}

/// Detect the local timezone using environment variables or system detection.
pub fn detect_local_timezone() -> Result<String> {
    if let Ok(value) = env::var(LOCAL_TIMEZONE_ENV_VAR) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Ok(value) = env::var(TZ_ENV_VAR) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    match get_timezone() {
        Ok(timezone) => Ok(timezone),
        Err(err) => {
            let fallback = Local::now().format("%:z").to_string();
            warn!(
                "Falling back to numeric offset '{fallback}' after failing to resolve IANA timezone: {err}"
            );
            Ok(fallback)
        }
    }
}

/// Check if a JSON schema requires a specific field.
pub fn schema_requires_field(schema: &Value, field: &str) -> bool {
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
                    && subschemas
                        .iter()
                        .any(|subschema| schema_requires_field(subschema, field))
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
pub fn build_headers(
    static_headers: &HashMap<String, String>,
    env_headers: &HashMap<String, String>,
) -> HeaderMap {
    let mut map = HeaderMap::new();

    for (key, value) in static_headers {
        match HeaderName::from_bytes(key.as_bytes()) {
            Ok(name) => match HeaderValue::from_str(value) {
                Ok(header_value) => {
                    map.insert(name, header_value);
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
        match env::var(env_var) {
            Ok(value) if !value.trim().is_empty() => match HeaderName::from_bytes(key.as_bytes()) {
                Ok(name) => match HeaderValue::from_str(&value) {
                    Ok(header_value) => {
                        map.insert(name, header_value);
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
            Ok(_) => {
                debug!(
                    header = key.as_str(),
                    env_var = env_var.as_str(),
                    "Skipping MCP HTTP header from environment because the value is empty"
                );
            }
            Err(_) => {
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
