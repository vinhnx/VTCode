//! Translation of provider-agnostic `output_format` into OpenAI request fields.

use super::super::{chat_response_format, responses_text_format};
use serde_json::json;

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
