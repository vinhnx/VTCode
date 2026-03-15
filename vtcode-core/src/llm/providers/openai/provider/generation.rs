use super::super::errors::{
    fallback_model_if_not_found, format_openai_error, is_model_not_found,
    is_responses_api_unsupported,
};
use super::super::headers;
use super::super::responses_api::build_standard_responses_payload;
use super::super::types::ResponsesApiState;
use super::OpenAIProvider;
use super::websocket::is_websocket_connection_limit_error;
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::provider::LLMProvider;
use crate::llm::providers::error_handling::{is_rate_limit_error, parse_api_error};
use crate::llm::providers::shared::parse_compacted_output_messages;
use futures::StreamExt;
use serde_json::{Value, json};

#[inline]
fn should_attempt_responses_api(state: ResponsesApiState) -> bool {
    !matches!(state, ResponsesApiState::Disabled)
}

async fn collect_streamed_response(
    stream: provider::LLMStream,
) -> Result<provider::LLMResponse, provider::LLMError> {
    let mut stream = stream;
    let mut streamed_content = String::new();
    let mut streamed_reasoning = String::new();
    let mut completed = None;

    while let Some(event) = stream.next().await {
        match event? {
            provider::LLMStreamEvent::Token { delta } => streamed_content.push_str(&delta),
            provider::LLMStreamEvent::Reasoning { delta } => streamed_reasoning.push_str(&delta),
            provider::LLMStreamEvent::ReasoningStage { .. } => {}
            provider::LLMStreamEvent::Completed { response } => {
                completed = Some(*response);
                break;
            }
        }
    }

    let mut response = completed.ok_or_else(|| provider::LLMError::Provider {
        message: error_display::format_llm_error(
            "OpenAI",
            "Streaming response ended without a completion event",
        ),
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
    pub(crate) async fn compact_history_request(
        &self,
        model: &str,
        history: &[provider::Message],
    ) -> Result<Vec<provider::Message>, provider::LLMError> {
        let resolved_model = if model.trim().is_empty() {
            self.model.to_string()
        } else {
            model.trim().to_string()
        };

        let request = provider::LLMRequest {
            model: resolved_model.clone(),
            messages: history.to_vec(),
            ..Default::default()
        };
        let responses_payload = build_standard_responses_payload(&request, true)?;
        if responses_payload.input.is_empty() {
            return Ok(history.to_vec());
        }

        let mut compact_payload = json!({
            "model": resolved_model,
            "input": responses_payload.input,
        });
        if let Some(instructions) = responses_payload.instructions
            && let Value::Object(ref mut map) = compact_payload
        {
            map.insert("instructions".to_string(), json!(instructions));
        }
        let url = format!("{}/responses/compact", self.base_url);
        let client_request_id = Self::new_client_request_id();

        let response = self
            .send_authorized(|auth| {
                headers::apply_client_request_id(
                    headers::apply_responses_beta(
                        self.authorize_with_api_key(self.http_client.post(&url), auth),
                    ),
                    &client_request_id,
                )
                .json(&compact_payload)
            })
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let headers = response.headers().clone();
            let error_text = response.text().await.unwrap_or_default();
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
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to parse compaction response: {}", e),
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;
        let output = response_json
            .get("output")
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    "Invalid compaction response format: missing output array",
                );
                provider::LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let compacted = parse_compacted_output_messages(output);
        if compacted.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "Compaction response contained no reusable messages",
            );
            return Err(provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        Ok(compacted)
    }

    pub(crate) async fn generate_request(
        &self,
        request: provider::LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let mut request = request;

        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
        }

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
                match self.generate_via_responses_websocket(&request).await {
                    Ok(response) => return Ok(response),
                    Err(err) => {
                        if is_websocket_connection_limit_error(&err) {
                            match self.generate_via_responses_websocket(&request).await {
                                Ok(response) => return Ok(response),
                                Err(_retry_err) => {}
                            }
                        }
                        let _ = err;
                    }
                }
            }

            let openai_request = self.convert_to_openai_responses_format(&request)?;
            let url = format!("{}/responses", self.base_url);
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

            if !response.status().is_success() {
                let status = response.status();
                let headers = response.headers().clone();
                let error_text = response.text().await.unwrap_or_default();

                if is_model_not_found(status, &error_text) {
                    if let Some(fallback_model) = fallback_model_if_not_found(&request.model) {
                        if fallback_model != request.model {
                            let mut retry_request = request.clone();
                            retry_request.model = fallback_model;
                            let retry_openai =
                                self.convert_to_openai_responses_format(&retry_request)?;
                            let retry_client_request_id = Self::new_client_request_id();
                            let retry_response = self
                                .send_authorized(|auth| {
                                    headers::apply_turn_metadata(
                                        headers::apply_client_request_id(
                                            headers::apply_responses_beta(
                                                self.authorize_with_api_key(
                                                    self.http_client.post(&url),
                                                    auth,
                                                ),
                                            ),
                                            &retry_client_request_id,
                                        ),
                                        &request.metadata,
                                    )
                                    .json(&retry_openai)
                                })
                                .await?;
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
                            Some(&client_request_id),
                        ),
                    );
                    return Err(provider::LLMError::Provider {
                        message: formatted_error,
                        metadata: None,
                    });
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
                            Some(&client_request_id),
                        ),
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
