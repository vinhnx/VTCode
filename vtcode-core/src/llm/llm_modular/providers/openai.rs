use crate::llm_modular::client::LLMClient;
use crate::llm_modular::types::{BackendKind, LLMError, LLMResponse, Usage};
use async_trait::async_trait;
use reqwest;
use serde_json::{json, Value};

/// OpenAI LLM provider
pub struct OpenAIProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMClient for OpenAIProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        // GPT-5.1 and the GPT-5 family use the Responses API and do not
        // support temperature/top_p/logprobs. We use reasoning + verbosity
        // controls instead and send a simple text input.

        let request_body = json!({
            "model": self.model,
            "input": prompt,
            "reasoning": {
                "effort": "none"
            },
            "text": {
                "verbosity": "medium"
            }
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
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

        // Responses API: primary text output is exposed via `output_text`.
        // Fall back to items[0].output_text if needed.
        let content = response_json
            .get("output_text")
            .and_then(|v| v.as_str())
            .or_else(|| {
                response_json
                    .get("output")
                    .and_then(|o| o.get("items"))
                    .and_then(|items| items.get(0))
                    .and_then(|item| item.get("output_text"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("")
            .to_string();

        let usage = response_json["usage"].as_object().map(|usage_obj| {
            let cached_prompt_tokens = usage_obj
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .and_then(|value| value.as_u64())
                .map(|value| value as usize);

            Usage {
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
                cached_prompt_tokens,
                cache_creation_tokens: usage_obj
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize),
                cache_read_tokens: usage_obj
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize),
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
        BackendKind::OpenAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}