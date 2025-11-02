use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{DeepSeekPromptCacheSettings, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageContent,
    MessageRole, ToolCall, ToolDefinition, Usage,
};
use crate::llm::types as llm_types;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value, json};

use super::{
    common::{extract_prompt_cache_settings, override_base_url, resolve_model},
    extract_reasoning_trace,
};

const PROVIDER_NAME: &str = "DeepSeek";
const PROVIDER_KEY: &str = "deepseek";

pub struct DeepSeekProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: DeepSeekPromptCacheSettings,
}

impl DeepSeekProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::deepseek::DEFAULT_MODEL.to_string(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::deepseek::DEFAULT_MODEL);

        Self::with_model_internal(api_key_value, model_value, prompt_cache, base_url)
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.deepseek,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: override_base_url(
                urls::DEEPSEEK_API_BASE,
                base_url,
                Some(env_vars::DEEPSEEK_BASE_URL),
            ),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn default_request(&self, prompt: &str) -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user(prompt.to_string())],
            system_prompt: None,
            tools: None,
            model: self.model.clone(),
            max_tokens: None,
            temperature: None,
            stream: false,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        }
    }

    fn parse_client_prompt(&self, prompt: &str) -> LLMRequest {
        let trimmed = prompt.trim_start();
        if trimmed.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                if let Some(request) = self.parse_chat_request(&value) {
                    return request;
                }
            }
        }

        self.default_request(prompt)
    }

    fn parse_chat_request(&self, value: &Value) -> Option<LLMRequest> {
        let messages_value = value.get("messages")?.as_array()?;
        let mut system_prompt = value
            .get("system")
            .and_then(|entry| entry.as_str())
            .map(|text| text.to_string());
        let mut messages = Vec::new();

        for entry in messages_value {
            let role = entry
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or(crate::config::constants::message_roles::USER);
            let content = entry
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string();

            match role {
                "system" => {
                    if system_prompt.is_none() && !content.is_empty() {
                        system_prompt = Some(content);
                    }
                }
                "assistant" => {
                    let tool_calls = entry
                        .get("tool_calls")
                        .and_then(|tc| tc.as_array())
                        .map(|calls| {
                            calls
                                .iter()
                                .filter_map(|call| Self::parse_tool_call(call))
                                .collect::<Vec<_>>()
                        })
                        .filter(|calls| !calls.is_empty());

                    messages.push(Message {
                        role: MessageRole::Assistant,
                        content: MessageContent::from(content),
                        reasoning: None,
                        reasoning_details: None,
                        tool_calls,
                        tool_call_id: None,
                    });
                }
                "tool" => {
                    if let Some(tool_call_id) = entry.get("tool_call_id").and_then(|v| v.as_str()) {
                        messages.push(Message::tool_response(tool_call_id.to_string(), content));
                    }
                }
                _ => {
                    messages.push(Message::user(content));
                }
            }
        }

        Some(LLMRequest {
            messages,
            system_prompt,
            model: value
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or(&self.model)
                .to_string(),
            max_tokens: value
                .get("max_tokens")
                .and_then(|m| m.as_u64())
                .map(|m| m as u32),
            temperature: value
                .get("temperature")
                .and_then(|t| t.as_f64())
                .map(|t| t as f32),
            stream: value
                .get("stream")
                .and_then(|s| s.as_bool())
                .unwrap_or(false),
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        })
    }

    fn parse_tool_call(value: &Value) -> Option<ToolCall> {
        let id = value.get("id").and_then(|v| v.as_str())?;
        let function = value.get("function")?.as_object()?;
        let name = function.get("name").and_then(|v| v.as_str())?;
        let arguments = function.get("arguments").map(|arg| match arg {
            Value::String(text) => text.to_string(),
            _ => arg.to_string(),
        });

        Some(ToolCall::function(
            id.to_string(),
            name.to_string(),
            arguments.unwrap_or_else(|| "{}".to_string()),
        ))
    }

    fn convert_to_deepseek_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::new();

        payload.insert("model".to_string(), Value::String(request.model.clone()));
        payload.insert(
            "messages".to_string(),
            Value::Array(self.serialize_messages(request)?),
        );

        if let Some(system_prompt) = &request.system_prompt {
            payload.insert(
                "system".to_string(),
                Value::String(system_prompt.trim().to_string()),
            );
        }

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                "max_tokens".to_string(),
                Value::Number(serde_json::Number::from(max_tokens as u64)),
            );
        }

        if let Some(temperature) = request.temperature {
            payload.insert(
                "temperature".to_string(),
                Value::Number(serde_json::Number::from_f64(temperature as f64).ok_or_else(
                    || LLMError::InvalidRequest("Invalid temperature value".to_string()),
                )?),
            );
        }

        if request.stream {
            payload.insert("stream".to_string(), Value::Bool(true));
        }

        if let Some(tools) = &request.tools {
            if let Some(serialized_tools) = Self::serialize_tools(tools) {
                payload.insert("tools".to_string(), Value::Array(serialized_tools));
            }
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_string(),
                choice.to_provider_format(PROVIDER_KEY),
            );
        }

        if let Some(effort) = request.reasoning_effort {
            payload.insert(
                "reasoning_effort".to_string(),
                Value::String(effort.as_str().to_string()),
            );
        }

        Ok(Value::Object(payload))
    }

    fn serialize_messages(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        let mut messages = Vec::with_capacity(request.messages.len());

        for message in &request.messages {
            message
                .validate_for_provider(PROVIDER_KEY)
                .map_err(LLMError::InvalidRequest)?;

            let mut message_map = Map::new();
            message_map.insert(
                "role".to_string(),
                Value::String(message.role.as_generic_str().to_string()),
            );
            message_map.insert(
                "content".to_string(),
                Value::String(message.content.as_text()),
            );

            if let Some(tool_calls) = &message.tool_calls {
                let serialized_calls = tool_calls
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id.clone(),
                            "type": "function",
                            "function": {
                                "name": call.function.name.clone(),
                                "arguments": call.function.arguments.clone()
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                message_map.insert("tool_calls".to_string(), Value::Array(serialized_calls));
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                message_map.insert(
                    "tool_call_id".to_string(),
                    Value::String(tool_call_id.clone()),
                );
            }

            messages.push(Value::Object(message_map));
        }

        Ok(messages)
    }

    fn serialize_tools(tools: &[ToolDefinition]) -> Option<Vec<Value>> {
        if tools.is_empty() {
            return None;
        }

        Some(tools.iter().map(|tool| json!(tool)).collect::<Vec<_>>())
    }

    fn parse_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        let choices = response_json
            .get("choices")
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    PROVIDER_NAME,
                    "Invalid response format: missing choices",
                );
                LLMError::Provider(formatted_error)
            })?;

        if choices.is_empty() {
            let formatted_error =
                error_display::format_llm_error(PROVIDER_NAME, "No choices in response");
            return Err(LLMError::Provider(formatted_error));
        }

        let choice = &choices[0];
        let message = choice.get("message").ok_or_else(|| {
            let formatted_error = error_display::format_llm_error(
                PROVIDER_NAME,
                "Invalid response format: missing message",
            );
            LLMError::Provider(formatted_error)
        })?;

        let content = message
            .get("content")
            .and_then(|value| match value {
                Value::String(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }
                Value::Array(parts) => Some(
                    parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .filter(|text| !text.is_empty());

        let tool_calls = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| Self::parse_tool_call(call))
                    .collect::<Vec<_>>()
            })
            .filter(|calls| !calls.is_empty());

        let reasoning = message
            .get("reasoning_content")
            .and_then(extract_reasoning_trace)
            .or_else(|| message.get("reasoning").and_then(extract_reasoning_trace))
            .or_else(|| {
                choice
                    .get("reasoning_content")
                    .and_then(extract_reasoning_trace)
            });

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|value| value.as_str())
            .map(|reason| match reason {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "tool_calls" => FinishReason::ToolCalls,
                other => FinishReason::Error(other.to_string()),
            })
            .unwrap_or(FinishReason::Stop);

        let usage = response_json.get("usage").map(|usage_value| Usage {
            prompt_tokens: usage_value
                .get("prompt_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: usage_value
                .get("completion_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
            cached_prompt_tokens: if self.prompt_cache_enabled
                && self.prompt_cache_settings.surface_metrics
            {
                usage_value
                    .get("prompt_cache_hit_tokens")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32)
            } else {
                None
            },
            cache_creation_tokens: if self.prompt_cache_enabled
                && self.prompt_cache_settings.surface_metrics
            {
                usage_value
                    .get("prompt_cache_miss_tokens")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32)
            } else {
                None
            },
            cache_read_tokens: None,
        });

        Ok(LLMResponse {
            content,
            tool_calls,
            usage,
            finish_reason,
            reasoning,
            reasoning_details: None,
        })
    }
}

