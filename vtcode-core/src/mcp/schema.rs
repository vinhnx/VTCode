/// JSON Schema generation and validation for MCP tools
///
/// This module provides type-safe schema generation using schemars
/// and validation using jsonschema crates.
///
/// Phase 1: Basic schema structure support. Full validation implemented in Phase 2.

use anyhow::Result;
use serde_json::{json, Value};

/// Validate input against a JSON Schema
///
/// # Arguments
/// * `schema` - The JSON Schema to validate against (JSON Schema 2020-12)
/// * `input` - The input value to validate
///
/// # Returns
/// * `Ok(())` if validation succeeds
/// * `Err` if input is null or invalid
///
/// Note: Phase 1 uses basic validation. Full JSON Schema validation will be
/// implemented in Phase 2 using jsonschema library capabilities.
pub fn validate_against_schema(schema: &Value, input: &Value) -> Result<()> {
    if input.is_null() {
        anyhow::bail!("Input cannot be null");
    }

    // Phase 1: Basic type checking for object schemas
    if let Some(expected_type) = schema.get("type").and_then(Value::as_str) {
        if expected_type == "object" && !input.is_object() {
            anyhow::bail!("Expected object, got {}", json_type_name(&input));
        }
    }

    // Phase 1: Check required properties have correct types
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some(input_obj) = input.as_object() {
            for (key, prop_schema) in properties.iter() {
                if let Some(value) = input_obj.get(key) {
                    if let Some(expected_type) = prop_schema.get("type").and_then(Value::as_str) {
                        let actual_type = match expected_type {
                            "string" => value.is_string(),
                            "number" => value.is_number(),
                            "integer" => value.is_number() && value.as_i64().is_some(),
                            "boolean" => value.is_boolean(),
                            "object" => value.is_object(),
                            "array" => value.is_array(),
                            _ => true,
                        };
                        if !actual_type {
                            anyhow::bail!(
                                "Property '{}': expected {}, got {}",
                                key,
                                expected_type,
                                json_type_name(&value)
                            );
                        }
                    }
                }
            }
        }
    }

    fn json_type_name(val: &Value) -> &'static str {
        if val.is_string() {
            "string"
        } else if val.is_number() {
            "number"
        } else if val.is_boolean() {
            "boolean"
        } else if val.is_object() {
            "object"
        } else if val.is_array() {
            "array"
        } else {
            "null"
        }
    }

    Ok(())
}

/// Validate tool input parameters
pub fn validate_tool_input(_input_schema: Option<&Value>, input: &Value) -> Result<()> {
    // Phase 1: Basic validation
    if input.is_null() {
        anyhow::bail!("Tool input cannot be null");
    }
    Ok(())
}

/// Build a simple JSON Schema for a tool with no specific input requirements
pub fn simple_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "$schema": "https://json-schema.org/draft/2020-12/schema"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_simple_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let valid_input = json!({"name": "test"});
        assert!(validate_against_schema(&schema, &valid_input).is_ok());

        let invalid_input = json!({"name": 123});
        assert!(validate_against_schema(&schema, &invalid_input).is_err());
    }

    #[test]
    fn test_validate_tool_input_with_no_schema() {
        let input = json!({"any": "value"});
        assert!(validate_tool_input(None, &input).is_ok());
    }

    #[test]
    fn test_simple_schema() {
        let schema = simple_schema();
        let input = json!({});
        assert!(validate_against_schema(&schema, &input).is_ok());
    }
}
