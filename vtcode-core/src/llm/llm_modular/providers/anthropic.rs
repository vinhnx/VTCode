use crate::llm_modular::client::LLMClient;
use crate::llm_modular::types::{BackendKind, LLMResponse, LLMError, Usage};
use crate::config::constants::models;
use async_trait::async_trait;
use reqwest;
use serde_json::{Value, json};

/// Anthropic LLM provider
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMClient for AnthropicProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request_body = json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": prompt
            }],
            "max_tokens": 1024,
            "temperature": 0.7,
            "thinking": {
                "type": models::anthropic::INTERLEAVED_THINKING_TYPE_ENABLED,
                "budget_tokens": models::anthropic::INTERLEAVED_THINKING_BUDGET_TOKENS
            }
        });

        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", models::anthropic::INTERLEAVED_THINKING_BETA)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| LLMError::ApiError(format!("Failed to send request: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LLMError::ApiError(format!("API error: {}", error_text)));
        }

        let response_json: Value = response
            .json()
            .await
            .map_err(|e| LLMError::ApiError(format!("Failed to parse response: {}", e)))?;

        let content = response_json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = response_json["usage"].as_object().map(|usage_obj| {
            let cache_creation_tokens = usage_obj
                .get("cache_creation_input_tokens")
                .and_then(|value| value.as_u64())
                .map(|value| value as usize);
            let cache_read_tokens = usage_obj
                .get("cache_read_input_tokens")
                .and_then(|value| value.as_u64())
                .map(|value| value as usize);

            Usage {
                prompt_tokens: usage_obj
                    .get("input_tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0) as usize,
                completion_tokens: usage_obj
                    .get("output_tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0) as usize,
                total_tokens: usage_obj
                    .get("input_tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0) as usize
                    + usage_obj
                        .get("output_tokens")
                        .and_then(|t| t.as_u64())
                        .unwrap_or(0) as usize,
                cached_prompt_tokens: cache_read_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            }
        });

        Ok(LLMResponse {
            content,
            model: self.model.clone(),
            usage,
            reasoning: None,
        })
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::Anthropic
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}