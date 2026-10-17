//! WebSocket session, continuation cache, and send/receive lease.

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

type ResponsesSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OpenAIResponsesWebSocketContinuationCache {
    pub(super) response_id: String,
    pub(super) full_input: Vec<Value>,
    pub(super) model: String,
    pub(super) instructions: Option<String>,
    pub(super) tools: Option<Value>,
}

impl OpenAIResponsesWebSocketContinuationCache {
    pub(crate) fn from_response(
        response_json: &Value,
        request: &LLMRequest,
        prepared: &PreparedWebSocketEvent,
        fallback_model: &str,
    ) -> Option<Self> {
        let response_id = response_json
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_string();

        let model = if request.model.trim().is_empty() {
            fallback_model.to_string()
        } else {
            request.model.clone()
        };
        let instructions = response_json
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| prepared.event.get("instructions").and_then(Value::as_str).map(str::to_owned));
        let tools = prepared.event.get("tools").cloned();
        let mut full_input = canonical_history(&prepared.full_input);
        if prepared.event.get("generate") != Some(&Value::Bool(false)) {
            full_input.extend(completed_replay_output(response_json, &model)?);
        }
        Some(Self {
            response_id,
            full_input,
            model,
            instructions,
            tools,
        })
    }

    pub(crate) fn can_continue_from(&self, payload: &Value) -> bool {
        if self.response_id.is_empty() {
            return false;
        }

        let Some(current_model) = payload.get("model").and_then(Value::as_str) else {
            return false;
        };
        if self.model != current_model {
            return false;
        }

        let current_instructions = payload.get("instructions").and_then(Value::as_str).map(str::to_owned);
        if self.instructions != current_instructions {
            return false;
        }

        let current_tools = payload.get("tools").cloned();
        if self.tools != current_tools {
            return false;
        }

        let Some(current_input) = payload.get("input").and_then(Value::as_array) else {
            return false;
        };
        input_is_incremental(self.full_input.as_slice(), current_input.as_slice())
    }
}

#[derive(Debug)]
pub(crate) struct OpenAIResponsesWebSocketSession {
    pub(super) socket: Box<ResponsesSocket>,
}

impl OpenAIResponsesWebSocketSession {
    pub(crate) fn new(socket: ResponsesSocket) -> Self {
        Self { socket: Box::new(socket) }
    }
}

#[derive(Debug)]
pub(crate) struct PreparedWebSocketEvent {
    pub(super) event: Value,
    pub(super) full_input: Vec<Value>,
    pub(super) used_previous_response_id: bool,
}

// A response owns the socket outside its reusable slot. Dropping a future or
// stream closes that socket; only a validated completion returns it to the slot.
pub(crate) struct WebSocketLease {
    pub(super) slot: OwnedMutexGuard<Option<OpenAIResponsesWebSocketSession>>,
    pub(super) session: Option<OpenAIResponsesWebSocketSession>,
    pub(super) cache: Arc<Mutex<Option<OpenAIResponsesWebSocketContinuationCache>>>,
    pub(super) continuation: Option<OpenAIResponsesWebSocketContinuationCache>,
    pub(super) tier_cache: Arc<Mutex<HashMap<String, bool>>>,
    pub(super) model: String,
    pub(super) tier_requested: bool,
    pub(super) deadline: Instant,
}

impl WebSocketLease {
    pub(crate) fn session(&mut self) -> Result<&mut OpenAIResponsesWebSocketSession, LLMError> {
        self.session
            .as_mut()
            .ok_or_else(|| format_provider_error("OpenAI WebSocket session missing".to_string()))
    }

    pub(crate) async fn send(&mut self, event: &Value) -> Result<(), LLMError> {
        let deadline = self.deadline;
        timeout_at(deadline, self.session()?.socket.send(Message::Text(event.to_string().into())))
            .await
            .map_err(|_elapsed| websocket_deadline_error())?
            .map_err(|err| format_network_error(format!("Failed to send OpenAI WebSocket payload: {err}")))
    }

    pub(crate) async fn read(&mut self) -> Result<Value, LLMError> {
        let deadline = self.deadline;
        let event = timeout_at(deadline, read_websocket_event(self.session()?))
            .await
            .map_err(|_elapsed| websocket_deadline_error())??;
        let policy = crate::providers::shared::response_stream_event_policy(&event)
            .map_err(|message| format_provider_error(message.to_string()))?;
        if policy == crate::providers::shared::ResponsesStreamEventPolicy::Unsupported {
            return Err(format_provider_error("Unsupported OpenAI WebSocket event".to_string()));
        }
        let event_type = event.get("type").and_then(Value::as_str);
        if matches!(event_type, Some("error" | "response.failed" | "response.incomplete")) {
            let error = event.get("response").unwrap_or(&event);
            let status = event
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|status| u16::try_from(status).ok())
                .and_then(|status| reqwest::StatusCode::from_u16(status).ok())
                .unwrap_or(reqwest::StatusCode::BAD_REQUEST);
            if self.tier_requested && is_flex_service_tier_unsupported(status, &error.to_string()) {
                self.tier_cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(self.model.clone(), true);
                tracing::warn!(model = %self.model, "OpenAI WebSocket rejected service tier; using tier-less HTTP");
            }
            let error = error.get("error").unwrap_or(error);
            let code = error.get("code").and_then(Value::as_str).unwrap_or("unknown_error");
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("OpenAI WebSocket response failed or incomplete");
            return Err(format_provider_error(format!("{code}: {message}")));
        }
        if event_type == Some("response.completed") {
            let response = event.get("response").filter(|value| value.is_object()).ok_or_else(|| {
                format_provider_error("OpenAI WebSocket completed event missing response".to_string())
            })?;
            if response
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status != "completed")
            {
                return Err(format_provider_error("OpenAI WebSocket completion has unsuccessful status".to_string()));
            }
        }
        Ok(event)
    }

    pub(crate) async fn read_response(&mut self) -> Result<Value, LLMError> {
        loop {
            let event = self.read().await?;
            if event.get("type").and_then(Value::as_str) == Some("response.completed") {
                return event
                    .get("response")
                    .cloned()
                    .ok_or_else(|| format_provider_error("OpenAI WebSocket completion missing response".to_string()));
            }
        }
    }

    pub(crate) fn complete(mut self, continuation: Option<OpenAIResponsesWebSocketContinuationCache>) {
        *self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = continuation;
        *self.slot = self.session.take();
    }
}

pub(crate) fn websocket_deadline_error() -> LLMError {
    format_network_error("OpenAI WebSocket request deadline exceeded".to_string())
}

pub(crate) fn completed_websocket_stream(
    lease: WebSocketLease,
    continuation: Option<OpenAIResponsesWebSocketContinuationCache>,
    events: Vec<NormalizedStreamEvent>,
) -> LLMNormalizedStream {
    Box::pin(async_stream::try_stream! {
        for event in events {
            if matches!(event, NormalizedStreamEvent::Done { .. }) {
                // Usage can precede Done. Dropping before completion delivery
                // must still invalidate this lease, including during startup.
                lease.complete(continuation);
                yield event;
                break;
            }
            yield event;
        }
    })
}
