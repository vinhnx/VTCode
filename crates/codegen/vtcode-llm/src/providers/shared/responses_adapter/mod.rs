//! Shared Responses stream adapter boundary.
//!
//! Core keeps this adapter because the runtime consumes provider-agnostic
//! `NormalizedStreamEvent`s for OpenAI Responses-style streams used outside
//! the native OpenAI provider. Request construction, HTTP clients, replay, and
//! ChatGPT-specific policy live in `vtcode-llm::providers::openai`.

use crate::provider::{LLMError, ReasoningSource};
use crate::providers::shared::StreamAssemblyError;
use rig::providers::openai::responses_api::Output as RigResponsesOutput;
use rig::providers::openai::responses_api::streaming::{
    ItemChunkKind as RigResponsesItemChunkKind, ResponseChunkKind as RigResponsesChunkKind,
    StreamingCompletionChunk as RigResponsesStreamingChunk,
};
use serde::Deserialize;
use serde_json::Value;

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
        source: ReasoningSource,
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
    ProviderValueBearingRigGap {
        event_type: String,
        item_id: Option<String>,
        call_id: Option<String>,
        output_index: Option<usize>,
        sequence_number: Option<u64>,
        payload: Value,
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

#[derive(Debug, Deserialize)]
struct ResponsesEventHeader<'a> {
    #[serde(rename = "type")]
    event_type: Option<&'a str>,
    #[serde(borrow)]
    call_id: Option<&'a str>,
}

pub(crate) struct ResponsesStreamAdapter;

impl ResponsesStreamAdapter {
    #[cfg(test)]
    fn parse_sse_data(data: &str) -> Result<ResponsesStreamEvent, LLMError> {
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

        let header: ResponsesEventHeader<'_> = serde_json::from_str(trimmed)
            .map_err(|err| StreamAssemblyError::InvalidPayload(err.to_string()).into_llm_error(provider_name))?;
        let event_type = header.event_type.ok_or_else(|| {
            StreamAssemblyError::InvalidPayload("missing Responses stream event type".to_string())
                .into_llm_error(provider_name)
        })?;

        if event_type == "error" {
            let raw_payload = parse_raw_payload(provider_name, trimmed)?;
            return Ok(ResponsesStreamEvent::Error {
                message: response_error_message(&raw_payload)
                    .unwrap_or_else(|| "Unknown error from Responses API".to_string()),
            });
        }

        let policy = response_stream_event_policy_for_type(event_type);
        if policy == ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop {
            return Ok(ResponsesStreamEvent::Unknown);
        }

        let parsed = match serde_json::from_str::<RigResponsesStreamingChunk>(trimmed) {
            Ok(parsed) => parsed,
            Err(err) if policy == ResponsesStreamEventPolicy::RigSupportedTyped => {
                let raw_payload = parse_raw_payload(provider_name, trimmed)?;
                if let Some(event) = adapt_rig_supported_envelope_fallback(&raw_payload) {
                    return Ok(event);
                }
                return Err(StreamAssemblyError::InvalidPayload(err.to_string()).into_llm_error(provider_name));
            }
            Err(_) => return adapt_policy_payload(provider_name, parse_raw_payload(provider_name, trimmed)?),
        };

        Self::adapt_rig_chunk(provider_name, parsed, header.call_id, trimmed)
    }

