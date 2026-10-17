//! OpenAIProvider WebSocket generate/stream entry points.

use super::*;

use super::super::super::errors::is_flex_service_tier_unsupported;
use super::super::super::responses_api::{build_standard_responses_payload, parse_responses_completion};
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
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

impl OpenAIProvider {
    async fn websocket_lease(
        &self,
        request: &LLMRequest,
        payload: &Value,
        deadline: Instant,
    ) -> Result<WebSocketLease, LLMError> {
        let mut slot = timeout_at(deadline, Arc::clone(&self.websocket_session).lock_owned())
            .await
            .map_err(|_elapsed| websocket_deadline_error())?;
        // Taking both before the first await makes cancellation invalidate state,
        // including cancellation during authentication or connection startup.
        let session = slot.take();
        let continuation = self
            .websocket_continuation_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let mut lease = WebSocketLease {
            slot,
            session,
            cache: Arc::clone(&self.websocket_continuation_cache),
            continuation,
            tier_cache: Arc::clone(&self.service_tier_unsupported_cache),
            model: request.model.clone(),
            tier_requested: payload_uses_flex_service_tier(payload),
            deadline,
        };
        if lease.session.is_none() {
            lease.continuation = None;
        }
        timeout_at(deadline, Box::pin(self.ensure_websocket_session(&mut lease.session, request)))
            .await
            .map_err(|_elapsed| websocket_deadline_error())??;
        Ok(lease)
    }

    pub(crate) async fn stream_via_responses_websocket(
        &self,
        request: &LLMRequest,
        legacy_startup: bool,
    ) -> Result<LLMNormalizedStream, LLMError> {
        let payload = self.convert_to_openai_responses_format(request)?;
        let payload = self.maybe_strip_flex_service_tier(&request.model, &payload);
        let deadline = Instant::now() + self.websocket_streaming_ceiling;
        let mut lease = self.websocket_lease(request, &payload, deadline).await?;
        // Another response may have rejected the tier while this request waited
        // for the lane. Apply that cache before sending either event.
        let payload = self.maybe_strip_flex_service_tier(&request.model, &payload);
        lease.tier_requested = payload_uses_flex_service_tier(&payload);
        if lease.continuation.is_none() {
            let warmup = prepare_websocket_event(&payload, None, true)?;
            lease.send(&warmup.event).await?;
            let response = lease.read_response().await?;
            lease.continuation =
                OpenAIResponsesWebSocketContinuationCache::from_response(&response, request, &warmup, &self.model);
        }
        let prepared = prepare_websocket_event(&payload, lease.continuation.as_ref(), false)?;
        lease.send(&prepared.event).await?;
        let model = request.model.clone();
        let parse_model = model.clone();
        let include_metrics = self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        let mut processor = ResponsesNormalizedStreamProcessor::new(
            ResponsesNormalizedStreamOptions {
                provider_name: "OpenAI",
                model,
                emit_reasoning: Self::model_supports_reasoning_summaries(&request.model),
                include_cached_prompt_metrics: include_metrics,
            },
            move |value| {
                let response = parse_responses_completion(value, parse_model.clone(), include_metrics)?;
                Ok(Self::normalize_reasoning_output(&parse_model, response))
            },
        );
        let request = request.clone();
        let fallback_model = self.model.clone();
        // Consume lifecycle/control frames here so failures before any deliverable
        // event can fall back to HTTP. No background task outlives stream drop.
        let mut first_events;
        let mut completed = None;
        loop {
            let event = lease.read().await?;
            first_events = processor.handle_payload_data(&event.to_string())?;
            if processor.is_done() {
                completed = event.get("response").cloned();
                break;
            }
            if legacy_startup {
                // Legacy callers only receive tool calls in their completion.
                first_events.retain(|event| {
                    !matches!(
                        event,
                        NormalizedStreamEvent::ToolCallStart { .. } | NormalizedStreamEvent::ToolCallDelta { .. }
                    )
                });
            }
            if !first_events.is_empty() {
                break;
            }
        }
        // Validate completion-only startup eagerly, without polling a stream
        // that would release its lease before the caller receives Done.
        if let Some(response) = completed {
            let final_events = processor.finish()?;
            let continuation = OpenAIResponsesWebSocketContinuationCache::from_response(
                &response,
                &request,
                &prepared,
                &fallback_model,
            );
            return Ok(completed_websocket_stream(lease, continuation, final_events));
        }
        Ok(Box::pin(async_stream::try_stream! {
            for event in first_events { yield event; }
            let response = loop {
                let event = lease.read().await?;
                for event in processor.handle_payload_data(&event.to_string())? { yield event; }
                if processor.is_done() {
                    break event.get("response").cloned().ok_or_else(|| format_provider_error("OpenAI WebSocket stream ended without completion".to_string()))?;
                }
            };
            let final_events = processor.finish()?;
            let continuation = OpenAIResponsesWebSocketContinuationCache::from_response(&response, &request, &prepared, &fallback_model);
            let mut completion = completed_websocket_stream(lease, continuation, final_events);
            while let Some(event) = completion.next().await {
                yield event?;
            }
        }))
    }

