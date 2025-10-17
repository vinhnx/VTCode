use crate::config::constants::{models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole, ToolCall, ToolChoice, ToolDefinition, Usage,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use ollama_rs::error::OllamaError;
use ollama_rs::generation::chat::{ChatMessage, ChatMessageFinalResponseData, ChatMessageResponse};
use ollama_rs::generation::tools::{
    ToolCall as OllamaToolCall, ToolCallFunction as OllamaToolCallFunction,
    ToolInfo as OllamaToolInfo,
};
use ollama_rs::headers::{AUTHORIZATION, HeaderValue};
use ollama_rs::models::ModelOptions;
use reqwest::{Client as HttpClient, RequestBuilder, StatusCode, Url};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct OllamaProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OllamaProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            models::ollama::DEFAULT_MODEL.to_string(),
            None,
            if api_key.trim().is_empty() {
                None
            } else {
                Some(api_key)
            },
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(
            model,
            None,
            if api_key.trim().is_empty() {
                None
            } else {
                Some(api_key)
            },
        )
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let resolved_model = model.unwrap_or_else(|| models::ollama::DEFAULT_MODEL.to_string());
        Self::with_model_internal(resolved_model, base_url, api_key)
    }

    fn with_model_internal(
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        let resolved_base_url = base_url
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| urls::OLLAMA_API_BASE.to_string());

        Self {
            http_client: HttpClient::new(),
            base_url: resolved_base_url,
            model,
            api_key,
        }
    }

    fn chat_endpoint(&self) -> Result<Url, LLMError> {
        let raw = self.base_url.trim();
        let base = Url::parse(raw).map_err(|err| {
            LLMError::InvalidRequest(format!(
                "Invalid Ollama base URL '{}': {err}",
                self.base_url
            ))
        })?;

        let sanitized = raw.trim_end_matches('/');
        if sanitized.ends_with("/v1") {
            return Err(LLMError::InvalidRequest(format!(
                "Ollama base URL '{}' points to the OpenAI-compatible /v1 path used by integrations like Droid. Configure VT Code with the root Ollama host (for example http://localhost:11434) so the /api/chat endpoint is reachable.",
                self.base_url
            )));
        }

        if sanitized.ends_with("/api/chat") {
            return Ok(base);
        }

        if sanitized.ends_with("/api") {
            let normalized = format!("{sanitized}/");
            let normalized_base = Url::parse(&normalized).map_err(|err| {
                LLMError::InvalidRequest(format!(
                    "Invalid Ollama base URL '{}': {err}",
                    self.base_url
                ))
            })?;

            return normalized_base.join("chat").map_err(|err| {
                LLMError::InvalidRequest(format!(
                    "Invalid Ollama base URL '{}': {err}",
                    self.base_url
                ))
            });
        }

        base.join("api/chat").map_err(|err| {
            LLMError::InvalidRequest(format!(
                "Invalid Ollama base URL '{}': {err}",
                self.base_url
            ))
        })
    }

    fn apply_auth(&self, builder: RequestBuilder) -> Result<RequestBuilder, LLMError> {
        let Some(key) = &self.api_key else {
            return Ok(builder);
        };

        let trimmed = key.trim();
        if trimmed.is_empty() {
            return Ok(builder);
        }

        let value = format!("Bearer {trimmed}");
        let header_value = HeaderValue::from_str(&value)
            .map_err(|err| LLMError::InvalidRequest(format!("Invalid Ollama API key: {err}")))?;

        Ok(builder.header(AUTHORIZATION, header_value))
    }

    fn build_chat_request(
        &self,
        request: &LLMRequest,
        stream: bool,
    ) -> Result<OllamaChatRequestPayload, LLMError> {
        let model = if request.model.is_empty() {
            self.model.clone()
        } else {
            request.model.clone()
        };

        let messages = self.convert_messages(request)?;
        let tools = self.convert_tool_definitions(request)?;
        let tool_choice = self.convert_tool_choice(request.tool_choice.as_ref())?;
        let think = if request.reasoning_effort.is_some() {
            Some(true)
        } else {
            None
        };

        Ok(OllamaChatRequestPayload {
            model,
            messages,
            tools,
            options: self.model_options_for(request),
            tool_choice,
            think,
            stream,
        })
    }

    fn convert_messages(&self, request: &LLMRequest) -> Result<Vec<ChatMessage>, LLMError> {
        let mut messages = Vec::new();

        if let Some(system) = &request.system_prompt {
            if !system.trim().is_empty() {
                messages.push(ChatMessage::system(system.clone()));
            }
        }

        for message in &request.messages {
            let chat_message = match message.role {
                MessageRole::System => ChatMessage::system(message.content.clone()),
                MessageRole::User => ChatMessage::user(message.content.clone()),
                MessageRole::Assistant => {
                    let mut assistant = ChatMessage::assistant(message.content.clone());
                    if let Some(tool_calls) = &message.tool_calls {
                        assistant.tool_calls = self.convert_assistant_tool_calls(tool_calls)?;
                    }
                    assistant
                }
                MessageRole::Tool => ChatMessage::tool(message.content.clone()),
            };

            messages.push(chat_message);
        }

        Ok(messages)
    }

    fn convert_assistant_tool_calls(
        &self,
        calls: &[ToolCall],
    ) -> Result<Vec<OllamaToolCall>, LLMError> {
        let mut converted = Vec::with_capacity(calls.len());

        for call in calls {
            let arguments: Value =
                serde_json::from_str(&call.function.arguments).map_err(|err| {
                    LLMError::InvalidRequest(format!(
                        "Invalid tool arguments for '{}': {err}",
                        call.function.name
                    ))
                })?;

            converted.push(OllamaToolCall {
                function: OllamaToolCallFunction {
                    name: call.function.name.clone(),
                    arguments,
                },
            });
        }

        Ok(converted)
    }

    fn convert_tool_definitions(
        &self,
        request: &LLMRequest,
    ) -> Result<Vec<OllamaToolInfo>, LLMError> {
        let Some(definitions) = &request.tools else {
            return Ok(Vec::new());
        };

        let mut converted = Vec::with_capacity(definitions.len());
        for tool in definitions {
            let tool_value = json!({
                "type": "function",
                "function": {
                    "name": tool.function.name,
                    "description": tool.function.description,
                    "parameters": tool.function.parameters,
                }
            });

            let tool_info: OllamaToolInfo = serde_json::from_value(tool_value).map_err(|err| {
                LLMError::InvalidRequest(format!(
                    "Invalid tool schema for '{}': {err}",
                    tool.function.name
                ))
            })?;

            converted.push(tool_info);
        }

        Ok(converted)
    }

    fn convert_tool_choice(&self, choice: Option<&ToolChoice>) -> Result<Option<Value>, LLMError> {
        let Some(choice) = choice else {
            return Ok(None);
        };

        let value = match choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Any => json!("required"),
            ToolChoice::Specific(specific) => {
                let tool_type = if specific.tool_type.trim().is_empty() {
                    "function"
                } else {
                    specific.tool_type.as_str()
                };

                json!({
                    "type": tool_type,
                    "function": {
                        "name": specific.function.name
                    }
                })
            }
        };

        Ok(Some(value))
    }

    fn model_options_for(&self, request: &LLMRequest) -> Option<ModelOptions> {
        let mut options = ModelOptions::default();
        let mut has_options = false;

        if let Some(temp) = request.temperature {
            options = options.temperature(temp);
            has_options = true;
        }

        if let Some(tokens) = request.max_tokens {
            options = options.num_predict(tokens as i32);
            has_options = true;
        }

        if has_options { Some(options) } else { None }
    }

    fn map_response(&self, response: ChatMessageResponse) -> Result<LLMResponse, LLMError> {
        let tool_calls = self.convert_tool_calls(&response.message.tool_calls)?;
        let has_tool_calls = !tool_calls.is_empty();
        let usage = self.usage_from_final_data(response.final_data.as_ref());
        let reasoning = response.message.thinking.as_ref().and_then(|text| {
            if text.trim().is_empty() {
                None
            } else {
                Some(text.clone())
            }
        });

        Ok(LLMResponse {
            content: if response.message.content.trim().is_empty() {
                None
            } else {
                Some(response.message.content)
            },
            tool_calls: if has_tool_calls {
                Some(tool_calls)
            } else {
                None
            },
            usage,
            finish_reason: if has_tool_calls {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            },
            reasoning,
        })
    }

    fn convert_tool_calls(&self, calls: &[OllamaToolCall]) -> Result<Vec<ToolCall>, LLMError> {
        let mut converted = Vec::with_capacity(calls.len());
        for (index, call) in calls.iter().enumerate() {
            let args_string = serde_json::to_string(&call.function.arguments).map_err(|err| {
                LLMError::Provider(format!(
                    "Failed to serialize Ollama tool arguments for '{}': {err}",
                    call.function.name
                ))
            })?;

            converted.push(ToolCall::function(
                Self::tool_id_for(&call.function.name, index),
                call.function.name.clone(),
                args_string,
            ));
        }

        Ok(converted)
    }

    fn usage_from_final_data(
        &self,
        final_data: Option<&ChatMessageFinalResponseData>,
    ) -> Option<Usage> {
        let data = final_data?;
        let prompt = Self::clamp_usage(data.prompt_eval_count);
        let completion = Self::clamp_usage(data.eval_count);

        Some(Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt.saturating_add(completion),
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        })
    }

    fn clamp_usage(value: u64) -> u32 {
        if value > u32::MAX as u64 {
            u32::MAX
        } else {
            value as u32
        }
    }

    fn reasoning_delta(accumulated: &mut String, next: Option<&String>) -> Option<String> {
        let next = next?;

        if next.trim().is_empty() {
            accumulated.clear();
            return None;
        }

        if next.len() < accumulated.len() {
            *accumulated = next.clone();
            return None;
        }

        let mut prefix_bytes = 0usize;
        for (left, right) in accumulated.chars().zip(next.chars()) {
            if left == right {
                prefix_bytes += left.len_utf8();
            } else {
                break;
            }
        }

        let delta = &next[prefix_bytes..];
        *accumulated = next.clone();

        if delta.is_empty() {
            None
        } else {
            Some(delta.to_string())
        }
    }

    fn is_rate_limit_message(message: &str) -> bool {
        let lowered = message.to_ascii_lowercase();
        lowered.contains("rate limit")
            || lowered.contains("too many requests")
            || lowered.contains("429")
    }

    fn map_ollama_error(err: OllamaError) -> LLMError {
        match err {
            OllamaError::ReqwestError(inner) => {
                if inner.is_timeout() || inner.is_connect() {
                    LLMError::Network(inner.to_string())
                } else if inner.status() == Some(StatusCode::UNAUTHORIZED) {
                    LLMError::Authentication(inner.to_string())
                } else if inner.status() == Some(StatusCode::TOO_MANY_REQUESTS) {
                    LLMError::RateLimit
                } else {
                    LLMError::Provider(inner.to_string())
                }
            }
            OllamaError::JsonError(inner) => {
                LLMError::Provider(format!("Failed to parse Ollama response: {inner}"))
            }
            OllamaError::InternalError(inner) => {
                let message = inner.message;
                if Self::is_rate_limit_message(&message) {
                    LLMError::RateLimit
                } else if message.to_lowercase().contains("unauthorized") {
                    LLMError::Authentication(message)
                } else {
                    LLMError::Provider(message)
                }
            }
            OllamaError::ToolCallError(inner) => LLMError::Provider(inner.to_string()),
            OllamaError::Other(message) => {
                if Self::is_rate_limit_message(&message) {
                    LLMError::RateLimit
                } else if message.to_lowercase().contains("unauthorized") {
                    LLMError::Authentication(message)
                } else {
                    LLMError::Provider(message)
                }
            }
        }
    }

    fn map_reqwest_error(err: reqwest::Error) -> LLMError {
        Self::map_ollama_error(OllamaError::ReqwestError(err))
    }

    fn error_from_status(status: StatusCode, body: String) -> LLMError {
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return LLMError::Authentication(body);
        }

        if status == StatusCode::TOO_MANY_REQUESTS || Self::is_rate_limit_message(&body) {
            return LLMError::RateLimit;
        }

        if status.is_client_error() || status.is_server_error() {
            let message = if body.trim().is_empty() {
                format!("Ollama request failed with status {status}")
            } else {
                format!("Ollama request failed ({status}): {body}")
            };

            return LLMError::Provider(message);
        }

        LLMError::Provider(body)
    }

    fn tool_id_for(name: &str, index: usize) -> String {
        format!("ollama-call-{name}-{index}")
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

        let tools = value
            .get("tools")
            .and_then(|entry| serde_json::from_value::<Vec<ToolDefinition>>(entry.clone()).ok())
            .map(|definitions| {
                definitions
                    .into_iter()
                    .filter(|tool| tool.validate().is_ok())
                    .collect::<Vec<_>>()
            })
            .filter(|definitions| !definitions.is_empty());

        let tool_choice = value.get("tool_choice").and_then(|entry| {
            if entry.is_null() {
                return None;
            }

            if let Some(text) = entry.as_str() {
                match text {
                    "auto" => Some(ToolChoice::Auto),
                    "none" => Some(ToolChoice::None),
                    "any" | "required" => Some(ToolChoice::Any),
                    _ => None,
                }
            } else {
                serde_json::from_value::<ToolChoice>(entry.clone()).ok()
            }
        });

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
            tool_choice,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        })
    }
}

