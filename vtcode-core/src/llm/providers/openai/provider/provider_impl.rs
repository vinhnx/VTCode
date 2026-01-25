use super::OpenAIProvider;
use super::super::errors::{
    fallback_model_if_not_found, format_openai_error, is_model_not_found,
    is_responses_api_unsupported,
};
use super::super::headers;
use super::super::stream_decoder;
use super::super::types::ResponsesApiState;
use crate::config::constants::models;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider as provider;
use crate::llm::providers::error_handling::is_rate_limit_error;
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use serde_json::{Value, json};
#[cfg(debug_assertions)]
use std::time::Instant;
use tracing::debug;

#[async_trait]
impl provider::LLMProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        // OpenAI requires ID verification for GPT-5 models, so we must disable streaming
        if matches!(
            self.model.as_ref(),
            models::openai::GPT_5
                | models::openai::GPT_5_CODEX
                | models::openai::GPT_5_MINI
                | models::openai::GPT_5_NANO
        ) {
            return false;
        }

        // Even if Responses API is disabled (e.g., Hugging Face router), we can stream via Chat Completions.
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };

        models::openai::REASONING_MODELS.contains(&requested)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };
        models::openai::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    fn supports_tools(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_ref()
        } else {
            model
        };

        !models::openai::TOOL_UNAVAILABLE_MODELS.contains(&requested)
    }

    async fn stream(
        &self,
        mut request: provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
        }

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        let responses_state = self.responses_api_state(&request.model);

        let prefer_responses_stream = matches!(responses_state, ResponsesApiState::Required)
            || (matches!(responses_state, ResponsesApiState::Allowed)
                && request.tools.as_ref().is_none_or(Vec::is_empty));

        if !prefer_responses_stream {
            #[cfg(debug_assertions)]
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                "Using standard Chat Completions for streaming"
            );
            let mut openai_request = self.convert_to_openai_format(&request)?;
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

            return Ok(stream_decoder::create_chat_stream(response));
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
                request.stream = true;
                return self.stream(request).await;
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
                    let response = self.generate(retry_request).await?;
                    let stream = try_stream! {
                        yield provider::LLMStreamEvent::Completed { response };
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
            include_metrics,
            debug_model,
            request_timer,
        ))
    }

    async fn generate(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let mut request = request;

        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
        }

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        // Check if this is a harmony model (GPT-OSS)
        if Self::uses_harmony(&request.model) {
            return self.generate_with_harmony(request).await;
        }

        let responses_state = self.responses_api_state(&request.model);
        let attempt_responses = !matches!(responses_state, ResponsesApiState::Disabled)
            && (matches!(responses_state, ResponsesApiState::Required)
                || request.tools.as_ref().is_none_or(Vec::is_empty));
        #[cfg(debug_assertions)]
        let request_timer = Instant::now();
        #[cfg(debug_assertions)]
        {
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                responses_api = attempt_responses,
                messages = request.messages.len(),
                tools = tool_count,
                "Dispatching non-streaming OpenAI request"
            );
        }

        if attempt_responses {
            let openai_request = self.convert_to_openai_responses_format(&request)?;
            let url = format!("{}/responses", self.base_url);

            let response = headers::apply_responses_beta(
                self.authorize(self.http_client.post(&url)),
            )
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
                        "Responses API unsupported; falling back to Chat Completions"
                    );
                    self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                    return self.generate(request).await;
                } else if is_rate_limit_error(status.as_u16(), &error_text) {
                    return Err(provider::LLMError::RateLimit { metadata: None });
                } else if is_model_not_found(status, &error_text) {
                    if let Some(fallback_model) = fallback_model_if_not_found(&request.model) {
                        if fallback_model != request.model {
                            #[cfg(debug_assertions)]
                            debug!(
                                target = "vtcode::llm::openai",
                                requested = %request.model,
                                fallback = %fallback_model,
                                "Model not found; retrying with fallback"
                            );
                            let mut retry_request = request.clone();
                            retry_request.model = fallback_model;
                            let retry_openai =
                                self.convert_to_openai_responses_format(&retry_request)?;
                            let retry_response = headers::apply_responses_beta(
                                self.authorize(self.http_client.post(&url)),
                            )
                                .json(&retry_openai)
                                .send()
                                .await
                                .map_err(|e| {
                                    let formatted_error = error_display::format_llm_error(
                                        "OpenAI",
                                        &format!("Network error: {}", e),
                                    );
                                    provider::LLMError::Network {
                                        message: formatted_error,
                                        metadata: None,
                                    }
                                })?;
                            if retry_response.status().is_success() {
                                let openai_response: Value =
                                    retry_response.json().await.map_err(|e| {
                                        let formatted_error = error_display::format_llm_error(
                                            "OpenAI",
                                            &format!("Failed to parse response: {}", e),
                                        );
                                        provider::LLMError::Provider {
                                            message: formatted_error,
                                            metadata: None,
                                        }
                                    })?;
                                let response =
                                    self.parse_openai_responses_response(openai_response)?;
                                return Ok(response);
                            }
                        }
                    }
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format_openai_error(status, &error_text, &headers, "Model not available"),
                    );
                    return Err(provider::LLMError::Provider {
                        message: formatted_error,
                        metadata: None,
                    });
                } else {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format_openai_error(status, &error_text, &headers, "Responses API error"),
                    );
                    return Err(provider::LLMError::Provider {
                        message: formatted_error,
                        metadata: None,
                    });
                }
            } else {
                let openai_response: Value = response.json().await.map_err(|e| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format!("Failed to parse response: {}", e),
                    );
                    provider::LLMError::Provider {
                        message: formatted_error,
                        metadata: None,
                    }
                })?;

                let response = self.parse_openai_responses_response(openai_response)?;
                #[cfg(debug_assertions)]
                {
                    let content_len = response.content.as_ref().map_or(0, |c| c.len());
                    debug!(
                        target = "vtcode::llm::openai",
                        model = %request.model,
                        responses_api = true,
                        elapsed_ms = request_timer.elapsed().as_millis(),
                        content_len = content_len,
                        finish_reason = ?response.finish_reason,
                        "Completed non-streaming OpenAI request"
                    );
                }
                return Ok(response);
            }
        } else {
            #[cfg(debug_assertions)]
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                "Skipping Responses API (disabled); using Chat Completions"
            );
        }

        let openai_request = self.convert_to_openai_format(&request)?;
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

        let openai_response: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to parse response: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        let response = self.parse_openai_response(openai_response)?;
        #[cfg(debug_assertions)]
        {
            let content_len = response.content.as_ref().map_or(0, |c| c.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                responses_api = false,
                elapsed_ms = request_timer.elapsed().as_millis(),
                content_len = content_len,
                finish_reason = ?response.finish_reason,
                "Completed non-streaming OpenAI request"
            );
        }
        Ok(response)
    }

    fn supported_models(&self) -> Vec<String> {
        models::openai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &provider::LLMRequest) -> Result<(), provider::LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "Messages cannot be empty");
            return Err(provider::LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        if !models::openai::SUPPORTED_MODELS
            .iter()
            .any(|m| *m == request.model)
        {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(provider::LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("OpenAI", &err);
                return Err(provider::LLMError::InvalidRequest {
                    message: formatted,
                    metadata: None,
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for OpenAIProvider {
    async fn generate(
        &mut self,
        prompt: &str,
    ) -> Result<llm_types::LLMResponse, provider::LLMError> {
        let request = self.parse_client_prompt(prompt);
        let request_model = request.model.to_string();
        let response = provider::LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: request_model,
            usage: response
                .usage
                .map(crate::llm::providers::common::convert_usage_to_llm_types),
            reasoning: response.reasoning,
            reasoning_details: response.reasoning_details,
            request_id: response.request_id,
            organization_id: response.organization_id,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::OpenAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
