use crate::config::constants::{models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole, Usage,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct OllamaProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::ollama::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        let _ = api_key;
        Self::with_model_internal(model, None)
    }

    pub fn from_config(
        _api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let resolved_model = model.unwrap_or_else(|| models::ollama::DEFAULT_MODEL.to_string());
        Self::with_model_internal(resolved_model, base_url)
    }

    fn with_model_internal(model: String, base_url: Option<String>) -> Self {
        let resolved_base_url = base_url.unwrap_or_else(|| urls::OLLAMA_API_BASE.to_string());

        Self {
            http_client: HttpClient::new(),
            base_url: resolved_base_url,
            model,
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url.trim_end_matches('/'))
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

        Some(LLMRequest {
            messages,
            system_prompt,
            tools: None,
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
        })
    }

    fn build_payload(
        &self,
        request: &LLMRequest,
        stream: bool,
    ) -> Result<OllamaChatRequest, LLMError> {
        let mut messages = Vec::new();

        if let Some(system) = &request.system_prompt {
            if !system.trim().is_empty() {
                messages.push(OllamaChatMessage {
                    role: "system".to_string(),
                    content: system.clone(),
                });
            }
        }

        for message in &request.messages {
            if message.has_tool_calls() {
                return Err(LLMError::InvalidRequest(
                    "Ollama does not support structured tool calling".to_string(),
                ));
            }

            match message.role {
                MessageRole::System => messages.push(OllamaChatMessage {
                    role: "system".to_string(),
                    content: message.content.clone(),
                }),
                MessageRole::User => messages.push(OllamaChatMessage {
                    role: "user".to_string(),
                    content: message.content.clone(),
                }),
                MessageRole::Assistant => messages.push(OllamaChatMessage {
                    role: "assistant".to_string(),
                    content: message.content.clone(),
                }),
                MessageRole::Tool => {
                    return Err(LLMError::InvalidRequest(
                        "Ollama does not support tool response messages".to_string(),
                    ));
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

        Ok(OllamaChatRequest {
            model: request.model.clone(),
            messages,
            stream,
            options,
        })
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
        finish_reason: Option<&str>,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> LLMResponse {
        LLMResponse {
            content,
            tool_calls: None,
            usage: Self::usage_from_counts(prompt_tokens, completion_tokens),
            finish_reason: Self::finish_reason_from(finish_reason),
            reasoning: None,
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
}

#[derive(Debug, Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OllamaChatOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
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
    content: String,
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
        false
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let payload = self.build_payload(&request, false)?;
        let url = self.chat_url();

        let response = self
            .http_client
            .post(url)
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

        let content = parsed
            .message
            .map(|message| message.content)
            .filter(|content| !content.is_empty());

        Ok(Self::build_response(
            content,
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
            .http_client
            .post(url)
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
        let stream = try_stream! {
            let mut prompt_tokens: Option<u32> = None;
            let mut completion_tokens: Option<u32> = None;
            let mut finish_reason: Option<String> = None;
            let mut completed = false;

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
                        if !message.content.is_empty() {
                            accumulated.push_str(&message.content);
                            yield LLMStreamEvent::Token {
                                delta: message.content,
                            };
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
        if request.tools.is_some() {
            return Err(LLMError::InvalidRequest(
                "Ollama does not support structured tool calling".to_string(),
            ));
        }

        if request
            .messages
            .iter()
            .any(|message| matches!(message.role, MessageRole::Tool))
        {
            return Err(LLMError::InvalidRequest(
                "Ollama does not support tool response messages".to_string(),
            ));
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
