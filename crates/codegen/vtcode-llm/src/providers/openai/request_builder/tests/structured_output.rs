//! Translation and validation of provider-agnostic `output_format` for OpenAI.

use super::super::{chat_response_format, responses_text_format, validate_output_format};
use crate::provider::LLMError;
use serde_json::{Value, json};

fn strict_format(schema: Value) -> Value {
    json!({
        "type": "json_schema",
        "json_schema": {"name": "event", "schema": schema, "strict": true}
    })
}

fn invalid_message(format: &Value) -> String {
    let Err(LLMError::InvalidRequest { message, .. }) = validate_output_format(Some(format), "OpenAI") else {
        panic!("expected an InvalidRequest error for {format:?}");
    };
    message
}

#[test]
fn output_format_translates_to_both_openai_wire_shapes() {
    let schema = json!({
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"],
        "additionalProperties": false
    });
    let chat_shape = json!({
        "type": "json_schema",
        "json_schema": {"name": "event", "schema": schema.clone(), "strict": false}
    });
    let responses_shape = json!({
        "type": "json_schema", "name": "event", "schema": schema.clone(), "strict": false
    });

    // Nested (Chat-style) and flat (Responses-style) descriptors are both accepted.
    assert_eq!(chat_response_format(Some(&chat_shape)), Some(chat_shape.clone()));
    assert_eq!(responses_text_format(Some(&chat_shape)), Some(responses_shape.clone()));
    assert_eq!(chat_response_format(Some(&responses_shape)), Some(chat_shape));
    assert_eq!(responses_text_format(Some(&responses_shape)), Some(responses_shape));

    // JSON mode and unknown shapes stay narrow: never forward a bad descriptor.
    assert_eq!(chat_response_format(Some(&json!({"type": "json_object"}))), Some(json!({"type": "json_object"})));
    assert_eq!(responses_text_format(Some(&json!({"type": "json_object"}))), Some(json!({"type": "json_object"})));
    assert_eq!(chat_response_format(Some(&json!({"type": "bogus"}))), None);
    assert_eq!(responses_text_format(None), None);
}

#[test]
fn output_format_defaults_to_strict_and_keeps_description() {
    let format = json!({
        "type": "json_schema",
        "name": "event",
        "description": "An extracted event",
        "schema": {"type": "object"}
    });
    let responses = responses_text_format(Some(&format)).expect("schema should translate");
    assert_eq!(responses["strict"], true);
    assert_eq!(responses["description"], "An extracted event");
    let chat = chat_response_format(Some(&format)).expect("schema should translate");
    assert_eq!(chat["json_schema"]["strict"], true);
    assert_eq!(chat["json_schema"]["description"], "An extracted event");
}

#[test]
fn strict_schema_validation_accepts_valid_nested_and_flat_descriptors() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "count": {"type": "integer"},
            "tags": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["name", "count", "tags"],
        "additionalProperties": false
    });
    for format in [
        strict_format(schema.clone()),
        json!({"type": "json_schema", "name": "event", "schema": schema, "strict": true}),
    ] {
        validate_output_format(Some(&format), "OpenAI").expect("strict schema should validate");
    }
    validate_output_format(None, "OpenAI").expect("no format should validate");
}

#[test]
fn strict_schema_validation_skips_json_mode_non_strict_and_unknown_shapes() {
    // JSON mode has no schema; non-strict schemas are not bound by the strict
    // subset; unknown descriptors are dropped by the wire translation.
    for format in [
        json!({"type": "json_object"}),
        json!({"type": "json_schema", "json_schema": {"name": "x", "strict": false, "schema": {"type": "string"}}}),
        json!({"type": "bogus"}),
    ] {
        validate_output_format(Some(&format), "OpenAI").expect("format should be exempt from strict checks");
    }
}

#[test]
fn strict_schema_validation_rejects_non_object_root() {
    let message = invalid_message(&strict_format(json!({
        "anyOf": [
            {"type": "object", "properties": {}, "required": [], "additionalProperties": false},
            {"type": "null"}
        ]
    })));
    assert!(message.contains("anyOf"), "{message}");

    let message = invalid_message(&strict_format(json!({"type": "string"})));
    assert!(message.contains("root must have type 'object'"), "{message}");
}

#[test]
fn strict_schema_validation_requires_additional_properties_false() {
    let missing = strict_format(json!({
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"]
    }));
    let message = invalid_message(&missing);
    assert!(message.contains("additionalProperties"), "{message}");

    let permissive = strict_format(json!({
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"],
        "additionalProperties": true
    }));
    let message = invalid_message(&permissive);
    assert!(message.contains("additionalProperties"), "{message}");
}

#[test]
fn strict_schema_validation_requires_every_property_in_required() {
    let format = strict_format(json!({
        "type": "object",
        "properties": {"name": {"type": "string"}, "optional": {"type": "boolean"}},
        "required": ["name"],
        "additionalProperties": false
    }));
    let message = invalid_message(&format);
    assert!(message.contains("optional"), "{message}");
    assert!(message.contains("required"), "{message}");
}

#[test]
fn strict_schema_validation_rejects_unsupported_keywords_recursively() {
    let format = strict_format(json!({
        "type": "object",
        "properties": {
            "child": {
                "type": "object",
                "properties": {"value": {"type": "string", "allOf": [{"type": "string"}]}},
                "required": ["value"],
                "additionalProperties": false
            }
        },
        "required": ["child"],
        "additionalProperties": false
    }));
    let message = invalid_message(&format);
    assert!(message.contains("allOf"), "{message}");
    assert!(message.contains("properties.child"), "{message}");
}

#[test]
fn strict_schema_validation_walks_nested_objects_arrays_anyof_and_defs() {
    // A violation deep in an array item is still caught before the wire call.
    let nested = strict_format(json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {"$ref": "#/$defs/item"}
            }
        },
        "required": ["items"],
        "additionalProperties": false,
        "$defs": {
            "item": {
                "type": "object",
                "properties": {"id": {"type": "string"}},
                "additionalProperties": false
            }
        }
    }));
    let message = invalid_message(&nested);
    assert!(message.contains("$defs.item"), "{message}");
    assert!(message.contains("required"), "{message}");
}

#[test]
fn strict_schema_validation_accepts_tuple_items_and_catches_nested_violations() {
    let valid = strict_format(json!({
        "type": "object",
        "properties": {
            "pair": {
                "type": "array",
                "prefixItems": [
                    {"type": "string"},
                    {"type": "integer"}
                ]
            }
        },
        "required": ["pair"],
        "additionalProperties": false
    }));
    validate_output_format(Some(&valid), "OpenAI").expect("tuple prefixItems should validate");

    let invalid = strict_format(json!({
        "type": "object",
        "properties": {
            "pair": {
                "type": "array",
                "prefixItems": [
                    {"type": "string"},
                    {"type": "object", "properties": {"x": {"type": "string"}}}
                ]
            }
        },
        "required": ["pair"],
        "additionalProperties": false
    }));
    let message = invalid_message(&invalid);
    assert!(message.contains("prefixItems[1]"), "{message}");
    assert!(message.contains("additionalProperties"), "{message}");
}

#[test]
fn strict_schema_validation_allows_nullable_anyof_branches() {
    let format = strict_format(json!({
        "type": "object",
        "properties": {
            "unit": {
                "anyOf": [
                    {"type": "string", "enum": ["F", "C"]},
                    {"type": "null"}
                ]
            }
        },
        "required": ["unit"],
        "additionalProperties": false
    }));
    validate_output_format(Some(&format), "OpenAI").expect("nullable union branch should validate");
}
