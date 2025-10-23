use crate::config::constants::{env_vars, headers, models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole, ToolCall,
    ToolChoice, ToolDefinition, Usage,
};
use crate::llm::types as llm_types;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Value, json};
use std::collections::HashSet;

use super::common::{override_base_url, resolve_model};

const PROVIDER_NAME: &str = "Z.AI";
const PROVIDER_KEY: &str = "zai";
const CHAT_COMPLETIONS_PATH: &str = "/paas/v4/chat/completions";

pub struct ZAIProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
}

impl ZAIProvider {
    fn serialize_tools(tools: &[ToolDefinition]) -> Option<Value> {
        if tools.is_empty() {
            return None;
        }

        let serialized = tools
            .iter()
            .map(|tool| {
                json!({
                    "type": tool.tool_type,
                    "function": {
                        "name": tool.function.name,
                        "description": tool.function.description,
                        "parameters": tool.function.parameters,
                    }
                })
            })
            .collect::<Vec<Value>>();

        Some(Value::Array(serialized))
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: override_base_url(
                urls::Z_AI_API_BASE,
                base_url,
                Some(env_vars::Z_AI_BASE_URL),
            ),
            model,
        }
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(api_key, models::zai::DEFAULT_MODEL.to_string(), None, None)
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
        let model_value = resolve_model(model, models::zai::DEFAULT_MODEL);
        Self::with_model_internal(api_key_value, model_value, base_url, prompt_cache)
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
                .map(|c| match c {
                    Value::String(text) => text.to_string(),
                    other => other.to_string(),
                })
                .unwrap_or_default();

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
                        content,
                        reasoning: None,
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
            tool_choice: value.get("tool_choice").and_then(|choice| match choice {
                Value::String(s) => match s.as_str() {
                    "auto" => Some(ToolChoice::auto()),
                    "none" => Some(ToolChoice::none()),
                    "any" | "required" => Some(ToolChoice::any()),
                    _ => None,
                },
                _ => None,
            }),
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        })
    }

    fn parse_tool_call(value: &Value) -> Option<ToolCall> {
        let id = value.get("id").and_then(|v| v.as_str())?;
        let function = value.get("function")?;
        let name = function.get("name").and_then(|v| v.as_str())?;
        let arguments = function.get("arguments");
        let serialized = arguments.map_or("{}".to_string(), |value| {
            if value.is_string() {
                value.as_str().unwrap_or("").to_string()
            } else {
                value.to_string()
            }
        });

        Some(ToolCall::function(
            id.to_string(),
            name.to_string(),
            serialized,
        ))
    }

    fn convert_to_zai_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut messages = Vec::new();
        let mut active_tool_call_ids: HashSet<String> = HashSet::new();

        if let Some(system_prompt) = &request.system_prompt {
            messages.push(json!({
                "role": crate::config::constants::message_roles::SYSTEM,
                "content": system_prompt
            }));
        }

        for msg in &request.messages {
            let role = msg.role.as_generic_str();
            let mut message = json!({
                "role": role,
                "content": msg.content
            });
            let mut skip_message = false;

            if msg.role == MessageRole::Assistant {
                if let Some(tool_calls) = &msg.tool_calls {
                    if !tool_calls.is_empty() {
                        let tool_calls_json: Vec<Value> = tool_calls
                            .iter()
                            .map(|tc| {
                                active_tool_call_ids.insert(tc.id.clone());
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.function.name,
                                        "arguments": tc.function.arguments,
                                    }
                                })
                            })
                            .collect();
                        message["tool_calls"] = Value::Array(tool_calls_json);
                    }
                }
            }

            if msg.role == MessageRole::Tool {
                match &msg.tool_call_id {
                    Some(tool_call_id) if active_tool_call_ids.contains(tool_call_id) => {
                        message["tool_call_id"] = Value::String(tool_call_id.clone());
                        active_tool_call_ids.remove(tool_call_id);
                    }
                    Some(_) | None => {
                        skip_message = true;
                    }
                }
            }

            if !skip_message {
                messages.push(message);
            }
        }

        if messages.is_empty() {
            let formatted = error_display::format_llm_error(PROVIDER_NAME, "No messages provided");
            return Err(LLMError::InvalidRequest(formatted));
        }

        let mut payload = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream,
        });

        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(temperature);
        }

        if let Some(tools) = &request.tools {
            if let Some(serialized) = Self::serialize_tools(tools) {
                payload["tools"] = serialized;
            }
        }

        if let Some(choice) = &request.tool_choice {
            payload["tool_choice"] = choice.to_provider_format("openai");
        }

        if self.supports_reasoning(&request.model) || request.reasoning_effort.is_some() {
            payload["thinking"] = json!({ "type": "enabled" });
        }

        Ok(payload)
    }

    fn parse_zai_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        let choices = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                let formatted = error_display::format_llm_error(
                    PROVIDER_NAME,
                    "Invalid response format: missing choices",
                );
                LLMError::Provider(formatted)
            })?;

        if choices.is_empty() {
            let formatted =
                error_display::format_llm_error(PROVIDER_NAME, "No choices in response");
            return Err(LLMError::Provider(formatted));
        }

        let choice = &choices[0];
        let message = choice.get("message").ok_or_else(|| {
            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                "Invalid response format: missing message",
            );
            LLMError::Provider(formatted)
        })?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        let reasoning = message
            .get("reasoning_content")
            .map(|value| match value {
                Value::String(text) => Some(text.to_string()),
                Value::Array(parts) => {
                    let combined = parts
                        .iter()
                        .filter_map(|part| part.as_str())
                        .collect::<Vec<_>>()
                        .join("");
                    if combined.is_empty() {
                        None
                    } else {
                        Some(combined)
                    }
                }
                _ => None,
            })
            .flatten();

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

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|fr| fr.as_str())
            .map(Self::map_finish_reason)
            .unwrap_or(FinishReason::Stop);

        let usage = response_json.get("usage").map(|usage_value| Usage {
            prompt_tokens: usage_value
                .get("prompt_tokens")
                .and_then(|pt| pt.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: usage_value
                .get("completion_tokens")
                .and_then(|ct| ct.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|tt| tt.as_u64())
                .unwrap_or(0) as u32,
            cached_prompt_tokens: usage_value
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .and_then(|value| value.as_u64())
                .map(|value| value as u32),
            cache_creation_tokens: None,
            cache_read_tokens: None,
        });

        Ok(LLMResponse {
            content,
            tool_calls,
            usage,
            finish_reason,
            reasoning,
        })
    }

    fn map_finish_reason(reason: &str) -> FinishReason {
        match reason {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            "sensitive" => FinishReason::ContentFilter,
            other => FinishReason::Error(other.to_string()),
        }
    }

    fn available_models() -> Vec<String> {
        models::zai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

#[async_trait]
impl LLMProvider for ZAIProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        matches!(
            model,
            models::zai::GLM_4_6
                | models::zai::GLM_4_5
                | models::zai::GLM_4_5_AIR
                | models::zai::GLM_4_5_X
                | models::zai::GLM_4_5_AIRX
        )
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        if !Self::available_models().contains(&request.model) {
            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider(PROVIDER_KEY) {
                let formatted = error_display::format_llm_error(PROVIDER_NAME, &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        let payload = self.convert_to_zai_format(&request)?;
        let url = format!("{}{}", self.base_url, CHAT_COMPLETIONS_PATH);

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header(headers::ACCEPT_LANGUAGE, headers::ACCEPT_LANGUAGE_DEFAULT)
            .json(&payload)
            .send()
            .await
            .map_err(|err| {
                let formatted = error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("Network error: {}", err),
                );
                LLMError::Network(formatted)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();

            if status.as_u16() == 429 || text.to_lowercase().contains("rate") {
                return Err(LLMError::RateLimit);
            }

            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .get("message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or(text);

            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("HTTP {}: {}", status, message),
            );
            return Err(LLMError::Provider(formatted));
        }

        let json: Value = response.json().await.map_err(|err| {
            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Failed to parse response: {}", err),
            );
            LLMError::Provider(formatted)
        })?;

        self.parse_zai_response(json)
    }

    fn supported_models(&self) -> Vec<String> {
        Self::available_models()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted =
                error_display::format_llm_error(PROVIDER_NAME, "Messages cannot be empty");
            return Err(LLMError::InvalidRequest(formatted));
        }

        if !request.model.is_empty() && !Self::available_models().contains(&request.model) {
            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider(PROVIDER_KEY) {
                let formatted = error_display::format_llm_error(PROVIDER_NAME, &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for ZAIProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = self.parse_client_prompt(prompt);
        let request_model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: request_model,
            usage: response.usage.map(|usage| llm_types::Usage {
                prompt_tokens: usage.prompt_tokens as usize,
                completion_tokens: usage.completion_tokens as usize,
                total_tokens: usage.total_tokens as usize,
                cached_prompt_tokens: usage.cached_prompt_tokens.map(|v| v as usize),
                cache_creation_tokens: usage.cache_creation_tokens.map(|v| v as usize),
                cache_read_tokens: usage.cache_read_tokens.map(|v| v as usize),
            }),
            reasoning: response.reasoning,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::ZAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
