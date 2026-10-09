use super::super::errors::{
    fallback_model_if_not_found, format_openai_error, is_flex_service_tier_unsupported, is_model_not_found,
    is_responses_api_unsupported,
};
use super::super::headers;
use super::super::types::ResponsesApiState;
use super::OpenAIProvider;
use crate::error_display;
use crate::provider;
use crate::provider::LLMProvider;
use crate::providers::error_handling::{is_rate_limit_error, parse_api_error};
use crate::providers::shared::parse_compacted_output_messages;
use futures::StreamExt;
use serde_json::{Value, json};
use std::borrow::Cow;
use vtcode_commons::sanitizer::sanitize_provider_diagnostic;

const MANUAL_COMPACTION_INSTRUCTIONS_LABEL: &str = "[Manual Compaction Instructions]";

#[inline]
fn should_attempt_responses_api(state: ResponsesApiState) -> bool {
    !matches!(state, ResponsesApiState::Disabled)
}

fn truncate_for_log(input: &str, max_chars: usize) -> String {
    vtcode_commons::formatting::truncate_text(input, max_chars, "")
}

/// Whether the payload carries a tier eligible for the strip-and-retry
/// fallback. The `flex_` name is historical (built for flex first): any
/// user-selectable tier can be rejected per model, account, or project
/// policy, and omitting it resolves to the project default.
pub(super) fn payload_uses_flex_service_tier(payload: &Value) -> bool {
    payload_service_tier(payload).is_some_and(|value| {
        value.eq_ignore_ascii_case("flex")
            || value.eq_ignore_ascii_case("ultrafast")
            || value.eq_ignore_ascii_case("priority")
    })
}

fn payload_without_service_tier(payload: &Value) -> Value {
    let mut payload = payload.clone();
    if let Some(object) = payload.as_object_mut() {
        object.remove("service_tier");
    }
    payload
}

