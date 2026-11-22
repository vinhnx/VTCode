//! Provider-level validation of strict Structured Outputs schemas on the wire path.

use super::*;
use crate::provider::LLMError;
use vtcode_config::constants::models;

fn strict_request(schema: Value) -> provider::LLMRequest {
    let mut request = sample_request(models::openai::GPT_5_6);
    request.output_format = Some(json!({
        "type": "json_schema",
        "json_schema": {"name": "event", "schema": schema, "strict": true}
    }));
    request
}

#[test]
fn response_payload_rejects_non_strict_compatible_schema() {
    let provider = native_openai_provider(models::openai::GPT_5_6);
    let request = strict_request(json!({
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "additionalProperties": false
    }));

    let Err(LLMError::InvalidRequest { message, .. }) = provider.convert_to_openai_responses_format(&request) else {
        panic!("a strict schema with a missing required entry must be rejected");
    };
    assert!(message.contains("required"), "{message}");
}

#[test]
fn chat_payload_accepts_strict_compatible_schema() {
    let provider = native_openai_provider(models::openai::GPT_5_6);
    let request = strict_request(json!({
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"],
        "additionalProperties": false
    }));
    provider
        .convert_to_openai_format(&request)
        .expect("a strict-compatible schema should be accepted");
}

#[test]
fn payloads_ignore_json_mode_and_non_strict_schemas() {
    let provider = native_openai_provider(models::openai::GPT_5_6);
    let mut request = sample_request(models::openai::GPT_5_6);
    request.output_format = Some(json!({"type": "json_object"}));
    provider
        .convert_to_openai_responses_format(&request)
        .expect("JSON mode has no strict schema");

    request.output_format = Some(json!({
        "type": "json_schema",
        "json_schema": {"name": "x", "strict": false, "schema": {"type": "string"}}
    }));
    provider
        .convert_to_openai_responses_format(&request)
        .expect("non-strict schemas are exempt");
}

#[test]
fn chatgpt_backend_drops_descriptor_instead_of_validating_it() {
    // ChatGPT subscription routes never emit `text.format`, so an invalid
    // strict schema there must not fail the request — the descriptor is absent.
    let provider = chatgpt_backend_provider(models::openai::GPT_5_6);
    let request = strict_request(json!({"type": "string"}));
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("descriptor is dropped before validation matters");
    assert!(payload.get("text").and_then(|text| text.get("format")).is_none());
}