#[derive(Default)]
struct OllamaStreamState {
    accumulated: String,
    reasoning_accumulated: String,
    final_chunk: Option<ChatMessageResponse>,
    buffer: String,
    done: bool,
}

impl OllamaStreamState {
    fn push_bytes(&mut self, bytes: &[u8]) -> Result<Vec<LLMStreamEvent>, LLMError> {
        if self.done || bytes.is_empty() {
            return Ok(Vec::new());
        }

        let chunk_str = std::str::from_utf8(bytes).map_err(|err| {
            LLMError::Provider(format!("Failed to decode Ollama stream chunk: {err}"))
        })?;

        self.buffer.push_str(chunk_str);
        let mut events = Vec::new();

        while let Some(newline_index) = self.buffer.find('\n') {
            let line = self.buffer[..newline_index].to_string();
            self.buffer = self.buffer[newline_index + 1..].to_string();

            self.process_line(&line, &mut events)?;

            if self.done {
                break;
            }
        }

        Ok(events)
    }

    fn finalize(&mut self) -> Result<Vec<LLMStreamEvent>, LLMError> {
        if self.done {
            self.buffer.clear();
            return Ok(Vec::new());
        }

        let buffer = std::mem::take(&mut self.buffer);
        let trimmed = buffer.trim().to_string();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let mut events = Vec::new();
        self.process_line(&trimmed, &mut events)?;

        Ok(events)
    }

