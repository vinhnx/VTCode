use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole, ToolCall, ToolChoice, ToolDefinition, Usage,
};
use crate::llm::types as llm_types;
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

use super::common::{override_base_url, resolve_model};

#[derive(Debug, Deserialize, Serialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTag>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaTag {
    name: String,
    model: String,
    modified_at: String,
    size: u64,
    digest: String,
    details: OllamaModelDetails,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaModelDetails {
    format: String,
    family: String,
    families: Option<Vec<String>>,
    parameter_size: String,
    quantization_level: String,
}

/// Fetches available local Ollama models from the Ollama API endpoint
pub async fn fetch_ollama_models(base_url: Option<String>) -> Result<Vec<String>, anyhow::Error> {
    use crate::config::constants::{env_vars, urls};

    let resolved_base_url = override_base_url(
        urls::OLLAMA_API_BASE,
        base_url,
        Some(env_vars::OLLAMA_BASE_URL),
    );

    // Construct the tags endpoint URL
    let tags_url = format!("{}/api/tags", resolved_base_url);

    // Create HTTP client
    let client = reqwest::Client::new();

    // Make GET request to fetch models
    let response = client
        .get(&tags_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch Ollama models: {}", e))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch Ollama models: HTTP {}",
            response.status()
        ));
    }

    // Parse the response
    let tags_response: OllamaTagsResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse Ollama models response: {}", e))?;

    // Extract model names
    let model_names: Vec<String> = tags_response
        .models
        .into_iter()
        .map(|model| model.name) // Use 'name' field which is the full model name including tag
        .collect();

    Ok(model_names)
}