pub(super) fn payload_service_tier(payload: &Value) -> Option<&str> {
    payload
        .get("service_tier")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_ultrafast_service_tier(value: &str) -> bool {
    value.eq_ignore_ascii_case("ultrafast")
}

/// Shared tier-rejection warn so Responses and Chat Completions stay in sync.
/// Ultrafast carries extra access guidance (GA vs preview, residency).
pub(super) fn log_service_tier_rejection(api: &str, model: &str, client_request_id: &str, tier: &str) {
    if is_ultrafast_service_tier(tier) {
        tracing::warn!(
            model = %model,
            client_request_id = %client_request_id,
            tier = %tier,
            "OpenAI {api} request rejected service_tier=ultrafast; retrying without it (ultrafast is GA for gpt-6-astra/gpt-6.1-sol, preview-only for gpt-5.6-sol — contact your OpenAI account team for access; Astra US/global only, 6.1-sol US/EU/global)"
        );
    } else {
        tracing::warn!(
            model = %model,
            client_request_id = %client_request_id,
            tier = %tier,
            "OpenAI {api} request rejected service_tier; retrying without it"
        );
    }
}

fn append_manual_compaction_instructions(
    derived_instructions: Option<&str>,
    manual_instructions: Option<&str>,
) -> Option<String> {
    let manual_instructions = manual_instructions.map(str::trim).filter(|value| !value.is_empty())?;
    let manual_section = format!("{MANUAL_COMPACTION_INSTRUCTIONS_LABEL}\n{manual_instructions}");

    match derived_instructions.map(str::trim).filter(|value| !value.is_empty()) {
        Some(existing) => Some(format!("{existing}\n\n{manual_section}")),
        None => Some(manual_section),
    }
}

async fn collect_streamed_response(stream: provider::LLMStream) -> Result<provider::LLMResponse, provider::LLMError> {
    let mut stream = stream;
    let mut streamed_content = String::new();
    let mut streamed_reasoning = String::new();
    let mut completed = None;

    while let Some(event) = stream.next().await {
        match event? {
            provider::LLMStreamEvent::Token { delta } => streamed_content.push_str(&delta),
            provider::LLMStreamEvent::Reasoning { delta } => streamed_reasoning.push_str(&delta),
            provider::LLMStreamEvent::ReasoningSignature { .. } => {}
            provider::LLMStreamEvent::ReasoningStage { .. } => {}
            provider::LLMStreamEvent::Completed { response } => {
                completed = Some(*response);
                break;
            }
        }
    }

    let mut response = completed.ok_or_else(|| provider::LLMError::Provider {
        message: error_display::format_llm_error("OpenAI", "Streaming response ended without a completion event"),
        metadata: None,
    })?;

    if response.content.as_deref().unwrap_or_default().is_empty() && !streamed_content.is_empty() {
        response.content = Some(streamed_content);
    }

    if response.reasoning.is_none() && !streamed_reasoning.is_empty() {
        response.reasoning = Some(streamed_reasoning);
    }

    Ok(response)
}

impl OpenAIProvider {
    /// Check if `service_tier=flex` is known to be unsupported for this model.
    /// Returns `true` if the model was previously rejected with flex tier.
    fn is_flex_unsupported_for_model(&self, model: &str) -> bool {
        self.service_tier_unsupported_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(model)
            .copied()
            .unwrap_or(false)
    }

    /// Mark `service_tier=flex` as unsupported for this model so future
    /// requests skip it entirely, avoiding the wasted first request + retry.
    pub(super) fn mark_flex_unsupported_for_model(&self, model: &str) {
        self.service_tier_unsupported_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(model.to_string(), true);
        tracing::info!(model = %model, "cached service_tier=flex unsupported for model");
    }

    /// Strip `service_tier` from the payload if it was previously rejected
    /// for this model. Returns a borrowed reference when no modification is
    /// needed (the common case), avoiding a deep clone of the full request
    /// payload.
    pub(super) fn maybe_strip_flex_service_tier<'a>(&self, model: &str, payload: &'a Value) -> Cow<'a, Value> {
        if self.is_flex_unsupported_for_model(model) && payload_uses_flex_service_tier(payload) {
            tracing::debug!(
                model = %model,
                "skipping service_tier=flex (previously unsupported)"
            );
            Cow::Owned(payload_without_service_tier(payload))
        } else {
            Cow::Borrowed(payload)
        }
    }

    /// Ultrafast is fastest on a persistent WebSocket; HTTP handshake overhead
    /// erodes the gain on agentic tool-call bursts. Emit a one-line hint when
    /// an ultrafast request will run over HTTP because `websocket_mode` is off.
    pub(super) fn log_ultrafast_websocket_hint(&self, model: &str, payload: &Value) {
        if payload_service_tier(payload).is_some_and(is_ultrafast_service_tier) && !self.websocket_mode_enabled(model) {
            tracing::debug!(
                model = %model,
                "service_tier=ultrafast without websocket_mode; set provider.openai.websocket_mode=true to keep one Responses WebSocket + previous_response_id chain for tool-call bursts"
            );
        }
    }

    pub(super) async fn retry_without_service_tier(
        &self,
        url: &str,
        metadata: &Option<Value>,
        payload: &Value,
        include_responses_beta: bool,
    ) -> Result<(reqwest::Response, String), provider::LLMError> {
        let retry_payload = payload_without_service_tier(payload);
        let retry_client_request_id = Self::new_client_request_id();
        let response = self
            .send_authorized(|auth| {
                let builder = self.authorize_with_api_key(self.http_client.post(url), auth);
                let builder = if include_responses_beta {
                    headers::apply_responses_beta(builder)
                } else {
                    builder
                };
                headers::apply_turn_metadata(
                    headers::apply_client_request_id(builder, &retry_client_request_id),
                    metadata,
                )
                .json(&retry_payload)
            })
            .await?;
        Ok((response, retry_client_request_id))
    }

    pub(crate) async fn compact_history_request(
        &self,
        model: &str,
        history: &[provider::Message],
    ) -> Result<Vec<provider::Message>, provider::LLMError> {
        self.compact_history_request_with_options(model, history, &provider::ResponsesCompactionOptions::default())
            .await
    }

    pub(crate) async fn compact_history_request_with_options(
        &self,
        model: &str,
        history: &[provider::Message],
        options: &provider::ResponsesCompactionOptions,
    ) -> Result<Vec<provider::Message>, provider::LLMError> {
        let resolved_model = if model.trim().is_empty() {
            self.model.to_string()
        } else {
            model.trim().to_string()
        };

        let request = provider::LLMRequest {
            model: resolved_model.clone(),
            messages: std::sync::Arc::new(history.to_vec()),
            max_tokens: options.max_output_tokens,
            reasoning_effort: options.reasoning_effort,
            verbosity: options.verbosity,
            response_store: options.response_store,
            responses_include: options.responses_include.clone(),
            service_tier: options.service_tier.clone(),
            prompt_cache_key: options.prompt_cache_key.clone(),
            ..Default::default()
        };
        let responses_request = self.convert_to_openai_responses_format(&request)?;
        let input = responses_request
            .get("input")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if input.is_empty() {
            return Ok(history.to_vec());
        }

        let mut compact_payload = json!({
            "model": resolved_model,
            "input": input,
            "stream": true,
        });
        if let Some(map) = compact_payload.as_object_mut() {
            let merged_instructions = append_manual_compaction_instructions(
                responses_request.get("instructions").and_then(Value::as_str),
                options.instructions.as_deref(),
            );
            if let Some(instructions) = merged_instructions {
                map.insert("instructions".to_string(), json!(instructions));
            }

            for key in [
                "max_output_tokens",
                "service_tier",
                "store",
                "include",
                "reasoning",
                "text",
                "prompt_cache_key",
                "prompt_cache_options",
                "prompt_cache_retention",
            ] {
                if let Some(value) = responses_request.get(key) {
                    map.insert(key.to_string(), value.clone());
                }
            }
        }
        let url = &self.responses_compact_url[..];
        let client_request_id = Self::new_client_request_id();

        let response = self
            .send_authorized(|auth| {
                headers::apply_client_request_id(
                    headers::apply_responses_beta(self.authorize_with_api_key(self.http_client.post(url), auth)),
                    &client_request_id,
                )
                .json(&compact_payload)
            })
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();
            let error_text = crate::providers::common::read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(
                    status,
                    &error_text,
                    &headers,
                    "Compaction endpoint error",
                    Some(&client_request_id),
                ),
            );
            return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
        }

        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error =
                error_display::format_llm_error("OpenAI", &format!("Failed to parse compaction response: {e}"));
            provider::LLMError::Provider { message: formatted_error, metadata: None }
        })?;
        let output = response_json.get("output").and_then(|value| value.as_array()).ok_or_else(|| {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "Invalid compaction response format: missing output array");
            provider::LLMError::Provider { message: formatted_error, metadata: None }
        })?;

        let compacted = parse_compacted_output_messages(output);
        if compacted.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "Compaction response contained no reusable messages");
            return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
        }

        Ok(compacted)
    }

    pub(crate) async fn generate_request(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let mut request = request;

        crate::providers::common::ensure_model(&mut request, &self.model);

        if Self::requires_streaming_responses(&request.model) {
            request.stream = true;
            let stream = self.stream_request(request).await?;
            return collect_streamed_response(stream).await;
        }

        let model = request.model.clone();

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        // Check if this is a harmony model (GPT-OSS)
        if Self::uses_harmony(&request.model) {
            return self.generate_with_harmony(request).await;
        }

        let responses_state = self.responses_api_state(&request.model);
        let attempt_responses = should_attempt_responses_api(responses_state);
        if attempt_responses {
            if self.websocket_mode_enabled(&request.model) {
                if let Ok(response) = self.generate_via_responses_websocket(&request).await {
                    return Ok(response);
                }
            }

            let openai_request = self.convert_to_openai_responses_format(&request)?;
            // Skip flex tier if it was previously rejected for this model
            let openai_request = self.maybe_strip_flex_service_tier(&request.model, &openai_request);
            self.log_ultrafast_websocket_hint(&request.model, &openai_request);
            let url = &self.responses_url[..];
            let client_request_id = Self::new_client_request_id();

            let response = self
                .send_authorized(|auth| {
                    headers::apply_turn_metadata(
                        headers::apply_client_request_id(
                            headers::apply_responses_beta(
                                self.authorize_with_api_key(self.http_client.post(url), auth),
                            ),
                            &client_request_id,
                        ),
                        &request.metadata,
                    )
                    .json(&openai_request)
                })
                .await?;

            if !response.status().is_success() {
                let mut status = response.status();
                let mut headers = response.headers().clone();
                let mut error_text = crate::providers::common::read_provider_error_body(response).await;
                let mut effective_client_request_id = client_request_id.clone();

                if payload_uses_flex_service_tier(&openai_request)
                    && is_flex_service_tier_unsupported(status, &error_text)
                {
                    // Cache this so future requests skip flex tier entirely
                    self.mark_flex_unsupported_for_model(&request.model);
                    let rejected_tier = payload_service_tier(&openai_request).unwrap_or("flex");
                    log_service_tier_rejection("Responses", &request.model, &client_request_id, rejected_tier);

                    let (retry_response, retry_client_request_id) = self
                        .retry_without_service_tier(url, &request.metadata, &openai_request, true)
                        .await?;
                    effective_client_request_id = retry_client_request_id;

                    if retry_response.status().is_success() {
                        let openai_response: Value = retry_response.json().await.map_err(|e| {
                            let formatted_error =
                                error_display::format_llm_error("OpenAI", &format!("Failed to parse response: {e}"));
                            provider::LLMError::Provider { message: formatted_error, metadata: None }
                        })?;

                        let response = self.parse_openai_responses_response(openai_response, model.clone())?;
                        return Ok(response);
                    }

                    status = retry_response.status();
                    headers = retry_response.headers().clone();
                    error_text = crate::providers::common::read_provider_error_body(retry_response).await;
                }
                let lower_error = error_text.to_ascii_lowercase();
                if status == reqwest::StatusCode::BAD_REQUEST
                    && lower_error.contains("invalid_request_error")
                    && lower_error.contains("\"param\":\"input\"")
                {
                    tracing::error!(
                        client_request_id = %client_request_id,
                        status = %status,
                        openai_request = %truncate_for_log(&openai_request.to_string(), 12_000),
                        openai_error_body = %sanitize_provider_diagnostic(error_text.as_bytes()),
                        "OpenAI Responses request rejected with invalid input payload"
                    );
                }

                if is_model_not_found(status, &error_text) {
                    if let Some(fallback_model) = fallback_model_if_not_found(&request.model) {
                        if fallback_model != request.model {
                            let mut retry_request = request.clone();
                            retry_request.model = fallback_model;
                            let retry_openai = self.convert_to_openai_responses_format(&retry_request)?;
                            let retry_client_request_id = Self::new_client_request_id();
                            let retry_response = self
                                .send_authorized(|auth| {
                                    headers::apply_turn_metadata(
                                        headers::apply_client_request_id(
                                            headers::apply_responses_beta(
                                                self.authorize_with_api_key(self.http_client.post(url), auth),
                                            ),
                                            &retry_client_request_id,
                                        ),
                                        &request.metadata,
                                    )
                                    .json(&retry_openai)
                                })
                                .await?;
                            if retry_response.status().is_success() {
                                let openai_response: Value = retry_response.json().await.map_err(|e| {
                                    let formatted_error = error_display::format_llm_error(
                                        "OpenAI",
                                        &format!("Failed to parse response: {e}"),
                                    );
                                    provider::LLMError::Provider { message: formatted_error, metadata: None }
                                })?;
                                let response = self.parse_openai_responses_response(openai_response, model.clone())?;
                                return Ok(response);
                            }
                        }
                    }
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format_openai_error(
                            status,
                            &error_text,
                            &headers,
                            "Model not available",
                            Some(&effective_client_request_id),
                        ),
                    );
                    return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
                } else if matches!(responses_state, ResponsesApiState::Allowed)
                    && is_responses_api_unsupported(status, &error_text)
                {
                    if self.allows_chat_completions_fallback() {
                        self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                        return self.generate_chat_completions(&request).await;
                    }

                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format_openai_error(
                            status,
                            &error_text,
                            &headers,
                            "Responses API error",
                            Some(&effective_client_request_id),
                        ),
                    );
                    return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
                } else if is_rate_limit_error(status.as_u16(), &error_text) {
                    return Err(parse_api_error("OpenAI", status, &error_text));
                } else {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format_openai_error(
                            status,
                            &error_text,
                            &headers,
                            "Responses API error",
                            Some(&effective_client_request_id),
                        ),
                    );
                    return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
                }
            } else {
                let openai_response: Value = response.json().await.map_err(|e| {
                    let formatted_error =
                        error_display::format_llm_error("OpenAI", &format!("Failed to parse response: {e}"));
                    provider::LLMError::Provider { message: formatted_error, metadata: None }
                })?;

                let response = self.parse_openai_responses_response(openai_response, model.clone())?;
                return Ok(response);
            }
        }

        self.generate_chat_completions(&request).await
    }

    async fn generate_chat_completions(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let model = request.model.clone();
        let openai_request = self.convert_to_openai_format(request)?;
        let url = &self.chat_completions_url[..];
        let client_request_id = Self::new_client_request_id();

        let response = self
            .send_authorized(|auth| {
                headers::apply_turn_metadata(
                    headers::apply_client_request_id(
                        self.authorize_with_api_key(self.http_client.post(url), auth),
                        &client_request_id,
                    ),
                    &request.metadata,
                )
                .json(&openai_request)
            })
            .await?;

        if !response.status().is_success() {
            let mut status = response.status();
            let mut headers = response.headers().clone();
            let mut error_text = crate::providers::common::read_provider_error_body(response).await;
            let mut effective_client_request_id = client_request_id.clone();

            if payload_uses_flex_service_tier(&openai_request) && is_flex_service_tier_unsupported(status, &error_text)
            {
                // Cache this so future requests skip flex tier entirely
                self.mark_flex_unsupported_for_model(&request.model);
                let rejected_tier = payload_service_tier(&openai_request).unwrap_or("flex");
                log_service_tier_rejection("Chat Completions", &request.model, &client_request_id, rejected_tier);

                let (retry_response, retry_client_request_id) = self
                    .retry_without_service_tier(url, &request.metadata, &openai_request, false)
                    .await?;
                effective_client_request_id = retry_client_request_id;

                if retry_response.status().is_success() {
                    let openai_response: Value = retry_response.json().await.map_err(|e| {
                        let formatted_error =
                            error_display::format_llm_error("OpenAI", &format!("Failed to parse response: {e}"));
                        provider::LLMError::Provider { message: formatted_error, metadata: None }
                    })?;

                    let response = self.parse_openai_response(openai_response, model.clone())?;
                    return Ok(response);
                }

                status = retry_response.status();
                headers = retry_response.headers().clone();
                error_text = crate::providers::common::read_provider_error_body(retry_response).await;
            }

            if is_rate_limit_error(status.as_u16(), &error_text) {
                return Err(parse_api_error("OpenAI", status, &error_text));
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(
                    status,
                    &error_text,
                    &headers,
                    "Chat Completions error",
                    Some(&effective_client_request_id),
                ),
            );
            return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
        }

        let openai_response: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error("OpenAI", &format!("Failed to parse response: {e}"));
            provider::LLMError::Provider { message: formatted_error, metadata: None }
        })?;

        let response = self.parse_openai_response(openai_response, model.clone())?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::{ResponsesApiState, is_ultrafast_service_tier, payload_service_tier, should_attempt_responses_api};
    use serde_json::json;

    #[test]
    fn responses_attempt_logic_prefers_responses_for_allowed_and_required() {
        assert!(should_attempt_responses_api(ResponsesApiState::Allowed));
        assert!(should_attempt_responses_api(ResponsesApiState::Required));
        assert!(!should_attempt_responses_api(ResponsesApiState::Disabled));
    }

    #[test]
    fn ultrafast_tier_detection_is_case_insensitive_and_trims() {
        assert!(is_ultrafast_service_tier("ultrafast"));
        assert!(is_ultrafast_service_tier("ULTRAFAST"));
        assert!(!is_ultrafast_service_tier("flex"));
        assert!(!is_ultrafast_service_tier("priority"));
        assert_eq!(payload_service_tier(&json!({"service_tier": "ultrafast"})), Some("ultrafast"));
        assert_eq!(payload_service_tier(&json!({"service_tier": "   "})), None);
        assert_eq!(payload_service_tier(&json!({})), None);
    }
}
