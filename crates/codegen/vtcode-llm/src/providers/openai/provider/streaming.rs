use super::super::errors::is_flex_service_tier_unsupported;
use super::super::errors::{
    fallback_model_if_not_found, format_openai_error, is_model_not_found, is_responses_api_unsupported,
};
use super::super::headers;
use super::super::responses_api::parse_responses_completion;
use super::super::stream_decoder;
use super::super::types::ResponsesApiState;
use super::OpenAIProvider;
use super::generation::{log_service_tier_rejection, payload_service_tier, payload_uses_flex_service_tier};
use crate::error_display;
use crate::provider::LLMProvider;
use crate::provider::{self, LLMNormalizedStream};
use crate::providers::error_handling::{is_rate_limit_error, parse_api_error};
use crate::providers::shared::{ResponsesNormalizedStreamOptions, create_responses_normalized_stream};
use futures::StreamExt;
use serde_json::{Value, json};

// Retained custom streaming boundary.
// Rig 0.40 exposes Responses SSE support, but this provider still needs the
// VTCode request retry/fallback path, client request id and turn metadata
// headers, ChatGPT backend defaults, prompt-cache usage parsing, and both
// LLMStreamEvent and NormalizedStreamEvent shapes. Protected by
// `api_key_responses_stream_sends_metadata_and_preserves_usage`,
// `chatgpt_responses_stream_accepts_empty_final_output_after_text_delta`, and
// the shared `responses_stream`/`stream_decoder` tests. Remove this boundary
// only once Rig can feed those exact VTCode event and error contracts.
#[inline]
fn should_prefer_responses_stream(state: ResponsesApiState) -> bool {
    !matches!(state, ResponsesApiState::Disabled)
}

struct StreamingHttpFailure {
    status: reqwest::StatusCode,
    headers: reqwest::header::HeaderMap,
    error_text: String,
    client_request_id: String,
}

enum StreamingHttpResponse {
    Success(reqwest::Response),
    Failure(StreamingHttpFailure),
}

impl StreamingHttpResponse {
    async fn from_response(response: reqwest::Response, client_request_id: String) -> Self {
        if response.status().is_success() {
            return Self::Success(response);
        }
        let status = response.status();
        let headers = response.headers().clone();
        let error_text = crate::providers::common::read_provider_error_body(response).await;
        Self::Failure(StreamingHttpFailure { status, headers, error_text, client_request_id })
    }
}

