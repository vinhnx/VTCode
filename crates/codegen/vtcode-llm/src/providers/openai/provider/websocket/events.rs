//! WebSocket event preparation, replay chaining, and transport.

use super::*;

use super::super::super::errors::is_flex_service_tier_unsupported;
use super::super::super::responses_api::{build_standard_responses_payload, parse_responses_payload};
use super::super::OpenAIProvider;
use super::super::generation::payload_uses_flex_service_tier;
use crate::error_display;
use crate::provider::{LLMError, LLMNormalizedStream, LLMRequest, LLMResponse, NormalizedStreamEvent};
use crate::providers::shared::{ResponsesNormalizedStreamOptions, ResponsesNormalizedStreamProcessor};
use futures::{SinkExt, StreamExt};
use hashbrown::HashMap;
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::OwnedMutexGuard;
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

pub(crate) fn input_is_incremental(last_input: &[Value], current_input: &[Value]) -> bool {
    if current_input.len() < last_input.len() {
        return false;
    }
    canonical_history(current_input).starts_with(&canonical_history(last_input))
}

pub(crate) fn apply_generate_mode(request_obj: &mut Map<String, Value>, warmup: bool) {
    if warmup {
        request_obj.insert("generate".to_string(), Value::Bool(false));
    } else {
        request_obj.remove("generate");
    }
}

pub(crate) fn prepare_websocket_event(
    payload: &Value,
    continuation: Option<&OpenAIResponsesWebSocketContinuationCache>,
    warmup: bool,
) -> Result<PreparedWebSocketEvent, LLMError> {
    let mut request_obj = payload
        .as_object()
        .cloned()
        .ok_or_else(|| format_provider_error("Invalid Responses payload".to_string()))?;

    request_obj.remove("stream");
    request_obj.remove("background");
    request_obj.insert("store".to_string(), Value::Bool(false));
    apply_generate_mode(&mut request_obj, warmup);

    let full_input = request_obj
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format_provider_error("Responses payload missing input".to_string()))?;

    let mut used_previous_response_id = false;
    if !warmup
        && let Some(continuation) = continuation
        && continuation.can_continue_from(&Value::Object(request_obj.clone()))
    {
        if !continuation.response_id.is_empty() {
            request_obj.insert("previous_response_id".to_string(), Value::String(continuation.response_id.clone()));
            let incremental = full_input[continuation.full_input.len()..].to_vec();
            request_obj.insert("input".to_string(), Value::Array(incremental));
            used_previous_response_id = true;
        }
    } else {
        request_obj.remove("previous_response_id");
    }

    let event = Value::Object(
        std::iter::once(("type".to_string(), Value::String("response.create".to_string())))
            .chain(request_obj)
            .collect(),
    );

    Ok(PreparedWebSocketEvent { event, full_input, used_previous_response_id })
}

pub(crate) async fn read_websocket_event(session: &mut OpenAIResponsesWebSocketSession) -> Result<Value, LLMError> {
    while let Some(message) = session.socket.next().await {
        let message = message.map_err(|err| format_network_error(format!("OpenAI WebSocket receive failed: {err}")))?;
        match message {
            Message::Text(text) => {
                let event: Value = serde_json::from_str(text.as_ref())
                    .map_err(|err| format_provider_error(format!("Invalid OpenAI WebSocket event JSON: {err}")))?;
                if event.get("type").and_then(Value::as_str).is_none() {
                    return Err(format_provider_error("OpenAI WebSocket event missing type".to_string()));
                }
                return Ok(event);
            }
            Message::Ping(payload) => {
                session
                    .socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|err| format_network_error(format!("Failed to reply to OpenAI WebSocket ping: {err}")))?;
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                return Err(format_network_error("OpenAI WebSocket connection closed before completion".to_string()));
            }
            _ => return Err(format_provider_error("Unexpected OpenAI WebSocket frame".to_string())),
        }
    }
    Err(format_network_error("OpenAI WebSocket stream ended without completion".to_string()))
}

pub(crate) fn canonical_history(input: &[Value]) -> Vec<Value> {
    pub(crate) fn strip_breakpoints(value: &mut Value) {
        match value {
            Value::Object(object) => {
                object.remove("prompt_cache_breakpoint");
                for value in object.values_mut() {
                    strip_breakpoints(value);
                }
            }
            Value::Array(array) => {
                for value in array {
                    strip_breakpoints(value);
                }
            }
            _ => {}
        }
    }
    let mut input = input.to_vec();
    for item in &mut input {
        strip_breakpoints(item);
    }
    input
}

// Compare output in the same replay shape the next request builder uses. Opaque
// or unrecoverable output starts a full-input chain rather than risking a match.
pub(crate) fn completed_replay_output(response: &Value, model: &str) -> Option<Vec<Value>> {
    let output = response.get("output").and_then(Value::as_array)?;
    if output.is_empty() {
        return None;
    }
    if output.iter().any(|item| item.get("phase").is_some()) {
        return None;
    }
    if output.iter().any(|item| {
        !matches!(
            item.get("type").and_then(Value::as_str),
            Some("message" | "reasoning" | "function_call" | "custom_tool_call")
        )
    }) {
        return None;
    }
    let response = parse_responses_payload(response.clone(), model.to_string(), false).ok()?;
    let response = OpenAIProvider::normalize_reasoning_output(model, response);
    let message = crate::provider::Message::assistant_with_tools_and_reasoning(
        response.content.unwrap_or_default(),
        response.tool_calls.unwrap_or_default(),
        response
            .reasoning_details
            .map(|details| {
                details
                    .into_iter()
                    .map(|detail| serde_json::from_str(&detail))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
            .ok()?,
    );
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![message].into(),
        ..Default::default()
    };
    let mut replay = build_standard_responses_payload(&request, true).ok()?.input;
    // The history builder supplies missing tool outputs for HTTP replay. They
    // are not part of this completion and must stay in the next turn's delta.
    replay.retain(|item| {
        !matches!(item.get("type").and_then(Value::as_str), Some("function_call_output" | "custom_tool_call_output"))
    });
    Some(canonical_history(&replay))
}

pub(crate) fn responses_websocket_url(base_url: &str) -> Result<String, LLMError> {
    let mut url = url::Url::parse(base_url)
        .map_err(|err| format_provider_error(format!("Invalid OpenAI base URL for WebSocket mode: {err}")))?;

    match url.scheme() {
        "https" => {
            let _ = url.set_scheme("wss");
        }
        "http" => {
            let _ = url.set_scheme("ws");
        }
        "wss" | "ws" => {}
        other => {
            return Err(format_provider_error(format!("Unsupported URL scheme for WebSocket mode: {other}")));
        }
    }

    if !url.path().ends_with("/responses") {
        let mut path = url.path().trim_end_matches('/').to_string();
        if path.is_empty() {
            path.push('/');
        }
        path.push_str("/responses");
        url.set_path(&path);
    }

    Ok(url.to_string())
}

#[cold]
pub(crate) fn format_provider_error(message: String) -> LLMError {
    LLMError::Provider {
        message: error_display::format_llm_error("OpenAI", &message),
        metadata: None,
    }
}

#[cold]
pub(crate) fn format_network_error(message: String) -> LLMError {
    LLMError::Network {
        message: error_display::format_llm_error("OpenAI", &message),
        metadata: None,
    }
}
