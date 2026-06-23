//! Shared OpenAI/ChatGPT Responses adapter boundary.
//!
//! Current VTCode request/event types enter through `provider::LLMRequest` and
//! `provider::LLMStreamEvent`; native OpenAI and ChatGPT both shape Responses
//! requests through `OpenAIProvider::convert_to_openai_responses_format`.
//! This module keeps the request-side boundary explicit: Rig 0.39 owns the
//! typed Responses state that it supports, while VTCode overlays remain local
//! until Rig exposes equivalent request JSON and replay semantics.
//!
//! Overlay removal conditions:
//! - Assistant phase metadata: remove when Rig Responses input items can carry
//!   replay-safe assistant phase metadata without custom JSON surgery.
//! - Open/unknown include strings: remove when Rig `Include` accepts typed
//!   known values plus provider-accepted custom strings without dropping them.
//! - `context_management`: remove when Rig exposes the Responses field with
//!   pass-through parity for current VTCode payloads.
//! - `prompt_cache_key` and retention: remove when Rig exposes typed fields and
//!   preserves native-only gating semantics.
//! - `output_types`: remove when Rig models GPT-5 item constraints including
//!   hosted shell additions.
//! - Nested `sampling_parameters`: remove when Rig supports that nested
//!   Responses shape instead of only top-level generation fields.
//! - `text.verbosity`: remove when Rig's text config models verbosity alongside
//!   structured/grammar formats.
//! - `custom_tool_call` and `custom_tool_call_output`: remove when Rig input
//!   items support custom tool replay and paired outputs.
//! - Rich hosted/custom tool payloads: remove when Rig tool definitions cover
//!   VTCode's hosted shell, tool search, remote MCP, and custom-tool payloads.

use crate::llm::provider::{LLMError, LLMRequest};
use rig::providers::openai::responses_api::{
    AdditionalParameters as RigResponsesAdditionalParameters, Include as RigResponsesInclude,
};
use serde_json::{Value, json};

use super::responses_api::build_standard_responses_payload;
use super::types::OpenAIResponsesPayload;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResponsesItemAdapterOptions {
    pub include_structured_history_in_input: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PromptCacheOverlay<'a> {
    pub prompt_cache_key: Option<&'a str>,
    pub include_prompt_cache_retention: bool,
    pub is_responses_api_model: bool,
    pub prompt_cache_retention: Option<&'a str>,
}

pub(crate) fn map_request_items_to_responses(
    request: &LLMRequest,
    options: ResponsesItemAdapterOptions,
) -> Result<OpenAIResponsesPayload, LLMError> {
    build_standard_responses_payload(request, options.include_structured_history_in_input)
}

pub(crate) fn strip_assistant_phase_overlay(input: &mut [Value]) {
    for item in input {
        if let Some(map) = item.as_object_mut() {
            map.remove("phase");
        }
    }
}

pub(crate) fn rig_supported_state_parameters(
    previous_response_id: Option<&str>,
    store: Option<bool>,
) -> RigResponsesAdditionalParameters {
    RigResponsesAdditionalParameters {
        previous_response_id: previous_response_id.map(ToOwned::to_owned),
        store,
        ..Default::default()
    }
}

pub(crate) fn merge_rig_supported_state(
    openai_request: &mut Value,
    params: RigResponsesAdditionalParameters,
) {
    let Ok(Value::Object(fields)) = serde_json::to_value(params) else {
        return;
    };
    let Some(request) = openai_request.as_object_mut() else {
        return;
    };

    request.extend(fields);
}

pub(crate) fn map_include_fields(
    include_fields: Option<&[String]>,
    include_encrypted_reasoning: bool,
) -> Option<Value> {
    let mut include_values = Vec::new();
    if let Some(include_fields) = include_fields {
        for field in include_fields {
            push_unique_include(&mut include_values, field);
        }
    }
    if include_encrypted_reasoning {
        push_unique_include(&mut include_values, "reasoning.encrypted_content");
    }

    (!include_values.is_empty()).then(|| {
        Value::Array(
            include_values
                .iter()
                .map(|field| responses_include_value(field))
                .collect(),
        )
    })
}

pub(crate) fn apply_prompt_cache_overlay(
    openai_request: &mut Value,
    overlay: PromptCacheOverlay<'_>,
) {
    let Some(request) = openai_request.as_object_mut() else {
        return;
    };

    if let Some(prompt_cache_key) = trimmed_non_empty(overlay.prompt_cache_key) {
        request
            .entry("prompt_cache_key".to_string())
            .or_insert_with(|| json!(prompt_cache_key));
    }

    if overlay.include_prompt_cache_retention
        && overlay.is_responses_api_model
        && let Some(retention) = trimmed_non_empty(overlay.prompt_cache_retention)
    {
        request
            .entry("prompt_cache_retention".to_string())
            .or_insert_with(|| json!(retention));
    }
}