impl OpenAIProvider {
    pub(crate) async fn stream_normalized_request(
        &self,
        mut request: provider::LLMRequest,
    ) -> Result<LLMNormalizedStream, provider::LLMError> {
        crate::providers::common::ensure_model(&mut request, &self.model);
        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        let responses_state = self.responses_api_state(&request.model);
        if !should_prefer_responses_stream(responses_state) {
            return self.stream_chat_completions_normalized(&request).await;
        }

        if self.websocket_mode_enabled(&request.model) {
            match self.stream_via_responses_websocket(&request, false).await {
                Ok(stream) => return Ok(stream),
                Err(err) => tracing::warn!(error = %err, "OpenAI WebSocket startup failed; using HTTP SSE"),
            }
        }

        loop {
            let model = request.model.clone();
            let include_metrics = self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
            let mut openai_request = self.convert_to_openai_responses_format(&request)?;
            openai_request["stream"] = Value::Bool(true);
            let openai_request = self.maybe_strip_flex_service_tier(&request.model, &openai_request);
            self.log_ultrafast_websocket_hint(&request.model, &openai_request);
            let StreamingHttpFailure { status, headers, error_text, client_request_id } =
                match self.send_http_streaming_request(&request, &openai_request, true).await? {
                    StreamingHttpResponse::Success(response) => {
                        return Ok(self.responses_normalized_stream(response, model, include_metrics));
                    }
                    StreamingHttpResponse::Failure(failure) => failure,
                };

            if is_model_not_found(status, &error_text) {
                if let Some(fallback_model) = fallback_model_if_not_found(&request.model)
                    && fallback_model != request.model
                {
                    request.model = fallback_model;
                    continue;
                }
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format_openai_error(
                        status,
                        &error_text,
                        &headers,
                        "Model not available",
                        Some(&client_request_id),
                    ),
                );
                return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
            }

            if matches!(responses_state, ResponsesApiState::Allowed)
                && is_responses_api_unsupported(status, &error_text)
                && self.allows_chat_completions_fallback()
            {
                self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                return self.stream_chat_completions_normalized(&request).await;
            }

            if is_rate_limit_error(status.as_u16(), &error_text) {
                return Err(parse_api_error("OpenAI", status, &error_text));
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(status, &error_text, &headers, "Responses API error", Some(&client_request_id)),
            );
            return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
        }
    }

    pub(crate) async fn stream_request(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        self.stream_request_with_websocket(request, true).await
    }

    pub(super) async fn stream_request_with_websocket(
        &self,
        mut request: provider::LLMRequest,
        use_websocket: bool,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        crate::providers::common::ensure_model(&mut request, &self.model);
        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        let responses_state = self.responses_api_state(&request.model);

        let prefer_responses_stream = should_prefer_responses_stream(responses_state);

        if !prefer_responses_stream {
            return self.stream_chat_completions(&request).await;
        }

        if use_websocket && self.websocket_mode_enabled(&request.model) {
            match self.stream_legacy_via_responses_websocket(&request).await {
                Ok(stream) => return Ok(stream),
                Err(err) => tracing::warn!(error = %err, "OpenAI WebSocket startup failed; using HTTP SSE"),
            }
        }

        loop {
            let model = request.model.clone();
            let include_metrics = self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
            let mut openai_request = self.convert_to_openai_responses_format(&request)?;
            openai_request["stream"] = Value::Bool(true);
            let openai_request = self.maybe_strip_flex_service_tier(&request.model, &openai_request);
            self.log_ultrafast_websocket_hint(&request.model, &openai_request);
            let StreamingHttpFailure { status, headers, error_text, client_request_id } = match self
                .send_http_streaming_request(&request, &openai_request, true)
                .await?
            {
                StreamingHttpResponse::Success(response) => {
                    return Ok(stream_decoder::create_responses_stream(response, model, include_metrics, None, None));
                }
                StreamingHttpResponse::Failure(failure) => failure,
            };

            if is_model_not_found(status, &error_text) {
                if let Some(fallback_model) = fallback_model_if_not_found(&request.model)
                    && fallback_model != request.model
                {
                    request.model = fallback_model;
                    continue;
                }
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format_openai_error(
                        status,
                        &error_text,
                        &headers,
                        "Model not available",
                        Some(&client_request_id),
                    ),
                );
                return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
            }

            if matches!(responses_state, ResponsesApiState::Allowed)
                && is_responses_api_unsupported(status, &error_text)
            {
                if self.allows_chat_completions_fallback() {
                    self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                    return self.stream_chat_completions(&request).await;
                }
            }

            if is_rate_limit_error(status.as_u16(), &error_text) {
                return Err(parse_api_error("OpenAI", status, &error_text));
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(status, &error_text, &headers, "Responses API error", Some(&client_request_id)),
            );
            return Err(provider::LLMError::Provider { message: formatted_error, metadata: None });
        }
    }

    async fn stream_legacy_via_responses_websocket(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        let mut normalized = self.stream_via_responses_websocket(request, true).await?;
        let model = request.model.clone();
        Ok(Box::pin(async_stream::try_stream! {
            while let Some(event) = normalized.next().await {
                match event? {
                    provider::NormalizedStreamEvent::TextDelta { delta } => yield provider::LLMStreamEvent::Token { delta },
                    provider::NormalizedStreamEvent::ReasoningDelta { delta, .. } => yield provider::LLMStreamEvent::Reasoning { delta },
                    provider::NormalizedStreamEvent::ReasoningStage { stage } => yield provider::LLMStreamEvent::ReasoningStage { stage },
                    provider::NormalizedStreamEvent::Done { response } => yield provider::LLMStreamEvent::Completed {
                        response: Box::new(Self::normalize_reasoning_output(&model, *response)),
                    },
                    provider::NormalizedStreamEvent::ToolCallStart { .. } | provider::NormalizedStreamEvent::ToolCallDelta { .. } | provider::NormalizedStreamEvent::Usage { .. } => {}
                }
            }
        }))
    }

    fn responses_normalized_stream(
        &self,
        response: reqwest::Response,
        model: String,
        include_metrics: bool,
    ) -> LLMNormalizedStream {
        let parse_model = model.clone();
        create_responses_normalized_stream(
            response,
            ResponsesNormalizedStreamOptions {
                provider_name: "OpenAI",
                emit_reasoning: Self::model_supports_reasoning_summaries(&model),
                model,
                include_cached_prompt_metrics: include_metrics,
            },
            move |value| {
                let response = parse_responses_completion(value, parse_model.clone(), include_metrics)?;
                Ok(Self::normalize_reasoning_output(&parse_model, response))
            },
        )
    }

    async fn send_http_streaming_request(
        &self,
        request: &provider::LLMRequest,
        payload: &Value,
        responses: bool,
    ) -> Result<StreamingHttpResponse, provider::LLMError> {
        let (url, api) = if responses {
            (&self.responses_url[..], "Responses")
        } else {
            (&self.chat_completions_url[..], "Chat Completions")
        };
        let client_request_id = Self::new_client_request_id();
        let response = self
            .send_authorized(|auth| {
                let builder = self.authorize_with_api_key(self.http_client.post(url), auth);
                let builder = if responses {
                    headers::apply_responses_beta(builder)
                } else {
                    builder
                };
                headers::apply_turn_metadata(
                    headers::apply_client_request_id(builder, &client_request_id),
                    &request.metadata,
                )
                .json(payload)
            })
            .await?;
        let result = StreamingHttpResponse::from_response(response, client_request_id).await;
        if let StreamingHttpResponse::Failure(failure) = &result
            && payload_uses_flex_service_tier(payload)
            && is_flex_service_tier_unsupported(failure.status, &failure.error_text)
        {
            self.mark_flex_unsupported_for_model(&request.model);
            log_service_tier_rejection(
                api,
                &request.model,
                &failure.client_request_id,
                payload_service_tier(payload).unwrap_or("flex"),
            );
            // Downgrade once; classify a failed retry through the caller's usual fallback path.
            let (response, request_id) = self
                .retry_without_service_tier(url, &request.metadata, payload, responses)
                .await?;
            return Ok(StreamingHttpResponse::from_response(response, request_id).await);
        }
        Ok(result)
    }

    async fn stream_chat_completions(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        let model = request.model.clone();
        let response = self.send_chat_completions_stream(request).await?;
        Ok(stream_decoder::create_chat_stream(response, model))
    }

    async fn stream_chat_completions_normalized(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<LLMNormalizedStream, provider::LLMError> {
        let model = request.model.clone();
        let response = self.send_chat_completions_stream(request).await?;
        Ok(stream_decoder::create_chat_normalized_stream(response, model))
    }

    async fn send_chat_completions_stream(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<reqwest::Response, provider::LLMError> {
        let mut openai_request = self.convert_to_openai_format(request)?;
        openai_request["stream"] = Value::Bool(true);
        // Request usage stats in the stream (compatible with newer OpenAI models)
        // Note: Some proxies do not support stream_options and will return 400.
        if self.is_native_openai_api() {
            openai_request["stream_options"] = json!({ "include_usage": true });
        }
        let openai_request = self.maybe_strip_flex_service_tier(&request.model, &openai_request);
        let StreamingHttpFailure { status, headers, error_text, client_request_id } =
            match self.send_http_streaming_request(request, &openai_request, false).await? {
                StreamingHttpResponse::Success(response) => return Ok(response),
                StreamingHttpResponse::Failure(failure) => failure,
            };

        if is_rate_limit_error(status.as_u16(), &error_text) {
            return Err(parse_api_error("OpenAI", status, &error_text));
        }

        let formatted_error = error_display::format_llm_error(
            "OpenAI",
            &format_openai_error(status, &error_text, &headers, "Chat Completions error", Some(&client_request_id)),
        );
        Err(provider::LLMError::Provider { message: formatted_error, metadata: None })
    }
}

#[cfg(test)]
mod tests {
    use super::{ResponsesApiState, should_prefer_responses_stream};

    #[test]
    fn streaming_prefers_responses_for_allowed_and_required() {
        assert!(should_prefer_responses_stream(ResponsesApiState::Allowed));
        assert!(should_prefer_responses_stream(ResponsesApiState::Required));
        assert!(!should_prefer_responses_stream(ResponsesApiState::Disabled));
    }
}
