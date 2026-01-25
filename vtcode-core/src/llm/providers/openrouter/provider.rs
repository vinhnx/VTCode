#![allow(clippy::collapsible_if, clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, OpenRouterPromptCacheSettings, PromptCachingConfig};
use crate::config::models::ModelId;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageContent, MessageRole, ToolChoice, ToolDefinition, Usage,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client as HttpClient, Response, StatusCode};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::str::FromStr;
use crate::llm::providers::{
    ReasoningBuffer, TagStreamSanitizer,
    common::{
        convert_usage_to_llm_types, extract_prompt_cache_settings, override_base_url,
        parse_client_prompt_common, resolve_model,
    },
    shared::{
        StreamAssemblyError, StreamFragment, ToolCallBuilder, extract_data_payload,
        find_sse_boundary, parse_openai_tool_calls,
    },
};
use super::stream_decoder::{
    OpenRouterStreamTelemetry, finalize_stream_response, parse_stream_payload,
};

pub struct OpenRouterProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenRouterPromptCacheSettings,
}

impl OpenRouterProvider {
    const TOOL_UNSUPPORTED_ERROR: &'static str = "No endpoints found that support tool use";

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::openrouter::DEFAULT_MODEL.to_string(),
            None,
            None,
            TimeoutsConfig::default(),
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, TimeoutsConfig::default())
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            api_key,
            http_client,
            base_url,
            model,
            prompt_cache_enabled: false,
            prompt_cache_settings: OpenRouterPromptCacheSettings::default(),
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::openrouter::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            model_value,
            prompt_cache,
            base_url,
            timeouts.unwrap_or_default(),
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        timeouts: TimeoutsConfig,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.openrouter,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::OPENROUTER_API_BASE,
                base_url,
                Some(env_vars::OPENROUTER_BASE_URL),
            ),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn parse_client_prompt(&self, prompt: &str) -> LLMRequest {
        parse_client_prompt_common(prompt, &self.model, |value| self.parse_chat_request(value))
    }

    pub(super) fn is_gpt5_codex_model(model: &str) -> bool {
        model == models::openrouter::OPENAI_GPT_5_CODEX
    }

    pub(super) fn resolve_model<'a>(&'a self, request: &'a LLMRequest) -> &'a str {
        if request.model.trim().is_empty() {
            self.model.as_str()
        } else {
            request.model.as_str()
        }
    }

    fn uses_responses_api_for(&self, request: &LLMRequest) -> bool {
        Self::is_gpt5_codex_model(self.resolve_model(request))
    }

    fn request_includes_tools(request: &LLMRequest) -> bool {
        request
            .tools
            .as_ref()
            .map(|tools| !tools.is_empty())
            .unwrap_or(false)
    }

    fn enforce_tool_capabilities<'a>(&'a self, request: &'a LLMRequest) -> Cow<'a, LLMRequest> {
        let resolved_model = self.resolve_model(request);
        let tools_requested = Self::request_includes_tools(request);
        let tool_restricted = if let Ok(model_id) = ModelId::from_str(resolved_model) {
            !model_id.supports_tool_calls()
        } else {
            models::openrouter::TOOL_UNAVAILABLE_MODELS.contains(&resolved_model)
        };

        if tools_requested && tool_restricted {
            Cow::Owned(Self::tool_free_request(request))
        } else {
            Cow::Borrowed(request)
        }
    }

    fn tool_free_request(original: &LLMRequest) -> LLMRequest {
        let mut sanitized = original.clone();
        sanitized.tools = None;
        sanitized.tool_choice = Some(ToolChoice::None);
        sanitized.parallel_tool_calls = None;
        sanitized.parallel_tool_config = None;

        let mut normalized_messages: Vec<Message> = Vec::with_capacity(original.messages.len());

        for message in &original.messages {
            match message.role {
                MessageRole::Assistant => {
                    let mut cleaned = message.clone();
                    cleaned.tool_calls = None;
                    cleaned.tool_call_id = None;

                    let content_text = cleaned.content.as_text();
                    let has_content = !content_text.trim().is_empty();
                    if has_content || cleaned.reasoning.is_some() {
                        normalized_messages.push(cleaned);
                    }
                }
                MessageRole::Tool => {
                    let content_text = message.content.as_text();
                    if content_text.trim().is_empty() {
                        continue;
                    }

                    let mut converted = Message::user(content_text.into_owned());
                    converted.reasoning = message.reasoning.clone();
                    normalized_messages.push(converted);
                }
                _ => {
                    normalized_messages.push(message.clone());
                }
            }
        }

        sanitized.messages = normalized_messages;
        sanitized
    }

    fn build_provider_payload(&self, request: &LLMRequest) -> Result<(Value, String), LLMError> {
        if self.uses_responses_api_for(request) {
            Ok((
                self.convert_to_openrouter_responses_format(request)?,
                format!("{}/responses", self.base_url),
            ))
        } else {
            Ok((
                self.convert_to_openrouter_format(request)?,
                format!("{}/chat/completions", self.base_url),
            ))
        }
    }

    async fn dispatch_request(&self, url: &str, payload: &Value) -> Result<Response, LLMError> {
        self.http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenRouter", &format!("Network error: {}", e));
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })
    }

    fn is_tool_unsupported_error(status: StatusCode, body: &str) -> bool {
        status == StatusCode::NOT_FOUND && body.contains(Self::TOOL_UNSUPPORTED_ERROR)
    }

    async fn send_with_tool_fallback(
        &self,
        request: &LLMRequest,
        stream_override: Option<bool>,
    ) -> Result<Response, LLMError> {
        let adjusted_request = self.enforce_tool_capabilities(request);
        let request_ref = adjusted_request.as_ref();
        let request_with_tools = Self::request_includes_tools(request_ref);

        let (mut payload, url) = self.build_provider_payload(request_ref)?;
        if let Some(stream_flag) = stream_override {
            payload["stream"] = Value::Bool(stream_flag);
        }

        let response = self.dispatch_request(&url, &payload).await?;
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();

        if status.as_u16() == 429 || error_text.contains("quota") {
            return Err(LLMError::RateLimit { metadata: None });
        }

        if request_with_tools && Self::is_tool_unsupported_error(status, &error_text) {
            let fallback_request = Self::tool_free_request(request_ref);
            let (mut fallback_payload, fallback_url) =
                self.build_provider_payload(&fallback_request)?;
            if let Some(stream_flag) = stream_override {
                fallback_payload["stream"] = Value::Bool(stream_flag);
            }

            let fallback_response = self
                .dispatch_request(&fallback_url, &fallback_payload)
                .await?;
            if fallback_response.status().is_success() {
                return Ok(fallback_response);
            }

            let fallback_status = fallback_response.status();
            let fallback_text = fallback_response.text().await.unwrap_or_default();

            if fallback_status.as_u16() == 429 || fallback_text.contains("quota") {
                return Err(LLMError::RateLimit { metadata: None });
            }

            let combined_error = format!(
                "HTTP {}: {} | Tool fallback failed with HTTP {}: {}",
                status, error_text, fallback_status, fallback_text
            );
            let formatted_error = error_display::format_llm_error("OpenRouter", &combined_error);
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        // Use unified error parsing for consistent error categorization
        use crate::llm::providers::error_handling::parse_api_error;
        Err(parse_api_error("OpenRouter", status, &error_text))
    }

    fn parse_chat_request(&self, value: &Value) -> Option<LLMRequest> {
        let messages_value = value.get("messages")?.as_array()?;
        let mut system_prompt = None;
        let mut messages = Vec::new();

        for entry in messages_value {
            let role = entry
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or(crate::config::constants::message_roles::USER);
            let content = entry.get("content");
            let text_content = content.map(Self::extract_content_text).unwrap_or_default();

            match role {
                "system" => {
                    if system_prompt.is_none() && !text_content.is_empty() {
                        system_prompt = Some(text_content);
                    }
                }
                "assistant" => {
                    let tool_calls = entry
                        .get("tool_calls")
                        .and_then(|tc| tc.as_array())
                        .map(|calls| parse_openai_tool_calls(calls))
                        .filter(|calls| !calls.is_empty());

                    let message = if let Some(calls) = tool_calls {
                        Message {
                            role: MessageRole::Assistant,
                            content: MessageContent::Text(text_content),
                            reasoning: None,
                            reasoning_details: None,
                            tool_calls: Some(calls),
                            tool_call_id: None,
                            origin_tool: None,
                        }
                    } else {
                        Message::assistant(text_content)
                    };
                    messages.push(message);
                }
                "tool" => {
                    let tool_call_id = entry
                        .get("tool_call_id")
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string());
                    let content_value = entry
                        .get("content")
                        .map(|value| {
                            if text_content.is_empty() {
                                value.to_string()
                            } else {
                                text_content.clone()
                            }
                        })
                        .unwrap_or_else(|| text_content.clone());
                    messages.push(Message {
                        role: MessageRole::Tool,
                        content: MessageContent::Text(content_value),
                        reasoning: None,
                        reasoning_details: None,
                        tool_calls: None,
                        tool_call_id,
                        origin_tool: None,
                    });
                }
                _ => {
                    messages.push(Message::user(text_content));
                }
            }
        }

        if messages.is_empty() {
            return None;
        }

        let tools = value.get("tools").and_then(|tools_value| {
            let tools_array = tools_value.as_array()?;
            let converted: Vec<_> = tools_array
                .iter()
                .filter_map(|tool| {
                    let function = tool.get("function")?;
                    let name = function.get("name").and_then(|n| n.as_str())?;
                    let description = function
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string();
                    let parameters = function
                        .get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    Some(ToolDefinition::function(
                        name.to_string(),
                        description,
                        parameters,
                    ))
                })
                .collect();

            if converted.is_empty() {
                None
            } else {
                Some(converted)
            }
        });

        let max_tokens = value
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let temperature = value
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);
        let stream = value
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let tool_choice = value.get("tool_choice").and_then(Self::parse_tool_choice);
        let parallel_tool_calls = value.get("parallel_tool_calls").and_then(|v| v.as_bool());
        let reasoning_effort = value
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .and_then(ReasoningEffortLevel::parse)
            .or_else(|| {
                value
                    .get("reasoning")
                    .and_then(|r| r.get("effort"))
                    .and_then(|effort| effort.as_str())
                    .and_then(ReasoningEffortLevel::parse)
            });

        let model = value
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&self.model)
            .to_string();

        Some(LLMRequest {
            messages,
            system_prompt,
            tools,
            model,
            max_tokens,
            temperature,
            stream,
            tool_choice,
            parallel_tool_calls,
            reasoning_effort,
            ..Default::default()
        })
    }

    fn extract_content_text(content: &Value) -> String {
        match content {
            Value::String(text) => text.to_string(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            part.get("content")
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string())
                        })
                })
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }

    fn parse_tool_choice(choice: &Value) -> Option<ToolChoice> {
        match choice {
            Value::String(value) => match value.as_str() {
                "auto" => Some(ToolChoice::auto()),
                "none" => Some(ToolChoice::none()),
                "required" => Some(ToolChoice::any()),
                _ => None,
            },
            Value::Object(map) => {
                let choice_type = map.get("type").and_then(|t| t.as_str())?;
                match choice_type {
                    "function" => map
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|name| ToolChoice::function(name.to_string())),
                    "auto" => Some(ToolChoice::auto()),
                    "none" => Some(ToolChoice::none()),
                    "any" | "required" => Some(ToolChoice::any()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

}

#[async_trait]
impl LLMProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn supports_streaming(&self) -> bool {
        // OpenAI requires ID verification for GPT-5 models, so we must disable streaming
        // for the OpenRouter variants as well since they proxy to OpenAI's backend
        if matches!(
            self.model.as_str(),
            models::openrouter::OPENAI_GPT_5
                | models::openrouter::OPENAI_GPT_5_CODEX
                | models::openrouter::OPENAI_GPT_5_CHAT
        ) {
            return false;
        }

        true
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };
        if let Ok(model_id) = ModelId::from_str(requested) {
            return model_id.is_reasoning_variant();
        }
        models::openrouter::REASONING_MODELS.contains(&requested)
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.supports_reasoning_effort(model)
    }

    fn supports_tools(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        if let Ok(model_id) = ModelId::from_str(requested) {
            return model_id.supports_tool_calls();
        }

        !models::openrouter::TOOL_UNAVAILABLE_MODELS.contains(&requested)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let response = self.send_with_tool_fallback(&request, Some(true)).await?;

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregated_content = String::new();
            let mut tool_call_builders: Vec<ToolCallBuilder> = Vec::new();
            let mut reasoning = ReasoningBuffer::default();
            let mut usage: Option<Usage> = None;
            let mut finish_reason = FinishReason::Stop;
            let mut done = false;
            let mut sanitizer = TagStreamSanitizer::new();
            let telemetry = OpenRouterStreamTelemetry;

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|err| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenRouter",
                        &format!("Streaming error: {}", err),
                    );
                    LLMError::Network { message: formatted_error, metadata: None }
                })?;

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = find_sse_boundary(&buffer) {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);

                    if let Some(data_payload) = extract_data_payload(&event) {
                        let trimmed_payload = data_payload.trim();
                        if trimmed_payload == "[DONE]" {
                            done = true;
                            break;
                        }

                        if !trimmed_payload.is_empty() {
                            let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                                StreamAssemblyError::InvalidPayload(err.to_string())
                                    .into_llm_error("OpenRouter")
                            })?;

                            if let Some(delta) = parse_stream_payload(
                                &payload,
                                &mut aggregated_content,
                                &mut tool_call_builders,
                                &mut reasoning,
                                &mut usage,
                                &mut finish_reason,
                                &telemetry,
                            ) {
                                for fragment in delta.into_fragments() {
                                    match fragment {
                                        StreamFragment::Content(text) if !text.is_empty() => {
                                            for event in sanitizer.process_chunk(&text) {
                                                match &event {
                                                    LLMStreamEvent::Token { delta } => {
                                                        yield LLMStreamEvent::Token { delta: delta.clone() };
                                                    }
                                                    LLMStreamEvent::Reasoning { delta } => {
                                                        yield LLMStreamEvent::Reasoning { delta: delta.clone() };
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        StreamFragment::Reasoning(text) if !text.is_empty() => {
                                            yield LLMStreamEvent::Reasoning { delta: text };
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                if done {
                    break;
                }
            }

            if !done && !buffer.trim().is_empty() {
                if let Some(data_payload) = extract_data_payload(&buffer) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload != "[DONE]" && !trimmed_payload.is_empty() {
                        let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                            StreamAssemblyError::InvalidPayload(err.to_string())
                                .into_llm_error("OpenRouter")
                        })?;

                        if let Some(delta) = parse_stream_payload(
                            &payload,
                            &mut aggregated_content,
                            &mut tool_call_builders,
                            &mut reasoning,
                            &mut usage,
                            &mut finish_reason,
                            &telemetry,
                        ) {
                            for fragment in delta.into_fragments() {
                                match fragment {
                                    StreamFragment::Content(text) if !text.is_empty() => {
                                        for event in sanitizer.process_chunk(&text) {
                                            match &event {
                                                LLMStreamEvent::Token { delta } => {
                                                    yield LLMStreamEvent::Token { delta: delta.clone() };
                                                }
                                                LLMStreamEvent::Reasoning { delta } => {
                                                    yield LLMStreamEvent::Reasoning { delta: delta.clone() };
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    StreamFragment::Reasoning(text) if !text.is_empty() => {
                                        yield LLMStreamEvent::Reasoning { delta: text };
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            // Finalize sanitizer and yield leftover events
            for event in sanitizer.finalize() {
                yield event;
            }

            let response = finalize_stream_response(
                aggregated_content,
                tool_call_builders,
                usage,
                finish_reason,
                reasoning,
            );

            yield LLMStreamEvent::Completed { response };
        };

        Ok(Box::pin(stream))
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if self.prompt_cache_enabled && self.prompt_cache_settings.propagate_provider_capabilities {
            // When enabled, vtcode forwards provider-specific cache_control markers directly
            // through the OpenRouter payload without further transformation.
        }

        if self.prompt_cache_enabled && self.prompt_cache_settings.report_savings {
            // Cache savings are surfaced via usage metrics parsed later in the response cycle.
        }

        let response = self.send_with_tool_fallback(&request, None).await?;

        let openrouter_response: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        self.parse_openrouter_response(openrouter_response)
    }

    fn supported_models(&self) -> Vec<String> {
        models::openrouter::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("OpenRouter", &err);
                return Err(LLMError::InvalidRequest {
                    message: formatted,
                    metadata: None,
                });
            }
        }

        if request.model.trim().is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "Model must be provided");
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for OpenRouterProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = self.parse_client_prompt(prompt);
        let request_model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: request_model,
            usage: response.usage.map(convert_usage_to_llm_types),
            reasoning: response.reasoning,
            reasoning_details: response.reasoning_details,
            request_id: response.request_id,
            organization_id: response.organization_id,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::OpenRouter
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::stream_decoder::parse_usage_value;
    use crate::llm::providers::shared::NoopStreamTelemetry;
    use serde_json::json;

    fn sample_tool() -> ToolDefinition {
        ToolDefinition::function(
            "fetch_data".to_string(),
            "Fetch data".to_string(),
            json!({
                "type": "object",
                "properties": {}
            }),
        )
    }

    fn request_with_tools(model: &str) -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user("hi".to_string())],
            tools: Some(vec![sample_tool()]),
            model: model.to_string(),
            tool_choice: Some(ToolChoice::Any),
            parallel_tool_calls: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn enforce_tool_capabilities_disables_tools_for_restricted_models() {
        let provider = OpenRouterProvider::with_model(
            "test-key".to_string(),
            models::openrouter::MOONSHOTAI_KIMI_K2_0905.to_string(),
        );
        let request = request_with_tools(models::openrouter::MOONSHOTAI_KIMI_K2_0905);

        match provider.enforce_tool_capabilities(&request) {
            Cow::Borrowed(_) => panic!("expected sanitized request"),
            Cow::Owned(sanitized) => {
                assert!(sanitized.tools.is_none());
                assert!(matches!(sanitized.tool_choice, Some(ToolChoice::None)));
                assert!(sanitized.parallel_tool_calls.is_none());
                assert_eq!(sanitized.model, models::openrouter::MOONSHOTAI_KIMI_K2_0905);
                assert_eq!(sanitized.messages, request.messages);
            }
        }
    }

    #[test]
    fn enforce_tool_capabilities_keeps_tools_for_supported_models() {
        let provider = OpenRouterProvider::with_model(
            "test-key".to_string(),
            models::openrouter::OPENAI_GPT_5.to_string(),
        );
        let request = request_with_tools(models::openrouter::OPENAI_GPT_5);

        match provider.enforce_tool_capabilities(&request) {
            Cow::Borrowed(borrowed) => {
                assert!(std::ptr::eq(borrowed, &request));
                assert!(borrowed.tools.as_ref().is_some());
            }
            Cow::Owned(_) => panic!("should not sanitize supported models"),
        }
    }

    #[test]
    fn test_parse_stream_payload_chat_chunk() {
        let payload = json!({
            "choices": [{
                "delta": {
                    "content": [
                        {"type": "output_text", "text": "Hello"}
                    ]
                }
            }]
        });

        let mut aggregated = String::new();
        let mut builders = Vec::new();
        let mut reasoning = ReasoningBuffer::default();
        let mut usage = None;
        let mut finish_reason = FinishReason::Stop;
        let telemetry = NoopStreamTelemetry::default();

        let delta = parse_stream_payload(
            &payload,
            &mut aggregated,
            &mut builders,
            &mut reasoning,
            &mut usage,
            &mut finish_reason,
            &telemetry,
        );

        let fragments = delta.expect("delta should exist").into_fragments();
        assert_eq!(
            fragments,
            vec![StreamFragment::Content("Hello".to_string())]
        );
        assert_eq!(aggregated, "Hello");
        assert!(builders.is_empty());
        assert!(usage.is_none());
        assert!(reasoning.finalize().is_none());
    }

    #[test]
    fn test_parse_stream_payload_response_delta() {
        let payload = json!({
            "type": "response.delta",
            "delta": {
                "type": "output_text_delta",
                "text": "Stream"
            }
        });

        let mut aggregated = String::new();
        let mut builders = Vec::new();
        let mut reasoning = ReasoningBuffer::default();
        let mut usage = None;
        let mut finish_reason = FinishReason::Stop;
        let telemetry = NoopStreamTelemetry::default();

        let delta = parse_stream_payload(
            &payload,
            &mut aggregated,
            &mut builders,
            &mut reasoning,
            &mut usage,
            &mut finish_reason,
            &telemetry,
        );

        let fragments = delta.expect("delta should exist").into_fragments();
        assert_eq!(
            fragments,
            vec![StreamFragment::Content("Stream".to_string())]
        );
        assert_eq!(aggregated, "Stream");
    }

    #[test]
    fn test_extract_data_payload_joins_multiline_events() {
        let event = ": keep-alive\n".to_string() + "data: {\"a\":1}\n" + "data: {\"b\":2}\n";
        let payload = extract_data_payload(&event);
        assert_eq!(payload.as_deref(), Some("{\"a\":1}\n{\"b\":2}"));
    }

    #[test]
    fn parse_usage_value_includes_cache_metrics() {
        let value = json!({
            "prompt_tokens": 120,
            "completion_tokens": 80,
            "total_tokens": 200,
            "prompt_cache_read_tokens": 90,
            "prompt_cache_write_tokens": 15
        });

        let usage = parse_usage_value(&value);
        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 80);
        assert_eq!(usage.total_tokens, 200);
        assert_eq!(usage.cached_prompt_tokens, Some(90));
        assert_eq!(usage.cache_read_tokens, Some(90));
        assert_eq!(usage.cache_creation_tokens, Some(15));
    }
}