    fn into_final_response(self) -> Result<ChatMessageResponse, LLMError> {
        let mut final_chunk = self.final_chunk.ok_or_else(|| {
            LLMError::Provider("Ollama stream ended without completion signal".to_string())
        })?;

        if !final_chunk.done {
            return Err(LLMError::Provider(
                "Ollama stream ended without completion signal".to_string(),
            ));
        }

        if !self.accumulated.is_empty() {
            final_chunk.message.content = self.accumulated;
        }

        if !self.reasoning_accumulated.is_empty() {
            final_chunk.message.thinking = Some(self.reasoning_accumulated);
        }

        Ok(final_chunk)
    }

    fn is_done(&self) -> bool {
        self.done
    }

    fn process_line(
        &mut self,
        line: &str,
        events: &mut Vec<LLMStreamEvent>,
    ) -> Result<(), LLMError> {
        let mut content = line.trim();
        if content.is_empty() {
            return Ok(());
        }

        if let Some(stripped) = content.strip_prefix("data:") {
            let candidate = stripped.trim();
            if candidate.is_empty() || candidate == "[DONE]" {
                return Ok(());
            }

            content = candidate;
        }

        let chunk: ChatMessageResponse = serde_json::from_str(content)
            .map_err(|err| OllamaProvider::map_ollama_error(OllamaError::JsonError(err)))?;

        if let Some(delta) = OllamaProvider::reasoning_delta(
            &mut self.reasoning_accumulated,
            chunk.message.thinking.as_ref(),
        ) {
            if !delta.is_empty() {
                events.push(LLMStreamEvent::Reasoning { delta });
            }
        }

        if !chunk.message.content.is_empty() {
            let delta = chunk.message.content.clone();
            self.accumulated.push_str(&delta);
            events.push(LLMStreamEvent::Token { delta });
        }

        if chunk.done {
            self.done = true;
            self.buffer.clear();
            self.final_chunk = Some(chunk);
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct OllamaChatRequestPayload {
    #[serde(rename = "model")]
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OllamaToolInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<ModelOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    stream: bool,
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

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }

        let payload = self.build_chat_request(&request, false)?;
        let url = self.chat_endpoint()?;
        let response = self
            .apply_auth(self.http_client.post(url))?
            .json(&payload)
            .send()
            .await
            .map_err(Self::map_reqwest_error)?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_else(|err| err.to_string());
            return Err(Self::error_from_status(status, message));
        }

        let bytes = response.bytes().await.map_err(Self::map_reqwest_error)?;
        let response = serde_json::from_slice::<ChatMessageResponse>(&bytes)
            .map_err(|err| Self::map_ollama_error(OllamaError::JsonError(err)))?;

        self.map_response(response)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.validate_request(&request)?;
        if request.model.is_empty() {
            request.model = self.model.clone();
        }

        let payload = self.build_chat_request(&request, true)?;
        let url = self.chat_endpoint()?;
        let response = self
            .apply_auth(self.http_client.post(url))?
            .json(&payload)
            .send()
            .await
            .map_err(Self::map_reqwest_error)?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_else(|err| err.to_string());
            return Err(Self::error_from_status(status, message));
        }

