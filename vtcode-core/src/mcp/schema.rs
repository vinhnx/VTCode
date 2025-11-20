/// JSON Schema generation and validation for MCP tools
///
/// This module provides JSON Schema 2020-12 validation using jsonschema library.
/// Phase 1 provided basic type checking; Phase 2 adds full schema validation.
use anyhow::Result;
use serde_json::{Value, json};

/// Validate input against a JSON Schema (Phase 2 - Full JSON Schema 2020-12)
///
/// # Arguments
/// * `schema` - The JSON Schema to validate against (JSON Schema 2020-12)
/// * `input` - The input value to validate
///
/// # Returns
/// * `Ok(())` if validation succeeds
/// * `Err` with detailed error message if validation fails
///
/// # Validation Coverage (Phase 2)
/// - Type validation (string, number, integer, boolean, object, array)
/// - Required properties
/// - Min/max constraints (minLength, maxLength, minimum, maximum)
/// - Pattern matching (regex)
/// - Enum validation
/// - Nested objects and arrays
/// - Complex schemas (oneOf, anyOf, allOf, not)
pub fn validate_against_schema(schema: &Value, input: &Value) -> Result<()> {
    if input.is_null() {
        anyhow::bail!("Input cannot be null");
    }

    // Use jsonschema for full JSON Schema 2020-12 validation
    jsonschema::validate(schema, input)
        .map_err(|err| anyhow::anyhow!("Schema validation failed: {}", err))
}

/// Validate tool input parameters (Phase 2 - Full validation)
///
/// Validates tool input against the tool's schema using full JSON Schema support.
pub fn validate_tool_input(input_schema: Option<&Value>, input: &Value) -> Result<()> {
    if input.is_null() {
        anyhow::bail!("Tool input cannot be null");
    }

    // If schema is provided, validate against it
    if let Some(schema) = input_schema {
        validate_against_schema(schema, input)?;
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
    fn test_validate_required_properties() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" }
            },
            "required": ["name"]
        });

        let valid = json!({"name": "John"});
        assert!(validate_against_schema(&schema, &valid).is_ok());

        let invalid = json!({"age": 30});
        assert!(validate_against_schema(&schema, &invalid).is_err());
    }

    #[test]
    fn test_validate_string_length_constraints() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50
                }
            }
        });

        let valid = json!({"name": "John"});
        assert!(validate_against_schema(&schema, &valid).is_ok());

        let invalid_empty = json!({"name": ""});
        assert!(validate_against_schema(&schema, &invalid_empty).is_err());

        let invalid_long = json!({"name": "x".repeat(51)});
        assert!(validate_against_schema(&schema, &invalid_long).is_err());
    }

    #[test]
    fn test_validate_enum_values() {
        let schema = json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["active", "inactive", "pending"]
                }
            }
        });

        let valid = json!({"status": "active"});
        assert!(validate_against_schema(&schema, &valid).is_ok());

        let invalid = json!({"status": "unknown"});
        assert!(validate_against_schema(&schema, &invalid).is_err());
    }

    #[test]
    fn test_validate_array_items() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        });

        let valid = json!({"tags": ["rust", "mcp"]});
        assert!(validate_against_schema(&schema, &valid).is_ok());

        let invalid = json!({"tags": ["rust", 123]});
        assert!(validate_against_schema(&schema, &invalid).is_err());
    }

    #[test]
    fn test_validate_nested_objects() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "age": { "type": "integer" }
                    },
                    "required": ["name"]
                }
            }
        });

        let valid = json!({"user": {"name": "John", "age": 30}});
        assert!(validate_against_schema(&schema, &valid).is_ok());

        let invalid = json!({"user": {"age": 30}});
        assert!(validate_against_schema(&schema, &invalid).is_err());
    }

    #[test]
    fn test_validate_tool_input_with_no_schema() {
        let input = json!({"any": "value"});
        assert!(validate_tool_input(None, &input).is_ok());
    }

    #[test]
    fn test_validate_tool_input_with_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        });

        let valid = json!({"path": "/home"});
        assert!(validate_tool_input(Some(&schema), &valid).is_ok());

        let invalid = json!({});
        assert!(validate_tool_input(Some(&schema), &invalid).is_err());
    }

    #[test]
    fn test_simple_schema() {
        let schema = simple_schema();
        let input = json!({});
        assert!(validate_against_schema(&schema, &input).is_ok());
    }

    #[test]
    fn test_null_input_rejection() {
        let schema = json!({"type": "object"});
        let null_input = json!(null);
        assert!(validate_against_schema(&schema, &null_input).is_err());
    }
}
