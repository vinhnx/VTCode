use super::super::errors::{
    fallback_model_if_not_found, format_openai_error, is_model_not_found,
    is_responses_api_unsupported,
};
use super::super::headers;
use super::super::stream_decoder;
use super::super::types::ResponsesApiState;
use super::OpenAIProvider;
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::provider::LLMProvider;
use crate::llm::providers::error_handling::is_rate_limit_error;
use async_stream::try_stream;
use serde_json::{Value, json};
use tracing::debug;

impl OpenAIProvider {
    pub(crate) async fn stream_request(
        &self,
        mut request: provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
        }
        let model = request.model.clone();

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        let responses_state = self.responses_api_state(&request.model);

        let prefer_responses_stream = matches!(responses_state, ResponsesApiState::Required)
            || (matches!(responses_state, ResponsesApiState::Allowed)
                && request.tools.as_ref().is_none_or(Vec::is_empty));

        if !prefer_responses_stream {
            return self.stream_chat_completions(&request).await;
        }

        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;

        let mut openai_request = self.convert_to_openai_responses_format(&request)?;

        openai_request["stream"] = Value::Bool(true);
        #[cfg(debug_assertions)]
        let debug_model = Some(request.model.clone());
        #[cfg(not(debug_assertions))]
        let debug_model: Option<String> = None;
        #[cfg(debug_assertions)]
        let request_timer = Some(std::time::Instant::now());
        #[cfg(not(debug_assertions))]
        let request_timer: Option<std::time::Instant> = None;
        #[cfg(debug_assertions)]
        {
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                stream = true,
                messages = request.messages.len(),
                tools = tool_count,
                "Dispatching streaming Responses request"
            );
        }

        let url = format!("{}/responses", self.base_url);

        let response = headers::apply_responses_beta(self.authorize(self.http_client.post(&url)))
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                provider::LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();

            if matches!(responses_state, ResponsesApiState::Allowed)
                && is_responses_api_unsupported(status, &error_text)
            {
                #[cfg(debug_assertions)]
                debug!(
                    target = "vtcode::llm::openai",
                    model = %request.model,
                    "Responses API unsupported; falling back to Chat Completions for streaming"
                );
                self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                return self.stream_chat_completions(&request).await;
            }

            if is_rate_limit_error(status.as_u16(), &error_text) {
                return Err(provider::LLMError::RateLimit { metadata: None });
            }

            if is_model_not_found(status, &error_text) {
                if let Some(fallback_model) = fallback_model_if_not_found(&request.model)
                    && fallback_model != request.model
                {
                    #[cfg(debug_assertions)]
                    debug!(
                        target = "vtcode::llm::openai",
                        requested = %request.model,
                        fallback = %fallback_model,
                        "Model not found while streaming; retrying with fallback"
                    );
                    let mut retry_request = request.clone();
                    retry_request.model = fallback_model;
                    retry_request.stream = false;
                    let response = self.generate_request(retry_request).await?;
                    let stream = try_stream! {
                        yield provider::LLMStreamEvent::Completed { response: Box::new(response) };
                    };
                    return Ok(Box::pin(stream));
                }
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format_openai_error(status, &error_text, &headers, "Model not available"),
                );
                return Err(provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                });
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format_openai_error(status, &error_text, &headers, "Responses API error"),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        #[cfg(debug_assertions)]
        {
            if let Some(ref debug_model) = debug_model {
                if let Some(request_timer) = request_timer.as_ref() {
                    debug!(
                        target = "vtcode::llm::openai",
                        model = %debug_model,
                        status = %response.status(),
                        handshake_ms = request_timer.elapsed().as_millis(),
                        "Streaming response headers received"
                    );
                }
            }
        }

        Ok(stream_decoder::create_responses_stream(
            response,
            model,
            include_metrics,
            debug_model,
            request_timer,
        ))
    }

    async fn stream_chat_completions(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openai",
            model = %request.model,
            "Using standard Chat Completions for streaming"
        );
        let model = request.model.clone();
        let mut openai_request = self.convert_to_openai_format(request)?;
        openai_request["stream"] = Value::Bool(true);
        // Request usage stats in the stream (compatible with newer OpenAI models)
        // Note: Some proxies do not support stream_options and will return 400.
        let is_native_openai = self.base_url.contains("api.openai.com");
        if is_native_openai {
            openai_request["stream_options"] = json!({ "include_usage": true });
        }
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .authorize(self.http_client.post(&url))
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                provider::LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if is_rate_limit_error(status.as_u16(), &error_text) {
                return Err(provider::LLMError::RateLimit { metadata: None });
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        Ok(stream_decoder::create_chat_stream(response, model))
    }
}