        let mut response_stream = response.bytes_stream();
        let provider = self.clone();

        let stream = try_stream! {
            let mut state = OllamaStreamState::default();

            while let Some(item) = response_stream.next().await {
                let bytes = item.map_err(Self::map_reqwest_error)?;

                for event in state.push_bytes(&bytes)? {
                    yield event;
                }

                if state.is_done() {
                    break;
                }
            }

            for event in state.finalize()? {
                yield event;
            }

            let final_response = state.into_final_response()?;
            let response = provider.map_response(final_response)?;
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
        for message in &request.messages {
            message
                .validate_for_provider(self.name())
                .map_err(LLMError::InvalidRequest)?;
        }

        if let Some(tools) = &request.tools {
            for tool in tools {
                tool.validate().map_err(LLMError::InvalidRequest)?;
            }
        }

        if let Some(choice) = &request.tool_choice {
            match choice {
                ToolChoice::Auto => {}
                ToolChoice::None => {}
                ToolChoice::Any => {
                    if request
                        .tools
                        .as_ref()
                        .map_or(true, |tools| tools.is_empty())
                    {
                        return Err(LLMError::InvalidRequest(
                            "Ollama tool_choice 'required' needs at least one tool definition"
                                .to_string(),
                        ));
                    }
                }
                ToolChoice::Specific(specific) => {
                    let Some(tools) = &request.tools else {
                        return Err(LLMError::InvalidRequest(format!(
                            "Ollama tool_choice references '{}', but no tools were provided",
                            specific.function.name
                        )));
                    };

                    if !tools
                        .iter()
                        .any(|tool| tool.function.name == specific.function.name)
                    {
                        return Err(LLMError::InvalidRequest(format!(
                            "Ollama tool_choice references unknown tool '{}'",
                            specific.function.name
                        )));
                    }
                }
            }
        }

        if request.parallel_tool_calls.is_some() || request.parallel_tool_config.is_some() {
            return Err(LLMError::InvalidRequest(
                "Ollama does not support parallel tool call configuration".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::ReasoningEffortLevel;
    use serde_json::json;

    #[test]
    fn parse_chat_request_extracts_tools_and_tool_choice() {
        let provider = OllamaProvider::from_config(None, None, None, None);
        let request_json = json!({
            "messages": [
                {"role": "user", "content": "Call the weather tool"}
            ],
            "system": "You are helpful.",
            "model": "llama3:8b",
            "max_tokens": 256,
            "temperature": 0.5,
            "stream": true,
            "tool_choice": {
                "type": "function",
                "function": {"name": "get_weather"}
            },
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get the current weather for a location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {"type": "string"}
                            },
                            "required": ["location"]
                        }
                    }
                }
            ]
        });

