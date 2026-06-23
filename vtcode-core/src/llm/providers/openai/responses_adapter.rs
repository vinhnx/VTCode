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

        let parsed = match serde_json::from_str::<RigResponsesStreamingChunk>(trimmed) {
            Ok(parsed) => parsed,
            Err(err) if is_known_rig_stream_event(&raw_payload) => {
                return Err(StreamAssemblyError::InvalidPayload(err.to_string())
                    .into_llm_error(provider_name));
            }
            Err(_) => return adapt_overlay_payload(raw_payload),
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
                    _ => adapt_overlay_payload(raw_payload),
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

fn adapt_overlay_payload(payload: Value) -> Result<ResponsesStreamEvent, LLMError> {
    match payload.get("type").and_then(Value::as_str) {
        // VTCode overlay: Rig 0.39 models reasoning summary deltas, while some
        // OpenAI-compatible endpoints still emit reasoning_text deltas.
        Some("response.reasoning_text.delta") | Some("response.reasoning_content.delta") => {
            Ok(ResponsesStreamEvent::ReasoningDelta {
                delta: payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        }
        Some("response.function_call_arguments.done") => Ok(ResponsesStreamEvent::Unknown),
        _ => Ok(ResponsesStreamEvent::Unknown),
    }
}

fn is_known_rig_stream_event(payload: &Value) -> bool {
    matches!(
        payload.get("type").and_then(Value::as_str),
        Some(
            "response.created"
                | "response.in_progress"
                | "response.completed"
                | "response.failed"
                | "response.incomplete"
                | "response.output_item.added"
                | "response.output_item.done"
                | "response.content_part.added"
                | "response.content_part.done"
                | "response.output_text.delta"
                | "response.output_text.done"
                | "response.refusal.delta"
                | "response.refusal.done"
                | "response.function_call_arguments.delta"
                | "response.function_call_arguments.done"
                | "response.reasoning_summary_part.added"
                | "response.reasoning_summary_part.done"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning_summary_text.done"
        )
    )
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