pub struct OllamaProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OllamaProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::ollama::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(model, None, Some(api_key))
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        let resolved_model = resolve_model(model, models::ollama::DEFAULT_MODEL);
        Self::with_model_internal(resolved_model, base_url, api_key)
    }

    fn normalize_api_key(api_key: Option<String>) -> Option<String> {
        api_key.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    }

    fn with_model_internal(
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        let api_key = Self::normalize_api_key(api_key);

        // Determine if this is a cloud model based on the model name
        // Cloud models are identified by having ":cloud" or "-cloud" in their name
        let is_cloud_model = model.contains(":cloud") || model.contains("-cloud");

        // For local Ollama models (not cloud ones), do not use any API key
        // This prevents sending an API key to local Ollama instances
        let effective_api_key = if is_cloud_model {
            api_key
        } else {
            None // Always use no API key for local Ollama models
        };

        // Use appropriate base URL based on whether it's a cloud model or not
        let default_base = if is_cloud_model {
            urls::OLLAMA_CLOUD_API_BASE
        } else {
            urls::OLLAMA_API_BASE
        };

        Self {
            http_client: HttpClient::new(),
            base_url: override_base_url(default_base, base_url, Some(env_vars::OLLAMA_BASE_URL)),
            model,
            api_key: effective_api_key,
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url.trim_end_matches('/'))
    }

    fn authorized_post(&self, url: String) -> reqwest::RequestBuilder {
        let builder = self.http_client.post(url);
        if let Some(api_key) = &self.api_key {
            builder.bearer_auth(api_key)
        } else {
            builder
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
            verbosity: None,
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
            .filter(|text| !text.trim().is_empty())
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

            if content.trim().is_empty() {
                continue;
            }

            match role {
                "system" => {
                    if system_prompt.is_none() {
                        system_prompt = Some(content);
                    }
                }
                "assistant" => messages.push(Message::assistant(content)),
                "user" => messages.push(Message::user(content)),
                _ => {}
            }
        }

        if messages.is_empty() {
            return None;
        }

        let tools = value
            .get("tools")
            .and_then(|entry| serde_json::from_value::<Vec<ToolDefinition>>(entry.clone()).ok());

        Some(LLMRequest {
            messages,
            system_prompt,
            tools,
            model: value
                .get("model")
                .and_then(|m| m.as_str())
                .filter(|m| !m.trim().is_empty())
                .map(|m| m.to_string())
                .unwrap_or_else(|| self.model.clone()),
            max_tokens: value
                .get("max_tokens")
                .and_then(|entry| entry.as_u64())
                .map(|value| value as u32),
            temperature: value
                .get("temperature")
                .and_then(|entry| entry.as_f64())
                .map(|value| value as f32),
            stream: value
                .get("stream")
                .and_then(|entry| entry.as_bool())
                .unwrap_or(false),
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
        })
    }

    fn build_payload(
        &self,
        request: &LLMRequest,
        stream: bool,
    ) -> Result<OllamaChatRequest, LLMError> {
        let mut messages = Vec::new();
        let mut tool_names: HashMap<String, String> = HashMap::new();

        if let Some(system) = &request.system_prompt {
            if !system.trim().is_empty() {
                messages.push(OllamaChatMessage {
                    role: "system".to_string(),
                    content: Some(system.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                });
            }
        }

        for message in &request.messages {
            let content_text = message.content.as_text();
            match message.role {
                MessageRole::Tool => {
                    let tool_name = message
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| tool_names.get(id).cloned());
                    messages.push(OllamaChatMessage {
                        role: "tool".to_string(),
                        content: Some(content_text.clone()),
                        tool_calls: None,
                        tool_call_id: message.tool_call_id.clone(),
                        tool_name,
                    });
                }
                _ => {
                    let mut payload_message = OllamaChatMessage {
                        role: message.role.as_generic_str().to_string(),
                        content: Some(content_text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                        tool_name: None,
                    };

                    if let Some(tool_calls) = message.get_tool_calls() {
                        let mut converted = Vec::new();
                        for (index, tool_call) in tool_calls.iter().enumerate() {
                            if !tool_call.id.is_empty() {
                                tool_names
                                    .entry(tool_call.id.clone())
                                    .or_insert_with(|| tool_call.function.name.clone());
                            }

                            let arguments =
                                Self::parse_tool_arguments(&tool_call.function.arguments)?;
                            converted.push(OllamaToolCall {
                                call_type: tool_call.call_type.clone(),
                                function: OllamaToolFunctionCall {
                                    name: tool_call.function.name.clone(),
                                    arguments: Some(arguments),
                                    index: Some(index as u32),
                                },
                            });
                        }

                        if !converted.is_empty() {
                            payload_message.tool_calls = Some(converted);
                            if payload_message.content.is_none() {
                                payload_message.content = Some(String::new());
                            }
                        }
                    }

                    messages.push(payload_message);
                }
            }
        }

        let options = if request.temperature.is_some() || request.max_tokens.is_some() {
            Some(OllamaChatOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            })
        } else {
            None
        };

        let tools = match request.tool_choice {
            Some(ToolChoice::None) => None,
            _ => request.tools.clone(),
        };

        Ok(OllamaChatRequest {
            model: request.model.clone(),
            messages,
            stream,
            options,
            tools,
            think: Self::think_value(request),
        })
    }

    fn parse_tool_arguments(arguments: &str) -> Result<Value, LLMError> {
        if arguments.trim().is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        serde_json::from_str(arguments).map_err(|err| {
            LLMError::InvalidRequest(format!("Failed to parse tool arguments for Ollama: {err}"))
        })
    }

    fn think_value(request: &LLMRequest) -> Option<Value> {
        let model_id = request.model.as_str();
        if !models::ollama::REASONING_MODELS.contains(&model_id) {
            return None;
        }

        if models::ollama::REASONING_LEVEL_MODELS.contains(&model_id) {
            request
                .reasoning_effort
                .map(|effort| Value::String(effort.as_str().to_string()))
        } else {
            Some(Value::Bool(true))
        }
    }

    fn convert_tool_calls(
        tool_calls: Option<Vec<OllamaResponseToolCall>>,
    ) -> Result<Option<Vec<ToolCall>>, LLMError> {
        let Some(tool_calls) = tool_calls else {
            return Ok(None);
        };

        if tool_calls.is_empty() {
            return Ok(None);
        }

        let mut converted = Vec::new();
        for (index, call) in tool_calls.into_iter().enumerate() {
            let function = call.function.ok_or_else(|| {
                LLMError::Provider(
                    "Ollama response missing function details for tool call".to_string(),
                )
            })?;

            let name = function.name.ok_or_else(|| {
                LLMError::Provider("Ollama response missing tool function name".to_string())
            })?;

            let arguments_value = function
                .arguments
                .unwrap_or_else(|| Value::Object(Map::new()));
            let arguments = match arguments_value {
                Value::String(raw) => raw,
                other => serde_json::to_string(&other).map_err(|err| {
                    LLMError::Provider(format!("Failed to serialize Ollama tool arguments: {err}"))
                })?,
            };

            let id = function
                .index
                .map(|value| format!("tool_call_{value}"))
                .unwrap_or_else(|| format!("tool_call_{index}"));

            converted.push(ToolCall::function(id, name, arguments));
        }

        Ok(Some(converted))
    }

    fn usage_from_counts(
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> Option<Usage> {
        if prompt_tokens.is_none() && completion_tokens.is_none() {
            return None;
        }

        let prompt = prompt_tokens.unwrap_or_default();
        let completion = completion_tokens.unwrap_or_default();
        Some(Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        })
    }

    fn finish_reason_from(reason: Option<&str>) -> FinishReason {
        match reason {
            Some("stop") | None => FinishReason::Stop,
            Some("length") => FinishReason::Length,
            Some("tool_calls") => FinishReason::ToolCalls,
            Some(other) => FinishReason::Error(other.to_string()),
        }
    }

    fn build_response(
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
        reasoning: Option<String>,
        finish_reason: Option<&str>,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> LLMResponse {
        let mut finish = Self::finish_reason_from(finish_reason);
        if tool_calls.as_ref().map_or(false, |calls| !calls.is_empty()) {
            finish = FinishReason::ToolCalls;
        }

        LLMResponse {
            content,
            tool_calls,
            usage: Self::usage_from_counts(prompt_tokens, completion_tokens),
            finish_reason: finish,
            reasoning,
            reasoning_details: None,
        }
    }

    fn extract_error(body: &str) -> Option<String> {
        serde_json::from_str::<OllamaErrorResponse>(body)
            .ok()
            .and_then(|resp| resp.error)
    }
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaChatOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<Value>,
}