        let request = provider
            .parse_chat_request(&request_json)
            .expect("JSON payload should resolve to a chat request");

        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.system_prompt.as_deref(), Some("You are helpful."));

        let tools = request.tools.expect("tools should be extracted from JSON");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "get_weather");

        match request.tool_choice {
            Some(ToolChoice::Specific(choice)) => {
                assert_eq!(choice.function.name, "get_weather");
            }
            other => panic!("expected specific tool choice, found {other:?}"),
        }

        assert!(request.stream);
        assert_eq!(request.max_tokens, Some(256));
        assert_eq!(request.temperature, Some(0.5));
    }

    #[test]
    fn parse_chat_request_supports_string_tool_choice() {
        let provider = OllamaProvider::from_config(None, None, None, None);
        let request_json = json!({
            "messages": [
                {"role": "user", "content": "Hi"}
            ],
            "tool_choice": "none"
        });

        let request = provider
            .parse_chat_request(&request_json)
            .expect("payload should resolve to a chat request");

        match request.tool_choice {
            Some(ToolChoice::None) => {}
            other => panic!("expected 'none' tool choice, found {other:?}"),
        }

        assert!(request.tools.is_none());
    }

    #[test]
    fn build_chat_request_propagates_tool_choice() {
        let provider = OllamaProvider::from_config(None, None, None, None);
        let tool = ToolDefinition::function(
            "get_weather".to_string(),
            "Get the current weather".to_string(),
            json!({
                "type": "object",
                "properties": {"location": {"type": "string"}},
                "required": ["location"]
            }),
        );

        let request = LLMRequest {
            messages: vec![Message::user("What's the weather?".to_string())],
            system_prompt: None,
            tools: Some(vec![tool]),
            model: String::new(),
            max_tokens: None,
            temperature: None,
            stream: false,
            tool_choice: Some(ToolChoice::Any),
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        };

        let payload = provider
            .build_chat_request(&request, false)
            .expect("tool choice should serialize");

        assert_eq!(payload.stream, false);
        assert_eq!(payload.tool_choice, Some(json!("required")));
        assert_eq!(payload.think, None);
    }

    #[test]
    fn chat_endpoint_rejects_invalid_base_url() {
        let provider = OllamaProvider::from_config(
            Some("key".to_string()),
            None,
            Some("not a valid url".to_string()),
            None,
        );

        let result = provider.chat_endpoint();

        match result {
            Err(LLMError::InvalidRequest(message)) => {
                assert!(message.contains("Invalid Ollama base URL"));
            }
            other => panic!("expected invalid request error, found {other:?}"),
        }
    }

    #[test]
    fn map_ollama_error_detects_authentication_failures() {
        let err = OllamaError::Other("401 Unauthorized".to_string());
        let mapped = OllamaProvider::map_ollama_error(err);

        match mapped {
            LLMError::Authentication(message) => {
                assert!(message.to_lowercase().contains("unauthorized"));
            }
            other => panic!("expected authentication error, found {other:?}"),
        }
    }

    #[test]
    fn chat_endpoint_rejects_droid_v1_base_url() {
        let provider = OllamaProvider::from_config(
            None,
            None,
            Some("http://localhost:11434/v1".to_string()),
            None,
        );

        let error = provider
            .chat_endpoint()
            .expect_err("/v1 base URLs should be rejected with guidance");

        match error {
            LLMError::InvalidRequest(message) => {
                assert!(message.contains("/v1"));
                assert!(message.contains("Droid"));
            }
            other => panic!("expected invalid request error, found {other:?}"),
        }
    }

    #[test]
    fn build_chat_request_sets_think_when_reasoning_effort_requested() {
        let provider = OllamaProvider::from_config(None, None, None, None);

        let request = LLMRequest {
            messages: vec![Message::user("Solve the puzzle".to_string())],
            system_prompt: None,
            tools: None,
            model: String::new(),
            max_tokens: None,
            temperature: None,
            stream: false,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: Some(ReasoningEffortLevel::Medium),
        };

        let payload = provider
            .build_chat_request(&request, true)
            .expect("reasoning flag should serialize");

        assert_eq!(payload.think, Some(true));
    }

    #[test]
    fn supported_models_include_recent_coding_models() {
        let provider = OllamaProvider::from_config(None, None, None, None);
        let supported = provider.supported_models();

        assert!(
            supported
                .iter()
                .any(|model| model == models::ollama::GLM_4_6_CLOUD)
        );
        assert!(
            supported
                .iter()
                .any(|model| model == models::ollama::QWEN3_CODER_480B_CLOUD)
        );
    }

    #[test]
    fn map_ollama_error_detects_rate_limit() {
        let err = OllamaError::Other("429 Too Many Requests".to_string());
        let mapped = OllamaProvider::map_ollama_error(err);

        match mapped {
            LLMError::RateLimit => {}
            other => panic!("expected rate limit error, found {other:?}"),
        }
    }

    #[test]
    fn reasoning_delta_emits_incremental_thinking() {
        let mut accumulated = String::new();
        let first = "Step 1".to_string();
        let delta_one = OllamaProvider::reasoning_delta(&mut accumulated, Some(&first));
        assert_eq!(delta_one.as_deref(), Some("Step 1"));

        let second = "Step 1\nStep 2".to_string();
        let delta_two = OllamaProvider::reasoning_delta(&mut accumulated, Some(&second));
        assert_eq!(delta_two.as_deref(), Some("\nStep 2"));

        let shorter = "Step 1".to_string();
        let delta_three = OllamaProvider::reasoning_delta(&mut accumulated, Some(&shorter));
        assert!(delta_three.is_none());
    }

    #[test]
    fn stream_state_emits_reasoning_then_tokens() {
        let mut state = OllamaStreamState::default();

        let chunk_one = ChatMessageResponse {
            model: models::ollama::GLM_4_6_CLOUD.to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            message: ChatMessage {
                role: MessageRole::Assistant,
                content: "Hello".to_string(),
                tool_calls: Vec::new(),
                images: None,
                thinking: Some("Step 1".to_string()),
            },
            done: false,
            final_data: None,
        };

        let payload_one = serde_json::to_string(&chunk_one).expect("chunk should serialize");
        let events_one = state
            .push_bytes(format!("data: {payload_one}\n").as_bytes())
            .expect("first chunk should parse");

        assert_eq!(events_one.len(), 2);
        assert!(matches!(
            &events_one[0],
            LLMStreamEvent::Reasoning { delta } if delta == "Step 1"
        ));
        assert!(matches!(
            &events_one[1],
            LLMStreamEvent::Token { delta } if delta == "Hello"
        ));
        assert!(!state.is_done());

        let chunk_two = ChatMessageResponse {
            model: models::ollama::GLM_4_6_CLOUD.to_string(),
            created_at: "2025-01-01T00:00:01Z".to_string(),
            message: ChatMessage {
                role: MessageRole::Assistant,
                content: " World".to_string(),
                tool_calls: Vec::new(),
                images: None,
                thinking: Some("Step 1\nStep 2".to_string()),
            },
            done: true,
            final_data: None,
        };

        let payload_two = serde_json::to_string(&chunk_two).expect("chunk should serialize");
        let events_two = state
            .push_bytes(format!("data: {payload_two}\n").as_bytes())
            .expect("second chunk should parse");

        assert_eq!(events_two.len(), 2);
        assert!(matches!(
            &events_two[0],
            LLMStreamEvent::Reasoning { delta } if delta == "\nStep 2"
        ));
        assert!(matches!(
            &events_two[1],
            LLMStreamEvent::Token { delta } if delta == " World"
        ));
        assert!(state.is_done());

        let finalize_events = state.finalize().expect("finalization should succeed");
        assert!(finalize_events.is_empty());

        let final_chunk = state
            .into_final_response()
            .expect("final chunk should be available");

        assert_eq!(final_chunk.message.content, "Hello World");
        assert_eq!(
            final_chunk.message.thinking.as_deref(),
            Some("Step 1\nStep 2")
        );
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