fn push_unique_include(include_values: &mut Vec<String>, field: &str) {
    let field = field.trim();
    if field.is_empty() || include_values.iter().any(|value| value == field) {
        return;
    }

    include_values.push(field.to_string());
}

fn rig_include_for_field(field: &str) -> Option<RigResponsesInclude> {
    match field {
        "file_search_call.results" => Some(RigResponsesInclude::FileSearchCallResults),
        "message.input_image.image_url" => Some(RigResponsesInclude::MessageInputImageImageUrl),
        "computer_call.output.image_url" => {
            Some(RigResponsesInclude::ComputerCallOutputOutputImageUrl)
        }
        "reasoning.encrypted_content" => Some(RigResponsesInclude::ReasoningEncryptedContent),
        "code_interpreter_call.outputs" => Some(RigResponsesInclude::CodeInterpreterCallOutputs),
        _ => None,
    }
}

fn responses_include_value(field: &str) -> Value {
    rig_include_for_field(field)
        .and_then(|include| serde_json::to_value(include).ok())
        .unwrap_or_else(|| json!(field))
}

fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        PromptCacheOverlay, ResponsesItemAdapterOptions, apply_prompt_cache_overlay,
        map_include_fields, map_request_items_to_responses, rig_supported_state_parameters,
        strip_assistant_phase_overlay,
    };
    use crate::llm::provider::{AssistantPhase, LLMRequest, Message, ToolCall};
    use serde_json::{Value, json};

    #[test]
    fn maps_request_items_to_responses_input_shapes() {
        let request = LLMRequest {
            messages: vec![
                Message::user("run tests".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_1".to_string(),
                        "unified_exec".to_string(),
                        "{\"cmd\":\"cargo test\"}".to_string(),
                    )],
                ),
                Message::tool_response("call_1".to_string(), "ok".to_string()),
            ],
            ..Default::default()
        };

        let payload = map_request_items_to_responses(
            &request,
            ResponsesItemAdapterOptions {
                include_structured_history_in_input: true,
            },
        )
        .expect("request item mapping should succeed");

        assert_eq!(payload.input[0]["role"], "user");
        assert_eq!(payload.input[1]["type"], "function_call");
        assert_eq!(payload.input[1]["call_id"], "call_1");
        assert_eq!(payload.input[2]["type"], "function_call_output");
        assert_eq!(payload.input[2]["output"], "ok");
    }

    #[test]
    fn include_mapping_uses_rig_known_values_and_preserves_open_strings() {
        let include = vec![
            " output_text.annotations ".to_string(),
            "reasoning.encrypted_content".to_string(),
            "output_text.annotations".to_string(),
            "file_search_call.results".to_string(),
        ];

        let mapped =
            map_include_fields(Some(include.as_slice()), true).expect("include should exist");

        assert_eq!(
            mapped.as_array(),
            Some(&vec![
                json!("output_text.annotations"),
                json!("reasoning.encrypted_content"),
                json!("file_search_call.results"),
            ])
        );
    }

    #[test]
    fn reasoning_encryption_is_preserved_in_request_item_boundary() {
        let request = LLMRequest {
            messages: vec![
                Message::assistant("answer".to_string()).with_reasoning_details(Some(vec![
                    json!({
                        "type": "reasoning",
                        "id": "rs_1",
                        "encrypted_content": "opaque_reasoning",
                    }),
                ])),
            ],
            ..Default::default()
        };

        let payload = map_request_items_to_responses(
            &request,
            ResponsesItemAdapterOptions {
                include_structured_history_in_input: true,
            },
        )
        .expect("request item mapping should succeed");

        assert_eq!(payload.input[0]["type"], "reasoning");
        assert_eq!(payload.input[0]["encrypted_content"], "opaque_reasoning");
    }

    #[test]
    fn adapter_overlay_helpers_retain_custom_boundary_fields() {
        let mut input = vec![json!({
            "role": "assistant",
            "phase": AssistantPhase::Commentary.as_str(),
            "content": [{"type": "output_text", "text": "thinking"}]
        })];
        strip_assistant_phase_overlay(&mut input);
        assert!(input[0].get("phase").is_none());

        let params = rig_supported_state_parameters(Some("resp_123"), Some(false));
        let mut request = json!({"model": "gpt-5", "input": []});
        super::merge_rig_supported_state(&mut request, params);
        apply_prompt_cache_overlay(
            &mut request,
            PromptCacheOverlay {
                prompt_cache_key: Some("vtcode:session"),
                include_prompt_cache_retention: true,
                is_responses_api_model: true,
                prompt_cache_retention: Some("24h"),
            },
        );

        assert_eq!(
            request.get("previous_response_id").and_then(Value::as_str),
            Some("resp_123")
        );
        assert_eq!(request.get("store").and_then(Value::as_bool), Some(false));
        assert_eq!(
            request.get("prompt_cache_key").and_then(Value::as_str),
            Some("vtcode:session")
        );
        assert_eq!(
            request
                .get("prompt_cache_retention")
                .and_then(Value::as_str),
            Some("24h")
        );
    }
}