#[derive(Debug, Serialize)]
struct OllamaChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OllamaChatOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Debug, Serialize)]
struct OllamaToolCall {
    #[serde(rename = "type")]
    call_type: String,
    function: OllamaToolFunctionCall,
}

#[derive(Debug, Serialize)]
struct OllamaToolFunctionCall {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaResponseMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    #[serde(default)]
    #[allow(dead_code)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseToolCall {
    #[serde(default)]
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<OllamaResponseFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseFunctionCall {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<Value>,
    #[serde(default)]
    index: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaErrorResponse {
    error: Option<String>,
}

fn map_reqwest_error(err: reqwest::Error) -> LLMError {
    if err.is_timeout() || err.is_connect() {
        LLMError::Network(err.to_string())
    } else {
        LLMError::Provider(err.to_string())
    }
}

fn parse_stream_chunk(line: &str) -> Result<OllamaChatResponse, LLMError> {
    serde_json::from_str::<OllamaChatResponse>(line)
        .map_err(|err| LLMError::Provider(format!("Failed to parse Ollama stream chunk: {err}")))
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        models::ollama::REASONING_MODELS.contains(&model)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        models::ollama::REASONING_LEVEL_MODELS.contains(&model)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let payload = self.build_payload(&request, false)?;
        let url = self.chat_url();

        let response = self
            .authorized_post(url)
            .json(&payload)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let error_message = Self::extract_error(&body)
                .unwrap_or_else(|| format!("Ollama request failed ({status}): {body}"));
            return Err(LLMError::Provider(error_message));
        }

        let parsed = response
            .json::<OllamaChatResponse>()
            .await
            .map_err(map_reqwest_error)?;

        if let Some(error) = parsed.error {
            return Err(LLMError::Provider(error));
        }

        let (content, reasoning, tool_calls) = if let Some(message) = parsed.message {
            let content = message
                .content
                .and_then(|value| (!value.is_empty()).then_some(value));
            let reasoning = message
                .thinking
                .and_then(|value| (!value.is_empty()).then_some(value));
            let tool_calls = Self::convert_tool_calls(message.tool_calls)?;
            (content, reasoning, tool_calls)
        } else {
            (None, None, None)
        };

        Ok(Self::build_response(
            content,
            tool_calls,
            reasoning,
            parsed.done_reason.as_deref(),
            parsed.prompt_eval_count,
            parsed.eval_count,
        ))
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let payload = self.build_payload(&request, true)?;
        let url = self.chat_url();

        let response = self
            .authorized_post(url)
            .json(&payload)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let error_message = Self::extract_error(&body)
                .unwrap_or_else(|| format!("Ollama streaming request failed ({status}): {body}"));
            return Err(LLMError::Provider(error_message));
        }