    fn adapt_rig_chunk(
        provider_name: &str,
        chunk: RigResponsesStreamingChunk,
        call_id: Option<&str>,
        raw_data: &str,
    ) -> Result<ResponsesStreamEvent, LLMError> {
        match chunk {
            RigResponsesStreamingChunk::Response(response_chunk) => {
                let kind = response_chunk.kind;
                match kind {
                    RigResponsesChunkKind::ResponseCreated => {
                        Ok(ResponsesStreamEvent::Lifecycle { kind: ResponsesLifecycleEvent::Created })
                    }
                    RigResponsesChunkKind::ResponseInProgress => {
                        Ok(ResponsesStreamEvent::Lifecycle { kind: ResponsesLifecycleEvent::InProgress })
                    }
                    RigResponsesChunkKind::ResponseCompleted => {
                        let raw_payload = parse_raw_payload(provider_name, raw_data)?;
                        Ok(ResponsesStreamEvent::CompletedResponse {
                            response: raw_payload.get("response").cloned().unwrap_or(Value::Null),
                        })
                    }
                    RigResponsesChunkKind::ResponseFailed | RigResponsesChunkKind::ResponseIncomplete => {
                        let raw_payload = parse_raw_payload(provider_name, raw_data)?;
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
                        Ok(ResponsesStreamEvent::ReasoningDelta {
                            delta: delta.delta,
                            source: ReasoningSource::ProviderSummary,
                        })
                    }
                    RigResponsesItemChunkKind::ReasoningTextDelta(delta) => Ok(ResponsesStreamEvent::ReasoningDelta {
                        delta: delta.delta,
                        source: ReasoningSource::Continuation,
                    }),
                    RigResponsesItemChunkKind::OutputItemAdded(output) => {
                        adapt_output_item(provider_name, output.item, output_index, true, raw_data)
                    }
                    RigResponsesItemChunkKind::OutputItemDone(output) => {
                        adapt_output_item(provider_name, output.item, output_index, false, raw_data)
                    }
                    RigResponsesItemChunkKind::FunctionCallArgsDelta(delta) => {
                        let call_id = call_id.map(ToOwned::to_owned).or_else(|| item_id.clone()).unwrap_or_default();

                        Ok(ResponsesStreamEvent::FunctionCallArgumentsDelta {
                            call_id,
                            item_id,
                            delta: delta.delta,
                            output_index,
                        })
                    }
                    _ => adapt_policy_payload(provider_name, parse_raw_payload(provider_name, raw_data)?),
                }
            }
        }
    }
}

fn parse_raw_payload(provider_name: &str, data: &str) -> Result<Value, LLMError> {
    serde_json::from_str(data)
        .map_err(|err| StreamAssemblyError::InvalidPayload(err.to_string()).into_llm_error(provider_name))
}

