use crate::llm_modular::client::LLMClient;
use crate::llm_modular::types::{BackendKind, LLMError, LLMResponse, Usage};
use async_trait::async_trait;
use reqwest;
use serde_json::{Value, json};

/// xAI Grok provider for the modular LLM client
pub struct XAIProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
    base_url: String,
}

impl XAIProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
            base_url: "https://api.x.ai/v1".to_string(),
        }
    }
}

#[async_trait]
impl LLMClient for XAIProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request_body = json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": prompt
            }],
            "temperature": 0.7
        });

        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| LLMError::ApiError(format!("Failed to send request: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LLMError::ApiError(format!("API error: {}", error_text)));
        }

        let response_json: Value = response
            .json()
            .await
            .map_err(|e| LLMError::ApiError(format!("Failed to parse response: {}", e)))?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = response_json["usage"].as_object().map(|usage_obj| Usage {
            prompt_tokens: usage_obj
                .get("prompt_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as usize,
            completion_tokens: usage_obj
                .get("completion_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as usize,
            total_tokens: usage_obj
                .get("total_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or(0) as usize,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        });

        Ok(LLMResponse {
            content,
            model: self.model.clone(),
            usage,
            reasoning: None,
        })
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::XAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
