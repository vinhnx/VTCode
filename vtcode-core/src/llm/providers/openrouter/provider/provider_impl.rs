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
        let response = self.send_with_tool_fallback(&request, Some(false)).await?;

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

        let include_cache_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.report_savings;
        response_parser::parse_response(response_json, model, include_cache_metrics)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let model = request.model.clone();
        let response = self.send_with_tool_fallback(&request, Some(true)).await?;

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
                                        // Handle dedicated reasoning field (e.g. reasoning_content or reasoning)
                                        if let Some(reasoning) = delta
                                            .get("reasoning_content")
                                            .or_else(|| delta.get("reasoning"))
                                            .and_then(|v| v.as_str())
                                        {
                                            if let Some(delta) = aggregator.handle_reasoning(reasoning) {
                                                yield LLMStreamEvent::Reasoning { delta };
                                            }
                                        }

                                        // Handle standard content field
                                        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                            for ev in aggregator.handle_content(content) {
                                                yield ev;
                                            }
                                        }

                                        // Handle structured reasoning_details field
                                        if let Some(reasoning_details) = delta
                                            .get("reasoning_details")
                                            .and_then(|v| v.as_array())
                                        {
                                            // Extract new reasoning text from structured details
                                            let prev_reasoning = aggregator.reasoning.clone();
                                            aggregator.set_reasoning_details(reasoning_details);

                                            // If reasoning text grew, yield the delta
                                            if let Some(new_reasoning) = crate::llm::providers::common::extract_reasoning_text_from_detail_values(reasoning_details) {
                                                if new_reasoning.len() > prev_reasoning.len() {
                                                    let delta = new_reasoning[prev_reasoning.len()..].to_string();
                                                    if !delta.trim().is_empty() {
                                                        yield LLMStreamEvent::Reasoning { delta };
                                                    }
                                                }
                                            }
                                        }

                                        // Handle tool calls in deltas
                                        if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                            aggregator.handle_tool_calls(tool_calls);
                                        }
                                    }

                                    if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                        aggregator.set_finish_reason(crate::llm::providers::common::map_finish_reason_common(finish_reason));
                                    }
                                }
                            }

                            if let Some(usage) = crate::llm::providers::common::parse_usage_openai_format(&payload, true) {
                                aggregator.set_usage(usage);
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
