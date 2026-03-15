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
use crate::llm::providers::error_handling::{is_rate_limit_error, parse_api_error};
use serde_json::{Value, json};

#[inline]
fn should_prefer_responses_stream(state: ResponsesApiState) -> bool {
    !matches!(state, ResponsesApiState::Disabled)
}

impl OpenAIProvider {
    pub(crate) async fn stream_request(
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

        let prefer_responses_stream = should_prefer_responses_stream(responses_state);

        if !prefer_responses_stream {
            return self.stream_chat_completions(&request).await;
        }

        let url = format!("{}/responses", self.base_url);
        loop {
            let model = request.model.clone();
            let include_metrics =
                self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
            let mut openai_request = self.convert_to_openai_responses_format(&request)?;
            openai_request["stream"] = Value::Bool(true);
            let client_request_id = Self::new_client_request_id();

            let response = self
                .send_authorized(|auth| {
                    headers::apply_turn_metadata(
                        headers::apply_client_request_id(
                            headers::apply_responses_beta(
                                self.authorize_with_api_key(self.http_client.post(&url), auth),
                            ),
                            &client_request_id,
                        ),
                        &request.metadata,
                    )
                    .json(&openai_request)
                })
                .await?;

            if response.status().is_success() {
                return Ok(stream_decoder::create_responses_stream(
                    response,
                    model,
                    include_metrics,
                    None,
                    None,
                ));
            }

            let status = response.status();
            let headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();

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
                return Err(provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                });
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
                &format_openai_error(
                    status,
                    &error_text,
                    &headers,
                    "Responses API error",
                    Some(&client_request_id),
                ),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }
    }

    async fn stream_chat_completions(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<provider::LLMStream, provider::LLMError> {
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
        let client_request_id = Self::new_client_request_id();

        let response = self
            .send_authorized(|auth| {
                headers::apply_turn_metadata(
                    headers::apply_client_request_id(
                        self.authorize_with_api_key(self.http_client.post(&url), auth),
                        &client_request_id,
                    ),
                    &request.metadata,
                )
                .json(&openai_request)
            })
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();

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
                    Some(&client_request_id),
                ),
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        Ok(stream_decoder::create_chat_stream(response, model))
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