#[async_trait]
impl LLMProvider for DeepSeekProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let target = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };
        target == models::deepseek::DEEPSEEK_REASONER
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut request = request;
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        let payload = self.convert_to_deepseek_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("Network error: {}", e),
                );
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                let formatted_error = error_display::format_llm_error(
                    PROVIDER_NAME,
                    "Authentication failed (check DEEPSEEK_API_KEY)",
                );
                return Err(LLMError::Authentication(formatted_error));
            }

            if status.as_u16() == 429 || error_text.contains("quota") {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        self.parse_response(response_json)
    }

    fn supported_models(&self) -> Vec<String> {
        models::deepseek::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        for message in &request.messages {
            message
                .validate_for_provider(PROVIDER_KEY)
                .map_err(LLMError::InvalidRequest)?;
        }
        Ok(())
    }
}

#[async_trait]
impl LLMClient for DeepSeekProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = self.parse_client_prompt(prompt);
        let model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model,
            usage: response.usage.map(|usage| llm_types::Usage {
                prompt_tokens: usage.prompt_tokens as usize,
                completion_tokens: usage.completion_tokens as usize,
                total_tokens: usage.total_tokens as usize,
                cached_prompt_tokens: usage.cached_prompt_tokens.map(|value| value as usize),
                cache_creation_tokens: usage.cache_creation_tokens.map(|value| value as usize),
                cache_read_tokens: usage.cache_read_tokens.map(|value| value as usize),
            }),
            reasoning: response.reasoning,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::DeepSeek
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
