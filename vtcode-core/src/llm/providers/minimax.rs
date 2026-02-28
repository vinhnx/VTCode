#![allow(clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::Value;

use super::common::{override_base_url, resolve_model, serialize_message_content_openai};
use super::error_handling::{format_network_error, format_parse_error};

pub struct MinimaxProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: String,
    model_behavior: Option<ModelConfig>,
}

impl MinimaxProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::minimax::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(model, None, api_key, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            http_client,
            base_url,
            model,
            api_key,
            model_behavior: None,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let resolved_model = resolve_model(model, models::minimax::DEFAULT_MODEL);
        Self::with_model_internal(resolved_model, base_url, api_key_value, model_behavior)
    }

    fn with_model_internal(
        model: String,
        base_url: Option<String>,
        api_key: String,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let resolved_base = override_base_url(
            urls::MINIMAX_API_BASE,
            base_url,
            Some(env_vars::MINIMAX_BASE_URL),
        );

        Self {
            http_client: HttpClient::new(),
            base_url: normalize_openai_base_url(&resolved_base),
            model,
            api_key,
            model_behavior,
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn build_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        let mut messages = Vec::new();

        if let Some(system) = &request.system_prompt {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system
            }));
        }

        for message in &request.messages {
            messages.push(serde_json::json!({
                "role": message.role.as_generic_str(),
                "content": serialize_message_content_openai(&message.content)
            }));
        }

        Ok(serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": stream
        }))
    }
}

fn normalize_openai_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.minimax.io/v1".to_string();
    }

    if let Some(prefix) = trimmed.strip_suffix("/v1/messages") {
        let prefix = prefix.trim_end_matches('/');
        if let Some(no_anthropic) = prefix.strip_suffix("/anthropic") {
            return format!("{}/v1", no_anthropic.trim_end_matches('/'));
        }
        return format!("{}/v1", prefix);
    }
    if let Some(prefix) = trimmed.strip_suffix("/messages") {
        return format!("{}/v1", prefix.trim_end_matches('/'));
    }

    if let Some(prefix) = trimmed.strip_suffix("/anthropic/v1") {
        return format!("{}/v1", prefix.trim_end_matches('/'));
    }
    if let Some(prefix) = trimmed.strip_suffix("/anthropic") {
        return format!("{}/v1", prefix.trim_end_matches('/'));
    }

    if trimmed.ends_with("/v1") {
        return trimmed.to_string();
    }

    format!("{trimmed}/v1")
}

#[async_trait]
impl LLMProvider for MinimaxProvider {
    fn name(&self) -> &str {
        "minimax"
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

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();
        let payload = self.build_payload(&request, false)?;
        let url = self.chat_url();

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("MiniMax", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let formatted_error =
                error_display::format_llm_error("MiniMax", &format!("HTTP {}: {}", status, body));
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| format_parse_error("MiniMax", &e))?;

        let choice = json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .ok_or_else(|| LLMError::Provider {
                message: "Invalid response from MiniMax: missing choices".to_string(),
                metadata: None,
            })?;

        let message = choice.get("message").ok_or_else(|| LLMError::Provider {
            message: "Invalid response from MiniMax: missing message".to_string(),
            metadata: None,
        })?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        Ok(LLMResponse {
            content,
            tool_calls: None,
            model,
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();
        let payload = self.build_payload(&request, true)?;
        let url = self.chat_url();

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("MiniMax", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let formatted_error =
                error_display::format_llm_error("MiniMax", &format!("HTTP {}: {}", status, body));
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
                let chunk = chunk_result.map_err(|e| format_network_error("MiniMax", &e))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = crate::llm::providers::shared::find_sse_boundary(&buffer) {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);

                    if let Some(data_payload) = crate::llm::providers::shared::extract_data_payload(&event) {
                        let trimmed = data_payload.trim();
                        if trimmed.is_empty() || trimmed == "[DONE]" {
                            continue;
                        }

                        if let Ok(payload) = serde_json::from_str::<Value>(trimmed)
                            && let Some(choices) = payload.get("choices").and_then(|v| v.as_array())
                                && let Some(choice) = choices.first()
                                    && let Some(delta) = choice.get("delta")
                                        && let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                            for ev in aggregator.handle_content(content) {
                                                yield ev;
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
        models::minimax::SUPPORTED_MODELS
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

#[async_trait]
impl LLMClient for MinimaxProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Minimax
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_openai_base_url;

    #[test]
    fn normalize_minimax_anthropic_base_to_openai_v1() {
        assert_eq!(
            normalize_openai_base_url("https://api.minimax.io/anthropic"),
            "https://api.minimax.io/v1"
        );
    }

    #[test]
    fn normalize_minimax_anthropic_messages_base_to_openai_v1() {
        assert_eq!(
            normalize_openai_base_url("https://api.minimax.io/anthropic/v1/messages"),
            "https://api.minimax.io/v1"
        );
    }

    #[test]
    fn preserve_openai_v1_base() {
        assert_eq!(
            normalize_openai_base_url("https://api.minimax.io/v1"),
            "https://api.minimax.io/v1"
        );
    }
}