fn adapt_output_item(
    provider_name: &str,
    item: RigResponsesOutput,
    output_index: Option<usize>,
    emit_completed_arguments: bool,
    raw_data: &str,
) -> Result<ResponsesStreamEvent, LLMError> {
    match item {
        RigResponsesOutput::FunctionCall(function_call) => {
            let arguments = serde_json::to_string(&function_call.arguments)
                .map_err(|err| StreamAssemblyError::InvalidPayload(err.to_string()).into_llm_error(provider_name))?;
            let item_id = Some(function_call.id);
            let call_id = function_call.call_id;
            if function_call.status == rig::providers::openai::responses_api::ToolStatus::Completed {
                let arguments = if emit_completed_arguments {
                    arguments
                } else {
                    String::new()
                };

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
        RigResponsesOutput::Unknown(_) => {
            let raw_payload = parse_raw_payload(provider_name, raw_data)?;
            adapt_rig_gap_output_item_envelope(&raw_payload).ok_or_else(|| {
                StreamAssemblyError::InvalidPayload(
                    "Rig returned an unknown Responses output item without eligible raw fallback evidence".to_string(),
                )
                .into_llm_error(provider_name)
            })
        }
        _ => Ok(ResponsesStreamEvent::Unknown),
    }
}

fn adapt_policy_payload(provider_name: &str, payload: Value) -> Result<ResponsesStreamEvent, LLMError> {
    let policy = response_stream_event_policy(&payload).map_err(|err| err.into_llm_error(provider_name))?;

    match policy {
        ResponsesStreamEventPolicy::VtcodeOverlayConversion => adapt_overlay_conversion(provider_name, &payload),
        ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop => Ok(ResponsesStreamEvent::Unknown),
        ResponsesStreamEventPolicy::DocumentedValueBearingRigGap => adapt_value_bearing_rig_gap(payload),
        ResponsesStreamEventPolicy::Unsupported => Err(StreamAssemblyError::InvalidPayload(format!(
            "unsupported Responses stream event type `{}`",
            payload.get("type").and_then(Value::as_str).unwrap_or("<missing>")
        ))
        .into_llm_error(provider_name)),
        ResponsesStreamEventPolicy::RigSupportedTyped => Err(StreamAssemblyError::InvalidPayload(format!(
            "Rig-supported Responses stream event `{}` reached overlay fallback",
            payload.get("type").and_then(Value::as_str).unwrap_or("<missing>")
        ))
        .into_llm_error(provider_name)),
    }
}

fn adapt_value_bearing_rig_gap(payload: Value) -> Result<ResponsesStreamEvent, LLMError> {
    let event_type = payload.get("type").and_then(Value::as_str).unwrap_or_default();

    match event_type {
        "response.code_interpreter_call_code.delta"
        | "response.code_interpreter_call_code.done"
        | "response.mcp_call_arguments.delta"
        | "response.mcp_call_arguments.done"
        | "response.image_generation_call.partial_image"
        | "response.custom_tool_call_input.delta"
        | "response.custom_tool_call_input.done"
        | "response.output_text.annotation.added" => {
            // VTCode has no runtime surface for provider-hosted code execution,
            // provider-side MCP dispatch, partial image rendering, streamed
            // custom tool input, or streamed annotation metadata. Custom tool
            // execution is owned by the final `custom_tool_call` replay path,
            // so partial input cannot be surfaced earlier without changing
            // dispatch and permission ownership. Keep these documented
            // value-bearing payloads distinct from status no-ops and retain
            // reconciliation fields here; downstream processors intentionally
            // ignore this internal event.
            Ok(ResponsesStreamEvent::ProviderValueBearingRigGap {
                event_type: event_type.to_string(),
                item_id: payload.get("item_id").and_then(Value::as_str).map(ToOwned::to_owned),
                call_id: payload.get("call_id").and_then(Value::as_str).map(ToOwned::to_owned),
                output_index: payload
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok()),
                sequence_number: payload.get("sequence_number").and_then(Value::as_u64),
                payload,
            })
        }
        _ => Ok(ResponsesStreamEvent::Unknown),
    }
}

fn adapt_rig_supported_envelope_fallback(payload: &Value) -> Option<ResponsesStreamEvent> {
    let event_type = payload.get("type").and_then(Value::as_str)?;

    match event_type {
        "response.failed" | "response.incomplete" => Some(ResponsesStreamEvent::Error {
            message: response_error_message(payload).unwrap_or_else(|| "Unknown error from Responses API".to_string()),
        }),
        "response.created" => Some(ResponsesStreamEvent::Lifecycle { kind: ResponsesLifecycleEvent::Created }),
        "response.in_progress" => Some(ResponsesStreamEvent::Lifecycle { kind: ResponsesLifecycleEvent::InProgress }),
        "response.completed" => {
            let response = payload.get("response")?;
            // Accept any response.completed where Rig deserialization failed,
            // not just those with Rig-unknown output item types. Rig can fail
            // for missing required fields (sequence_number, object, model, etc.)
            // and the raw response is always preferable to a hard error.
            Some(ResponsesStreamEvent::CompletedResponse { response: response.clone() })
        }
        "response.output_item.added" | "response.output_item.done" => adapt_rig_gap_output_item_envelope(payload),
        _ => None,
    }
}

fn adapt_rig_gap_output_item_envelope(payload: &Value) -> Option<ResponsesStreamEvent> {
    let event_type = payload.get("type").and_then(Value::as_str)?;
    let item = payload.get("item")?;
    if !raw_output_item_is_rig_unknown(item) {
        return None;
    }

    // Rig 0.40 preserves unmodeled nested output items as `Output::Unknown(Value)`,
    // so the item body itself round-trips. This fallback still reconciles the
    // SSE envelope fields (item_id/call_id/output_index/sequence_number) that
    // live on the stream wrapper rather than the item, keeping downstream
    // correlation intact for provider-hosted tool/MCP/code-interpreter events
    // VTCode does not otherwise surface.
    Some(ResponsesStreamEvent::ProviderValueBearingRigGap {
        event_type: event_type.to_string(),
        item_id: payload
            .get("item_id")
            .and_then(Value::as_str)
            .or_else(|| item.get("id").and_then(Value::as_str))
            .map(ToOwned::to_owned),
        call_id: payload
            .get("call_id")
            .and_then(Value::as_str)
            .or_else(|| item.get("call_id").and_then(Value::as_str))
            .map(ToOwned::to_owned),
        output_index: payload
            .get("output_index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok()),
        sequence_number: payload.get("sequence_number").and_then(Value::as_u64),
        payload: payload.clone(),
    })
}

fn raw_output_item_is_rig_unknown(item: &Value) -> bool {
    let Some(item_type) = item.get("type").and_then(Value::as_str) else {
        return false;
    };

    !is_rig_modelled_output_item_type(item_type)
}

fn is_rig_modelled_output_item_type(item_type: &str) -> bool {
    matches!(item_type, "message" | "function_call" | "reasoning")
}

fn adapt_overlay_conversion(provider_name: &str, payload: &Value) -> Result<ResponsesStreamEvent, LLMError> {
    match payload.get("type").and_then(Value::as_str) {
        // VTCode overlay: Rig 0.40 models `reasoning_summary_text.delta` and
        // `reasoning_text.delta` natively (routed through the typed path), but
        // some OpenAI-compatible endpoints emit `reasoning_content.delta`, which
        // Rig does not model. `reasoning_text.delta` is retained defensively.
        Some("response.reasoning_text.delta") | Some("response.reasoning_content.delta") => {
            Ok(ResponsesStreamEvent::ReasoningDelta {
                delta: required_string_field(provider_name, payload, "delta")?,
                source: ReasoningSource::Continuation,
            })
        }
        Some("response.reasoning_text.done") => {
            let text = optional_string_field(provider_name, payload, "text")?;
            let delta = optional_string_field(provider_name, payload, "delta")?;
            if let Some(text) = text.or(delta) {
                Ok(ResponsesStreamEvent::ReasoningDelta { delta: text, source: ReasoningSource::Continuation })
            } else {
                Ok(ResponsesStreamEvent::Unknown)
            }
        }
        _ => Err(StreamAssemblyError::InvalidPayload("unsupported overlay conversion".to_string())
            .into_llm_error(provider_name)),
    }
}

fn response_stream_event_policy(payload: &Value) -> Result<ResponsesStreamEventPolicy, StreamAssemblyError> {
    let Some(event_type) = payload.get("type") else {
        return Err(StreamAssemblyError::InvalidPayload("missing Responses stream event type".to_string()));
    };
    let Some(event_type) = event_type.as_str() else {
        return Err(StreamAssemblyError::InvalidPayload("Responses stream event type must be a string".to_string()));
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
        | "response.reasoning_summary_text.delta"
        | "response.reasoning_text.delta" => ResponsesStreamEventPolicy::RigSupportedTyped,
        "response.reasoning_text.done" | "response.reasoning_content.delta" => {
            ResponsesStreamEventPolicy::VtcodeOverlayConversion
        }
        "response.queued"
        | "keepalive"
        | "response.content_part.added"
        | "response.content_part.done"
        | "response.output_text.done"
        | "response.refusal.done"
        | "response.function_call_arguments.done"
        | "response.reasoning_summary_part.added"
        | "response.reasoning_summary_part.done"
        | "response.reasoning_summary_text.done"
        | "response.reasoning_part.added"
        | "response.reasoning_part.done"
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
        | "response.code_interpreter_call.completed" => ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop,
        "response.code_interpreter_call_code.delta"
        | "response.code_interpreter_call_code.done"
        | "response.mcp_call_arguments.delta"
        | "response.mcp_call_arguments.done"
        | "response.image_generation_call.partial_image"
        | "response.custom_tool_call_input.delta"
        | "response.custom_tool_call_input.done"
        | "response.output_text.annotation.added" => ResponsesStreamEventPolicy::DocumentedValueBearingRigGap,
        _ => ResponsesStreamEventPolicy::Unsupported,
    }
}

fn required_string_field(provider_name: &str, payload: &Value, field: &'static str) -> Result<String, LLMError> {
    match payload.get(field) {
        Some(value) => value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
            StreamAssemblyError::InvalidPayload(format!("field `{field}` in stream payload must be a string"))
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
        Some(value) => value.as_str().map(|value| Some(value.to_string())).ok_or_else(|| {
            StreamAssemblyError::InvalidPayload(format!("field `{field}` in stream payload must be a string"))
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
mod tests;
