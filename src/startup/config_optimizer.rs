//! Configuration loading optimizer to eliminate redundant serialization
//!
//! This module provides configuration override application using JSON manipulation
//! for a flexible, schema-agnostic approach that works with any config structure.

use anyhow::{Context, Result};
use vtcode_config::loader::VTCodeConfig;

/// Configuration override application using JSON manipulation
pub struct ConfigOptimizer;

impl ConfigOptimizer {
    /// Apply configuration overrides using JSON manipulation
    ///
    /// This approach is flexible and works with any config structure changes
    /// without requiring manual field mapping updates.
    pub fn apply_overrides(
        config: &mut VTCodeConfig,
        overrides: &[(String, String)],
    ) -> Result<()> {
        if overrides.is_empty() {
            return Ok(());
        }

        // Convert config to JSON, apply overrides, convert back
        let mut json_value =
            serde_json::to_value(&*config).context("Failed to serialize config to JSON")?;

        for (key, value) in overrides {
            let segments: Vec<&str> = key.split('.').collect();
            apply_json_override(&mut json_value, &segments, value)
                .with_context(|| format!("Failed to apply override for key '{key}'"))?;
        }

        *config =
            serde_json::from_value(json_value).context("Failed to deserialize config from JSON")?;

        // Validate the updated configuration
        config
            .validate()
            .context("Configuration overrides failed validation")?;

        Ok(())
    }
}

/// Apply override to JSON value using path segments
fn apply_json_override(target: &mut serde_json::Value, path: &[&str], value: &str) -> Result<()> {
    let parsed_value = parse_json_value(value)?;

    let mut current = target;
    for (i, segment) in path.iter().enumerate() {
        if i == path.len() - 1 {
            // Last segment - set the value
            if let Some(obj) = current.as_object_mut() {
                obj.insert(segment.to_string(), parsed_value.clone());
            } else if let Some(arr) = current.as_array_mut() {
                if let Ok(index) = segment.parse::<usize>() {
                    if index < arr.len() {
                        arr[index] = parsed_value.clone();
                    } else {
                        return Err(anyhow::anyhow!("Array index out of bounds: {index}"));
                    }
                } else {
                    return Err(anyhow::anyhow!("Invalid array index: {segment}"));
                }
            } else {
                return Err(anyhow::anyhow!("Cannot set value on non-object/array"));
            }
        } else {
            // Navigate to the next level, creating objects as needed
            if let Some(obj) = current.as_object_mut() {
                if !obj.contains_key(*segment) {
                    obj.insert(
                        segment.to_string(),
                        serde_json::Value::Object(serde_json::Map::new()),
                    );
                }
                current = obj.get_mut(*segment).unwrap();
            } else {
                return Err(anyhow::anyhow!("Path segment '{segment}' not found"));
            }
        }
    }

    Ok(())
}

/// Parse a JSON value from string, trying various formats
fn parse_json_value(raw: &str) -> Result<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Value::String(String::new()));
    }

    // Try common boolean values
    if trimmed.eq_ignore_ascii_case("true") {
        return Ok(serde_json::Value::Bool(true));
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Ok(serde_json::Value::Bool(false));
    }

    // Try to parse as integer
    if let Ok(n) = trimmed.parse::<i64>() {
        return Ok(serde_json::Value::Number(n.into()));
    }

    // Try to parse as float
    if let Ok(n) = trimmed.parse::<f64>()
        && let Some(num) = serde_json::Number::from_f64(n) {
            return Ok(serde_json::Value::Number(num));
        }

    // Try to parse as JSON
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(json);
    }

    // Try to parse as TOML and convert to JSON
    if let Ok(toml) = toml::from_str::<toml::Value>(trimmed) {
        return Ok(toml_to_json(&toml));
    }

    // Fall back to string
    Ok(serde_json::Value::String(trimmed.to_owned()))
}

/// Convert TOML value to JSON value
fn toml_to_json(toml: &toml::Value) -> serde_json::Value {
    match toml {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => {
            let map = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_value_bool() {
        assert_eq!(
            parse_json_value("true").unwrap(),
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            parse_json_value("false").unwrap(),
            serde_json::Value::Bool(false)
        );
        assert_eq!(
            parse_json_value("TRUE").unwrap(),
            serde_json::Value::Bool(true)
        );
    }

    #[test]
    fn test_parse_json_value_numeric() {
        assert_eq!(parse_json_value("42").unwrap(), serde_json::json!(42));
        assert_eq!(parse_json_value("3.14").unwrap(), serde_json::json!(3.14));
    }

    #[test]
    fn test_parse_json_value_string() {
        assert_eq!(
            parse_json_value("hello").unwrap(),
            serde_json::Value::String("hello".to_string())
        );
    }

    #[test]
    fn test_json_override() {
        let mut json = serde_json::json!({
            "agent": {
                "provider": "gemini"
            }
        });

        apply_json_override(&mut json, &["agent", "provider"], "openai").unwrap();
        assert_eq!(json["agent"]["provider"], "openai");
    }

    #[test]
    fn test_json_override_nested() {
        let mut json = serde_json::json!({
            "level1": {}
        });

        apply_json_override(&mut json, &["level1", "level2", "value"], "test").unwrap();
        assert_eq!(json["level1"]["level2"]["value"], "test");
    }
}
