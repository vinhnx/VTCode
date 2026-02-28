use super::super::errors::{
    fallback_model_if_not_found, format_openai_error, is_model_not_found,
    is_responses_api_unsupported,
};
use super::super::headers;
use super::super::types::ResponsesApiState;
use super::OpenAIProvider;
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::provider::LLMProvider;
use crate::llm::providers::error_handling::is_rate_limit_error;
use serde_json::Value;
use tracing::debug;

#[cfg(debug_assertions)]
use std::time::Instant;

#[inline]
fn should_attempt_responses_api(state: ResponsesApiState) -> bool {
    !matches!(state, ResponsesApiState::Disabled)
}

impl OpenAIProvider {
    pub(crate) async fn generate_request(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let mut request = request;

        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
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
            if self.websocket_mode_enabled(&request.model) {
                match self.generate_via_responses_websocket(&request).await {
                    Ok(response) => return Ok(response),
                    Err(err) => {
                        #[cfg(debug_assertions)]
                        debug!(
                            target = "vtcode::llm::openai",
                            model = %request.model,
                            error = %err,
                            "WebSocket mode failed; falling back to HTTP Responses API"
                        );
                    }
                }
            }

            let openai_request = self.convert_to_openai_responses_format(&request)?;
            let url = format!("{}/responses", self.base_url);

            let response = headers::apply_turn_metadata(
                headers::apply_responses_beta(self.authorize(self.http_client.post(&url))),
                &request.metadata,
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
                    return self.generate_chat_completions(&request).await;
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
                            let retry_response = headers::apply_turn_metadata(
                                headers::apply_responses_beta(
                                    self.authorize(self.http_client.post(&url)),
                                ),
                                &request.metadata,
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
                                let response = self.parse_openai_responses_response(
                                    openai_response,
                                    model.clone(),
                                )?;
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

                let response =
                    self.parse_openai_responses_response(openai_response, model.clone())?;
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

        self.generate_chat_completions(&request).await
    }

    async fn generate_chat_completions(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        #[cfg(debug_assertions)]
        let request_timer = Instant::now();
        let model = request.model.clone();
        let openai_request = self.convert_to_openai_format(request)?;
        let url = format!("{}/chat/completions", self.base_url);

        let response = headers::apply_turn_metadata(
            self.authorize(self.http_client.post(&url)),
            &request.metadata,
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

        let response = self.parse_openai_response(openai_response, model.clone())?;
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
}

#[cfg(test)]
mod tests {
    use super::{ResponsesApiState, should_attempt_responses_api};

    #[test]
    fn responses_attempt_logic_prefers_responses_for_allowed_and_required() {
        assert!(should_attempt_responses_api(ResponsesApiState::Allowed));
        assert!(should_attempt_responses_api(ResponsesApiState::Required));
        assert!(!should_attempt_responses_api(ResponsesApiState::Disabled));
    }
}