    pub(crate) async fn generate_via_responses_websocket(&self, request: &LLMRequest) -> Result<LLMResponse, LLMError> {
        let payload = self.convert_to_openai_responses_format(request)?;
        let payload = self.maybe_strip_flex_service_tier(&request.model, &payload);
        let deadline = Instant::now() + self.websocket_streaming_ceiling;
        let mut retried_active_response = false;
        let mut retried_reconnect = false;
        let mut retried_new_chain = false;
        loop {
            let mut lease = self.websocket_lease(request, &payload, deadline).await?;
            let payload = self.maybe_strip_flex_service_tier(&request.model, &payload);
            lease.tier_requested = payload_uses_flex_service_tier(&payload);
            if lease.continuation.is_none() {
                let warmup = prepare_websocket_event(&payload, None, true)?;
                let result = async {
                    lease.send(&warmup.event).await?;
                    lease.read_response().await
                }
                .await;
                match result {
                    Ok(response) => {
                        lease.continuation = OpenAIResponsesWebSocketContinuationCache::from_response(
                            &response,
                            request,
                            &warmup,
                            &self.model,
                        )
                    }
                    Err(err) => {
                        if !retried_reconnect
                            && (is_websocket_reconnect_error(&err) || is_websocket_connection_limit_error(&err))
                        {
                            retried_reconnect = true;
                            continue;
                        }
                        return Err(err);
                    }
                }
            }
            let prepared = prepare_websocket_event(&payload, lease.continuation.as_ref(), false)?;
            let result = async {
                lease.send(&prepared.event).await?;
                lease.read_response().await
            }
            .await;
            match result {
                Ok(response_json) => {
                    let parsed = self.parse_openai_responses_response(response_json.clone(), request.model.clone())?;
                    let continuation = OpenAIResponsesWebSocketContinuationCache::from_response(
                        &response_json,
                        request,
                        &prepared,
                        &self.model,
                    );
                    lease.complete(continuation);
                    return Ok(parsed);
                }
                Err(err) => {
                    if !retried_active_response && is_websocket_active_response_error(&err) {
                        retried_active_response = true;
                        continue;
                    }
                    if !retried_new_chain
                        && prepared.used_previous_response_id
                        && is_websocket_previous_response_not_found_error(&err)
                    {
                        retried_new_chain = true;
                        continue;
                    }
                    if !retried_reconnect
                        && (is_websocket_reconnect_error(&err) || is_websocket_connection_limit_error(&err))
                    {
                        retried_reconnect = true;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    pub(super) async fn ensure_websocket_session<'a>(
        &self,
        session_guard: &'a mut Option<OpenAIResponsesWebSocketSession>,
        request: &LLMRequest,
    ) -> Result<&'a mut OpenAIResponsesWebSocketSession, LLMError> {
        if session_guard.is_none() {
            let connection_deadline = Instant::now() + Duration::from_secs(30);
            let ws_url = responses_websocket_url(&self.base_url)?;
            let build_request = |api_key: &str| -> Result<_, LLMError> {
                let mut ws_request = ws_url
                    .clone()
                    .into_client_request()
                    .map_err(|err| format_provider_error(format!("Invalid OpenAI WebSocket request: {err}")))?;

                ws_request.headers_mut().insert(
                    "Authorization",
                    HeaderValue::from_str(&format!("Bearer {api_key}"))
                        .map_err(|err| format_provider_error(format!("Invalid OpenAI authorization header: {err}")))?,
                );
                ws_request
                    .headers_mut()
                    .insert("OpenAI-Beta", HeaderValue::from_static(OPENAI_BETA_RESPONSES_WEBSOCKET_V2));
                if let Some(metadata) = &request.metadata
                    && let Ok(metadata_str) = serde_json::to_string(metadata)
                    && let Ok(value) = HeaderValue::from_str(&metadata_str)
                {
                    ws_request.headers_mut().insert("X-Turn-Metadata", value);
                }

                Ok(ws_request)
            };

            let api_key = timeout_at(connection_deadline, self.current_api_key())
                .await
                .map_err(|_elapsed| format_network_error("OpenAI WebSocket connection timeout".to_string()))??;
            let ws_request = build_request(&api_key)?;
            let connector = websocket_tls_connector()?;
            let socket = match timeout_at(
                connection_deadline,
                Box::pin(connect_async_tls_with_config(ws_request, None, false, Some(connector.clone()))),
            )
            .await
            .map_err(|_elapsed| websocket_deadline_error())?
            {
                Ok((socket, _)) => socket,
                Err(err) if self.uses_chatgpt_auth() && is_websocket_auth_retryable(&err) => {
                    let retry_api_key = timeout_at(connection_deadline, self.refresh_api_key_for_retry())
                        .await
                        .map_err(|_elapsed| {
                            format_network_error("OpenAI WebSocket connection timeout".to_string())
                        })??;
                    let retry_request = build_request(&retry_api_key)?;
                    let (socket, _) = timeout_at(
                        connection_deadline,
                        Box::pin(connect_async_tls_with_config(retry_request, None, false, Some(connector))),
                    )
                    .await
                    .map_err(|_elapsed| websocket_deadline_error())?
                    .map_err(|retry_err| {
                        format_network_error(format!("Failed to connect OpenAI WebSocket: {retry_err}"))
                    })?;
                    socket
                }
                Err(err) => {
                    return Err(format_network_error(format!("Failed to connect OpenAI WebSocket: {err}")));
                }
            };
            *session_guard = Some(OpenAIResponsesWebSocketSession::new(socket));
            tracing::info!(transport = "websocket", "OpenAI Responses connection established");
        }

        session_guard
            .as_mut()
            .ok_or_else(|| format_provider_error("OpenAI WebSocket session unexpectedly missing".to_string()))
    }

    #[cfg(test)]
    pub(super) fn websocket_continuation_snapshot(&self) -> Option<OpenAIResponsesWebSocketContinuationCache> {
        self.websocket_continuation_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    #[cfg(test)]
    pub(super) fn update_websocket_continuation(
        &self,
        response_json: &Value,
        request: &LLMRequest,
        prepared: &PreparedWebSocketEvent,
        fallback_model: &str,
    ) {
        let cache =
            OpenAIResponsesWebSocketContinuationCache::from_response(response_json, request, prepared, fallback_model);
        *self
            .websocket_continuation_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = cache;
    }
}
