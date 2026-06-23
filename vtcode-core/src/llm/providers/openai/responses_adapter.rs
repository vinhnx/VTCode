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
use crate::llm::providers::shared::StreamAssemblyError;
use rig::providers::openai::responses_api::Output as RigResponsesOutput;
use rig::providers::openai::responses_api::streaming::{
    ItemChunkKind as RigResponsesItemChunkKind, ResponseChunkKind as RigResponsesChunkKind,
    StreamingCompletionChunk as RigResponsesStreamingChunk,
};
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResponsesStreamEvent {
    Lifecycle {
        kind: ResponsesLifecycleEvent,
    },
    TextDelta {
        delta: String,
    },
    RefusalDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    FunctionCallNameDelta {
        call_id: String,
        item_id: Option<String>,
        name: String,
        output_index: Option<usize>,
    },
    FunctionCallArgumentsDelta {
        call_id: String,
        item_id: Option<String>,
        delta: String,
        output_index: Option<usize>,
    },
    CompletedToolCall {
        call_id: String,
        item_id: Option<String>,
        name: String,
        arguments: String,
        output_index: Option<usize>,
    },
    CompletedResponse {
        response: Value,
    },
    Error {
        message: String,
    },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponsesStreamEventPolicy {
    RigSupportedTyped,
    VtcodeOverlayConversion,
    DocumentedStatusMarkerNoop,
    DocumentedValueBearingRigGap,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponsesLifecycleEvent {
    Created,
    InProgress,
}

pub(crate) struct ResponsesStreamAdapter;

impl ResponsesStreamAdapter {
    pub(crate) fn parse_sse_data(data: &str) -> Result<ResponsesStreamEvent, LLMError> {
        Self::parse_sse_data_for_provider("OpenAI", data)
    }

    pub(crate) fn parse_sse_data_for_provider(
        provider_name: &str,
        data: &str,
    ) -> Result<ResponsesStreamEvent, LLMError> {
        let trimmed = data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return Ok(ResponsesStreamEvent::Unknown);
        }

        let raw_payload: Value = serde_json::from_str(trimmed).map_err(|err| {
            StreamAssemblyError::InvalidPayload(err.to_string()).into_llm_error(provider_name)
        })?;

        if raw_payload.get("type").and_then(Value::as_str) == Some("error") {
            return Ok(ResponsesStreamEvent::Error {
                message: response_error_message(&raw_payload)
                    .unwrap_or_else(|| "Unknown error from Responses API".to_string()),
            });
        }

        let policy = response_stream_event_policy(&raw_payload)
            .map_err(|err| err.into_llm_error(provider_name))?;

        let parsed = match serde_json::from_str::<RigResponsesStreamingChunk>(trimmed) {
            Ok(parsed) => parsed,
            Err(err) if policy == ResponsesStreamEventPolicy::RigSupportedTyped => {
                return Err(StreamAssemblyError::InvalidPayload(err.to_string())
                    .into_llm_error(provider_name));
            }
            Err(_) => return adapt_policy_payload(provider_name, raw_payload),
        };

        Self::adapt_rig_chunk(provider_name, parsed, raw_payload)
    }

    pub(crate) fn parse_payload_for_provider(
        provider_name: &str,
        payload: Value,
    ) -> Result<ResponsesStreamEvent, LLMError> {
        let data = serde_json::to_string(&payload).map_err(|err| {
            StreamAssemblyError::InvalidPayload(err.to_string()).into_llm_error(provider_name)
        })?;
        Self::parse_sse_data_for_provider(provider_name, &data)
    }

    fn adapt_rig_chunk(
        provider_name: &str,
        chunk: RigResponsesStreamingChunk,
        raw_payload: Value,
    ) -> Result<ResponsesStreamEvent, LLMError> {
        match chunk {
            RigResponsesStreamingChunk::Response(response_chunk) => {
                let kind = response_chunk.kind;
                match kind {
                    RigResponsesChunkKind::ResponseCreated => Ok(ResponsesStreamEvent::Lifecycle {
                        kind: ResponsesLifecycleEvent::Created,
                    }),
                    RigResponsesChunkKind::ResponseInProgress => {
                        Ok(ResponsesStreamEvent::Lifecycle {
                            kind: ResponsesLifecycleEvent::InProgress,
                        })
                    }
                    RigResponsesChunkKind::ResponseCompleted => {
                        Ok(ResponsesStreamEvent::CompletedResponse {
                            response: raw_payload.get("response").cloned().unwrap_or(Value::Null),
                        })
                    }
                    RigResponsesChunkKind::ResponseFailed
                    | RigResponsesChunkKind::ResponseIncomplete => {
                        Ok(ResponsesStreamEvent::Error {
                            message: response_error_message(&raw_payload)
                                .unwrap_or_else(|| "Unknown error from Responses API".to_string()),
                        })
                    }
                }
            }
            RigResponsesStreamingChunk::Delta(item_chunk) => {
                let output_index = usize::try_from(item_chunk.output_index).ok();
                let item_id = item_chunk.item_id;
                match item_chunk.data {
                    RigResponsesItemChunkKind::OutputTextDelta(delta) => {
                        Ok(ResponsesStreamEvent::TextDelta { delta: delta.delta })
                    }
                    RigResponsesItemChunkKind::RefusalDelta(delta) => {
                        Ok(ResponsesStreamEvent::RefusalDelta { delta: delta.delta })
                    }
                    RigResponsesItemChunkKind::ReasoningSummaryTextDelta(delta) => {
                        Ok(ResponsesStreamEvent::ReasoningDelta { delta: delta.delta })
                    }
                    RigResponsesItemChunkKind::OutputItemAdded(output)
                    | RigResponsesItemChunkKind::OutputItemDone(output) => {
                        adapt_output_item(provider_name, output.item, output_index)
                    }
                    RigResponsesItemChunkKind::FunctionCallArgsDelta(delta) => {
                        let item_id = item_id.or_else(|| {
                            raw_payload
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned)
                        });
                        let call_id = raw_payload
                            .get("call_id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                            .or_else(|| item_id.clone())
                            .unwrap_or_default();

                        Ok(ResponsesStreamEvent::FunctionCallArgumentsDelta {
                            call_id,
                            item_id,
                            delta: delta.delta,
                            output_index,
                        })
                    }
                    _ => adapt_policy_payload(provider_name, raw_payload),
                }
            }
        }
    }
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

pub(crate) fn rig_chatgpt_default_parameters() -> RigResponsesAdditionalParameters {
    RigResponsesAdditionalParameters {
        include: Some(vec![RigResponsesInclude::ReasoningEncryptedContent]),
        store: Some(false),
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

pub(crate) fn clear_rig_chatgpt_unsupported_parameters(openai_request: &mut Value) {
    let Some(request) = openai_request.as_object_mut() else {
        return;
    };

    for field in [
        "background",
        "max_output_tokens",
        "metadata",
        "output_types",
        "parallel_tool_calls",
        "parallel_tool_config",
        "prompt_cache_key",
        "prompt_cache_retention",
        "sampling_parameters",
        "service_tier",
        "temperature",
        "text",
        "top_p",
        "user",
    ] {
        request.remove(field);
    }
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

fn adapt_output_item(
    provider_name: &str,
    item: RigResponsesOutput,
    output_index: Option<usize>,
) -> Result<ResponsesStreamEvent, LLMError> {
    match item {
        RigResponsesOutput::FunctionCall(function_call) => {
            let arguments = serde_json::to_string(&function_call.arguments).map_err(|err| {
                StreamAssemblyError::InvalidPayload(err.to_string()).into_llm_error(provider_name)
            })?;
            let item_id = Some(function_call.id);
            let call_id = function_call.call_id;
            if function_call.status == rig::providers::openai::responses_api::ToolStatus::Completed
            {
                Ok(ResponsesStreamEvent::CompletedToolCall {
                    call_id,
                    item_id,
                    name: function_call.name,
                    arguments,
                    output_index,
                })
            } else {
                Ok(ResponsesStreamEvent::FunctionCallNameDelta {
                    call_id,
                    item_id,
                    name: function_call.name,
                    output_index,
                })
            }
        }
        _ => Ok(ResponsesStreamEvent::Unknown),
    }
}

fn adapt_policy_payload(
    provider_name: &str,
    payload: Value,
) -> Result<ResponsesStreamEvent, LLMError> {
    let policy =
        response_stream_event_policy(&payload).map_err(|err| err.into_llm_error(provider_name))?;

    match policy {
        ResponsesStreamEventPolicy::VtcodeOverlayConversion => {
            adapt_overlay_conversion(provider_name, &payload)
        }
        ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop => Ok(ResponsesStreamEvent::Unknown),
        ResponsesStreamEventPolicy::DocumentedValueBearingRigGap => {
            // These documented events carry provider-side values, but this
            // adapter has no streaming runtime surface for provider-hosted MCP,
            // code-interpreter, image, annotation, or custom-tool partials.
            // Keep them separate from status no-ops; completed response parsing
            // remains responsible for durable provider output items.
            Ok(ResponsesStreamEvent::Unknown)
        }
        ResponsesStreamEventPolicy::Unsupported => {
            Err(StreamAssemblyError::InvalidPayload(format!(
                "unsupported Responses stream event type `{}`",
                payload
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing>")
            ))
            .into_llm_error(provider_name))
        }
        ResponsesStreamEventPolicy::RigSupportedTyped => {
            Err(StreamAssemblyError::InvalidPayload(format!(
                "Rig-supported Responses stream event `{}` reached overlay fallback",
                payload
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing>")
            ))
            .into_llm_error(provider_name))
        }
    }
}

fn adapt_overlay_conversion(
    provider_name: &str,
    payload: &Value,
) -> Result<ResponsesStreamEvent, LLMError> {
    match payload.get("type").and_then(Value::as_str) {
        // VTCode overlay: Rig 0.39 models reasoning summary deltas, while some
        // OpenAI-compatible endpoints still emit reasoning_text deltas.
        Some("response.reasoning_text.delta") | Some("response.reasoning_content.delta") => {
            Ok(ResponsesStreamEvent::ReasoningDelta {
                delta: required_string_field(provider_name, payload, "delta")?,
            })
        }
        Some("response.reasoning_text.done") => {
            let text = optional_string_field(provider_name, payload, "text")?;
            let delta = optional_string_field(provider_name, payload, "delta")?;
            if let Some(text) = text.or(delta) {
                Ok(ResponsesStreamEvent::ReasoningDelta { delta: text })
            } else {
                Ok(ResponsesStreamEvent::Unknown)
            }
        }
        _ => Err(
            StreamAssemblyError::InvalidPayload("unsupported overlay conversion".to_string())
                .into_llm_error(provider_name),
        ),
    }
}

fn response_stream_event_policy(
    payload: &Value,
) -> Result<ResponsesStreamEventPolicy, StreamAssemblyError> {
    let Some(event_type) = payload.get("type") else {
        return Err(StreamAssemblyError::InvalidPayload(
            "missing Responses stream event type".to_string(),
        ));
    };
    let Some(event_type) = event_type.as_str() else {
        return Err(StreamAssemblyError::InvalidPayload(
            "Responses stream event type must be a string".to_string(),
        ));
    };

    Ok(response_stream_event_policy_for_type(event_type))
}

fn response_stream_event_policy_for_type(event_type: &str) -> ResponsesStreamEventPolicy {
    match event_type {
        "response.created"
        | "response.in_progress"
        | "response.completed"
        | "response.failed"
        | "response.incomplete"
        | "response.output_item.added"
        | "response.output_item.done"
        | "response.output_text.delta"
        | "response.refusal.delta"
        | "response.function_call_arguments.delta"
        | "response.reasoning_summary_text.delta" => ResponsesStreamEventPolicy::RigSupportedTyped,
        "response.reasoning_text.delta"
        | "response.reasoning_text.done"
        | "response.reasoning_content.delta" => ResponsesStreamEventPolicy::VtcodeOverlayConversion,
        "response.queued"
        | "response.content_part.added"
        | "response.content_part.done"
        | "response.output_text.done"
        | "response.refusal.done"
        | "response.function_call_arguments.done"
        | "response.reasoning_summary_part.added"
        | "response.reasoning_summary_part.done"
        | "response.reasoning_summary_text.done"
        | "response.file_search_call.in_progress"
        | "response.file_search_call.searching"
        | "response.file_search_call.completed"
        | "response.web_search_call.in_progress"
        | "response.web_search_call.searching"
        | "response.web_search_call.completed"
        | "response.image_generation_call.in_progress"
        | "response.image_generation_call.generating"
        | "response.image_generation_call.completed"
        | "response.mcp_call.in_progress"
        | "response.mcp_call.completed"
        | "response.mcp_list_tools.in_progress"
        | "response.mcp_list_tools.completed"
        | "response.code_interpreter_call.in_progress"
        | "response.code_interpreter_call.interpreting"
        | "response.code_interpreter_call.completed" => {
            ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop
        }
        "response.code_interpreter_call_code.delta"
        | "response.code_interpreter_call_code.done"
        | "response.mcp_call_arguments.delta"
        | "response.mcp_call_arguments.done"
        | "response.image_generation_call.partial_image"
        | "response.custom_tool_call_input.delta"
        | "response.custom_tool_call_input.done"
        | "response.output_text.annotation.added" => {
            ResponsesStreamEventPolicy::DocumentedValueBearingRigGap
        }
        _ => ResponsesStreamEventPolicy::Unsupported,
    }
}

fn required_string_field(
    provider_name: &str,
    payload: &Value,
    field: &'static str,
) -> Result<String, LLMError> {
    match payload.get(field) {
        Some(value) => value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
            StreamAssemblyError::InvalidPayload(format!(
                "field `{field}` in stream payload must be a string"
            ))
            .into_llm_error(provider_name)
        }),
        None => Err(StreamAssemblyError::MissingField(field).into_llm_error(provider_name)),
    }
}

fn optional_string_field(
    provider_name: &str,
    payload: &Value,
    field: &'static str,
) -> Result<Option<String>, LLMError> {
    match payload.get(field) {
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| {
                StreamAssemblyError::InvalidPayload(format!(
                    "field `{field}` in stream payload must be a string"
                ))
                .into_llm_error(provider_name)
            }),
        None => Ok(None),
    }
}

fn response_error_message(payload: &Value) -> Option<String> {
    payload
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        PromptCacheOverlay, ResponsesItemAdapterOptions, ResponsesLifecycleEvent,
        ResponsesStreamAdapter, ResponsesStreamEvent, apply_prompt_cache_overlay,
        clear_rig_chatgpt_unsupported_parameters, map_include_fields,
        map_request_items_to_responses, rig_chatgpt_default_parameters,
        rig_supported_state_parameters, strip_assistant_phase_overlay,
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

    #[test]
    fn chatgpt_defaults_are_modelled_with_rig_responses_parameters() {
        let params = rig_chatgpt_default_parameters();
        let encoded = serde_json::to_value(params).expect("params should serialize");

        assert_eq!(encoded.get("store").and_then(Value::as_bool), Some(false));
        assert_eq!(
            encoded.get("include").and_then(Value::as_array),
            Some(&vec![json!("reasoning.encrypted_content")])
        );
        assert!(encoded.get("previous_response_id").is_none());
    }

    #[test]
    fn chatgpt_unsupported_parameter_clear_matches_rig_boundary() {
        let mut request = json!({
            "model": "gpt-5.3-codex",
            "input": [],
            "stream": true,
            "background": true,
            "max_output_tokens": 123,
            "metadata": {"turn": "1"},
            "output_types": ["message"],
            "parallel_tool_calls": true,
            "parallel_tool_config": {"max_parallel_tool_calls": 2},
            "prompt_cache_key": "session",
            "prompt_cache_retention": "24h",
            "sampling_parameters": {"temperature": 0.2},
            "service_tier": "priority",
            "temperature": 0.2,
            "text": {"verbosity": "low"},
            "top_p": 0.9,
            "user": "tester"
        });

        clear_rig_chatgpt_unsupported_parameters(&mut request);

        for field in [
            "background",
            "max_output_tokens",
            "metadata",
            "output_types",
            "parallel_tool_calls",
            "parallel_tool_config",
            "prompt_cache_key",
            "prompt_cache_retention",
            "sampling_parameters",
            "service_tier",
            "temperature",
            "text",
            "top_p",
            "user",
        ] {
            assert!(request.get(field).is_none(), "{field} should be cleared");
        }
    }

    fn event_fixture(payload: Value) -> ResponsesStreamEvent {
        ResponsesStreamAdapter::parse_sse_data(&payload.to_string()).expect("fixture should parse")
    }

    #[test]
    fn stream_event_policy_separates_rig_overlay_noop_value_and_unsupported() {
        use super::ResponsesStreamEventPolicy as Policy;

        assert_eq!(
            super::response_stream_event_policy_for_type("response.output_text.delta"),
            Policy::RigSupportedTyped
        );
        assert_eq!(
            super::response_stream_event_policy_for_type("response.reasoning_text.delta"),
            Policy::VtcodeOverlayConversion
        );
        assert_eq!(
            super::response_stream_event_policy_for_type("response.queued"),
            Policy::DocumentedStatusMarkerNoop
        );

        for event_type in [
            "response.code_interpreter_call_code.delta",
            "response.code_interpreter_call_code.done",
            "response.mcp_call_arguments.delta",
            "response.mcp_call_arguments.done",
            "response.image_generation_call.partial_image",
            "response.custom_tool_call_input.delta",
            "response.custom_tool_call_input.done",
            "response.output_text.annotation.added",
            "response.reasoning_text.done",
        ] {
            assert_ne!(
                super::response_stream_event_policy_for_type(event_type),
                Policy::DocumentedStatusMarkerNoop,
                "{event_type} must not be classified as a status-only no-op"
            );
        }

        assert_eq!(
            super::response_stream_event_policy_for_type(
                "response.code_interpreter_call_code.delta"
            ),
            Policy::DocumentedValueBearingRigGap
        );
        assert_eq!(
            super::response_stream_event_policy_for_type("response.not_a_real_event"),
            Policy::Unsupported
        );
    }

    #[test]
    fn documented_status_marker_noop_fixtures_do_not_emit_runtime_events() {
        for (event_type, payload) in [
            (
                "response.queued",
                json!({
                    "type": "response.queued",
                    "sequence_number": 0,
                    "response": {
                        "id": "resp_queued",
                        "status": "queued"
                    }
                }),
            ),
            (
                "response.file_search_call.in_progress",
                json!({
                    "type": "response.file_search_call.in_progress",
                    "sequence_number": 1,
                    "item_id": "fs_1",
                    "output_index": 0
                }),
            ),
            (
                "response.file_search_call.searching",
                json!({
                    "type": "response.file_search_call.searching",
                    "sequence_number": 2,
                    "item_id": "fs_1",
                    "output_index": 0
                }),
            ),
            (
                "response.file_search_call.completed",
                json!({
                    "type": "response.file_search_call.completed",
                    "sequence_number": 3,
                    "item_id": "fs_1",
                    "output_index": 0
                }),
            ),
            (
                "response.web_search_call.in_progress",
                json!({
                    "type": "response.web_search_call.in_progress",
                    "sequence_number": 4,
                    "item_id": "ws_1",
                    "output_index": 1
                }),
            ),
            (
                "response.web_search_call.searching",
                json!({
                    "type": "response.web_search_call.searching",
                    "sequence_number": 5,
                    "item_id": "ws_1",
                    "output_index": 1
                }),
            ),
            (
                "response.web_search_call.completed",
                json!({
                    "type": "response.web_search_call.completed",
                    "sequence_number": 6,
                    "item_id": "ws_1",
                    "output_index": 1
                }),
            ),
            (
                "response.image_generation_call.in_progress",
                json!({
                    "type": "response.image_generation_call.in_progress",
                    "sequence_number": 7,
                    "item_id": "ig_1",
                    "output_index": 2
                }),
            ),
            (
                "response.image_generation_call.generating",
                json!({
                    "type": "response.image_generation_call.generating",
                    "sequence_number": 8,
                    "item_id": "ig_1",
                    "output_index": 2
                }),
            ),
            (
                "response.image_generation_call.completed",
                json!({
                    "type": "response.image_generation_call.completed",
                    "sequence_number": 9,
                    "item_id": "ig_1",
                    "output_index": 2
                }),
            ),
            (
                "response.mcp_call.in_progress",
                json!({
                    "type": "response.mcp_call.in_progress",
                    "sequence_number": 10,
                    "item_id": "mcp_1",
                    "output_index": 3
                }),
            ),
            (
                "response.mcp_call.completed",
                json!({
                    "type": "response.mcp_call.completed",
                    "sequence_number": 11,
                    "item_id": "mcp_1",
                    "output_index": 3
                }),
            ),
            (
                "response.mcp_list_tools.in_progress",
                json!({
                    "type": "response.mcp_list_tools.in_progress",
                    "sequence_number": 12,
                    "item_id": "mcp_tools_1",
                    "output_index": 4
                }),
            ),
            (
                "response.mcp_list_tools.completed",
                json!({
                    "type": "response.mcp_list_tools.completed",
                    "sequence_number": 13,
                    "item_id": "mcp_tools_1",
                    "output_index": 4
                }),
            ),
            (
                "response.code_interpreter_call.in_progress",
                json!({
                    "type": "response.code_interpreter_call.in_progress",
                    "sequence_number": 14,
                    "item_id": "ci_1",
                    "output_index": 5
                }),
            ),
            (
                "response.code_interpreter_call.interpreting",
                json!({
                    "type": "response.code_interpreter_call.interpreting",
                    "sequence_number": 15,
                    "item_id": "ci_1",
                    "output_index": 5
                }),
            ),
            (
                "response.code_interpreter_call.completed",
                json!({
                    "type": "response.code_interpreter_call.completed",
                    "sequence_number": 16,
                    "item_id": "ci_1",
                    "output_index": 5
                }),
            ),
        ] {
            assert_eq!(
                event_fixture(payload),
                ResponsesStreamEvent::Unknown,
                "{event_type} should be an explicit status/marker no-op"
            );
        }
    }

    #[test]
    fn documented_overlay_and_noop_rig_failures_do_not_become_invalid_payload() {
        assert_eq!(
            event_fixture(json!({
                "type": "response.reasoning_text.delta",
                "sequence_number": 1,
                "item_id": "rs_1",
                "output_index": 0,
                "delta": "private chain summary"
            })),
            ResponsesStreamEvent::ReasoningDelta {
                delta: "private chain summary".to_string()
            }
        );

        assert_eq!(
            event_fixture(json!({
                "type": "response.reasoning_text.done",
                "sequence_number": 2,
                "item_id": "rs_1",
                "output_index": 0
            })),
            ResponsesStreamEvent::Unknown
        );

        assert_eq!(
            event_fixture(json!({
                "type": "response.reasoning_text.done",
                "sequence_number": 3,
                "item_id": "rs_1",
                "output_index": 0,
                "text": "final reasoning text"
            })),
            ResponsesStreamEvent::ReasoningDelta {
                delta: "final reasoning text".to_string()
            }
        );
    }

    #[test]
    fn overlay_conversion_shapes_remain_strict() {
        assert!(
            ResponsesStreamAdapter::parse_sse_data(
                &json!({
                    "type": "response.reasoning_text.delta",
                    "sequence_number": 1,
                    "delta": 42
                })
                .to_string()
            )
            .is_err()
        );

        assert!(
            ResponsesStreamAdapter::parse_sse_data(
                &json!({
                    "type": "response.not_a_real_event",
                    "sequence_number": 1
                })
                .to_string()
            )
            .is_err()
        );
    }

    #[test]
    fn stream_adapter_parses_lifecycle_text_refusal_reasoning_and_usage_fixtures() {
        assert_eq!(
            event_fixture(json!({
                "type": "response.created",
                "sequence_number": 0,
                "response": {
                    "id": "resp_1",
                    "object": "response",
                    "created_at": 1,
                    "status": "in_progress",
                    "error": null,
                    "incomplete_details": null,
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": "gpt-5",
                    "usage": null,
                    "output": [],
                    "tools": []
                }
            })),
            ResponsesStreamEvent::Lifecycle {
                kind: ResponsesLifecycleEvent::Created
            }
        );

        assert_eq!(
            event_fixture(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 1,
                "delta": "hello"
            })),
            ResponsesStreamEvent::TextDelta {
                delta: "hello".to_string()
            }
        );

        assert_eq!(
            event_fixture(json!({
                "type": "response.refusal.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 2,
                "delta": "no"
            })),
            ResponsesStreamEvent::RefusalDelta {
                delta: "no".to_string()
            }
        );

        assert_eq!(
            event_fixture(json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": "rs_1",
                "output_index": 1,
                "summary_index": 0,
                "sequence_number": 3,
                "delta": "thinking"
            })),
            ResponsesStreamEvent::ReasoningDelta {
                delta: "thinking".to_string()
            }
        );

        let completed = event_fixture(json!({
            "type": "response.completed",
            "sequence_number": 4,
            "response": {
                "id": "resp_1",
                "object": "response",
                "created_at": 1,
                "status": "completed",
                "error": null,
                "incomplete_details": null,
                "instructions": null,
                "max_output_tokens": null,
                "model": "gpt-5",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5,
                    "total_tokens": 15
                },
                "output": [],
                "tools": [],
                "vtcode_overlay": "preserved"
            }
        }));

        let ResponsesStreamEvent::CompletedResponse { response } = completed else {
            panic!("expected completed response event");
        };
        assert_eq!(response["usage"]["input_tokens"], 10);
        assert_eq!(response["vtcode_overlay"], "preserved");
    }

    #[test]
    fn stream_adapter_parses_function_call_deltas_completed_tool_call_and_error_fixtures() {
        assert_eq!(
            event_fixture(json!({
                "type": "response.output_item.added",
                "item_id": "fc_1",
                "output_index": 0,
                "sequence_number": 1,
                "item": {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "search_workspace",
                    "arguments": "",
                    "status": "in_progress"
                }
            })),
            ResponsesStreamEvent::FunctionCallNameDelta {
                call_id: "call_1".to_string(),
                item_id: Some("fc_1".to_string()),
                name: "search_workspace".to_string(),
                output_index: Some(0)
            }
        );

        assert_eq!(
            event_fixture(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 2,
                "delta": "{\"query\":\"vtcode\"}"
            })),
            ResponsesStreamEvent::FunctionCallArgumentsDelta {
                call_id: "fc_1".to_string(),
                item_id: Some("fc_1".to_string()),
                delta: "{\"query\":\"vtcode\"}".to_string(),
                output_index: Some(0)
            }
        );

        assert_eq!(
            event_fixture(json!({
                "type": "response.output_item.done",
                "item_id": "fc_1",
                "output_index": 0,
                "sequence_number": 3,
                "item": {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "search_workspace",
                    "arguments": "{\"query\":\"vtcode\"}",
                    "status": "completed"
                }
            })),
            ResponsesStreamEvent::CompletedToolCall {
                call_id: "call_1".to_string(),
                item_id: Some("fc_1".to_string()),
                name: "search_workspace".to_string(),
                arguments: "{\"query\":\"vtcode\"}".to_string(),
                output_index: Some(0)
            }
        );

        assert_eq!(
            event_fixture(json!({
                "type": "error",
                "error": {"message": "rate limited"}
            })),
            ResponsesStreamEvent::Error {
                message: "rate limited".to_string()
            }
        );
    }
}
