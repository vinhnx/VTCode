use super::super::OpenRouterProvider;
use crate::llm::error_display;
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
};
use crate::llm::providers::error_handling::{format_network_error, format_parse_error};

use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::Value;

use super::super::response_parser;

#[async_trait]
impl LLMProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning)
            .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(false)
    }

    fn supports_tools(&self, model: &str) -> bool {
        use crate::config::constants::models;
        !models::openrouter::TOOL_UNAVAILABLE_MODELS.contains(&model)
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = request.model.clone();
        let openai_request = self.convert_to_openrouter_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://vtcode.dev")
            .header("X-Title", "VT Code")
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| format_network_error("OpenRouter", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let response_json: Value = response
            .json()
            .await
            .map_err(|e| format_parse_error("OpenRouter", &e))?;

        response_parser::parse_response(response_json, model)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let model = request.model.clone();
        let mut openai_request = self.convert_to_openrouter_format(&request)?;
        openai_request["stream"] = Value::Bool(true);

        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://vtcode.dev")
            .header("X-Title", "VT Code")
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| format_network_error("OpenRouter", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregator = crate::llm::providers::shared::StreamAggregator::new(model);

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|e| format_network_error("OpenRouter", &e))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = crate::llm::providers::shared::find_sse_boundary(&buffer) {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);

                    if let Some(data_payload) = crate::llm::providers::shared::extract_data_payload(&event) {
                        let trimmed = data_payload.trim();
                        if trimmed.is_empty() || trimmed == "[DONE]" {
                            continue;
                        }

                        if let Ok(payload) = serde_json::from_str::<Value>(trimmed) {
                            if let Some(choices) = payload.get("choices").and_then(|v| v.as_array()) {
                                if let Some(choice) = choices.first() {
                                    if let Some(delta) = choice.get("delta") {
                                        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                            for ev in aggregator.handle_content(content) {
                                                yield ev;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            yield LLMStreamEvent::Completed { response: Box::new(aggregator.finalize()) };
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        use crate::config::constants::models;
        models::openrouter::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            return Err(LLMError::InvalidRequest {
                message: "Messages cannot be empty".to_string(),
                metadata: None,
            });
        }
        Ok(())
    }
}