        let byte_stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut accumulated = String::new();
        let mut reasoning_buffer = String::new();
        let stream = try_stream! {
            let mut prompt_tokens: Option<u32> = None;
            let mut completion_tokens: Option<u32> = None;
            let mut finish_reason: Option<String> = None;
            let mut completed = false;
            let mut tool_calls: Option<Vec<ToolCall>> = None;

            futures::pin_mut!(byte_stream);
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk.map_err(map_reqwest_error)?;
                buffer.extend_from_slice(&chunk);

                while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
                    let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                    let line = std::str::from_utf8(&line_bytes)
                        .map_err(|err| LLMError::Provider(format!("Invalid UTF-8 in Ollama stream: {err}")))?
                        .trim();

                    if line.is_empty() {
                        continue;
                    }

                    let parsed = parse_stream_chunk(line)?;

                    if let Some(error) = parsed.error {
                        Err(LLMError::Provider(error))?;
                    }

                    if let Some(message) = parsed.message {
                        if let Some(thinking) = message
                            .thinking
                            .and_then(|value| (!value.is_empty()).then_some(value))
                        {
                            reasoning_buffer.push_str(&thinking);
                            yield LLMStreamEvent::Reasoning { delta: thinking };
                        }

                        if let Some(content) = message
                            .content
                            .and_then(|value| (!value.is_empty()).then_some(value))
                        {
                            accumulated.push_str(&content);
                            yield LLMStreamEvent::Token { delta: content };
                        }

                        if let Some(parsed_calls) = Self::convert_tool_calls(message.tool_calls)? {
                            tool_calls = Some(parsed_calls);
                        }
                    }

                    if parsed.done {
                        prompt_tokens = parsed.prompt_eval_count;
                        completion_tokens = parsed.eval_count;
                        finish_reason = parsed.done_reason;
                        completed = true;
                        break;
                    }
                }

                if completed {
                    break;
                }
            }

            if !completed {
                Err(LLMError::Provider(
                    "Ollama stream ended without completion signal".to_string(),
                ))?;
            }

            let response = Self::build_response(
                if accumulated.is_empty() {
                    None
                } else {
                    Some(accumulated.clone())
                },
                tool_calls,
                if reasoning_buffer.is_empty() {
                    None
                } else {
                    Some(reasoning_buffer.clone())
                },
                finish_reason.as_deref(),
                prompt_tokens,
                completion_tokens,
            );

            yield LLMStreamEvent::Completed { response };
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        models::ollama::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if let Some(tool_choice) = &request.tool_choice {
            match tool_choice {
                ToolChoice::Auto | ToolChoice::None => {}
                _ => {
                    return Err(LLMError::InvalidRequest(
                        "Ollama does not support explicit tool_choice overrides".to_string(),
                    ));
                }
            }
        }

        if request.parallel_tool_calls.is_some() || request.parallel_tool_config.is_some() {
            return Err(LLMError::InvalidRequest(
                "Ollama does not support parallel tool configuration".to_string(),
            ));
        }

        for message in &request.messages {
            if matches!(message.role, MessageRole::Tool) && message.tool_call_id.is_none() {
                return Err(LLMError::InvalidRequest(
                    "Ollama tool responses must include tool_call_id".to_string(),
                ));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for OllamaProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let mut request = self.parse_client_prompt(prompt);
        if request.model.is_empty() {
            request.model = self.model.clone();
        }

        let request_model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: request_model,
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
        llm_types::BackendKind::Ollama
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
