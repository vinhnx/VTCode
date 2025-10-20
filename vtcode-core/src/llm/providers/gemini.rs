use crate::config::constants::{models, urls};
use crate::config::core::{GeminiPromptCacheMode, GeminiPromptCacheSettings, PromptCachingConfig};
use crate::gemini::function_calling::{
    FunctionCall as GeminiFunctionCall, FunctionCallingConfig, FunctionResponse,
};
use crate::gemini::models::SystemInstruction;
use crate::gemini::streaming::{
    StreamingCandidate, StreamingError, StreamingProcessor, StreamingResponse,
};
use crate::gemini::{
    Candidate, Content, FunctionDeclaration, GenerateContentRequest, GenerateContentResponse, Part,
    Tool, ToolConfig,
};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, FunctionCall, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream,
    LLMStreamEvent, Message, MessageRole, ToolCall, ToolChoice,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::common::{extract_prompt_cache_settings, override_base_url, resolve_model};

pub struct GeminiProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: GeminiPromptCacheSettings,
}

impl GeminiProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::GEMINI_2_5_FLASH_PREVIEW.to_string(),
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
        let model_value = resolve_model(model, models::GEMINI_2_5_FLASH_PREVIEW);

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
            |providers| &providers.gemini,
            |cfg, provider_settings| {
                cfg.enabled
                    && provider_settings.enabled
                    && provider_settings.mode != GeminiPromptCacheMode::Off
            },
        );

        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: override_base_url(urls::GEMINI_API_BASE, base_url),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }
}

#[async_trait]
impl LLMProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let gemini_request = self.convert_to_gemini_request(&request)?;

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, request.model, self.api_key
        );

        let response = self
            .http_client
            .post(&url)
            .json(&gemini_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("Gemini", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            // Handle specific HTTP status codes
            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                "Gemini",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        let gemini_response: GenerateContentResponse = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "Gemini",
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        Self::convert_from_gemini_response(gemini_response)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let gemini_request = self.convert_to_gemini_request(&request)?;

        let url = format!(
            "{}/models/{}:streamGenerateContent?key={}",
            self.base_url, request.model, self.api_key
        );

        let response = self
            .http_client
            .post(&url)
            .json(&gemini_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("Gemini", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 || status.as_u16() == 403 {
                let formatted_error = error_display::format_llm_error(
                    "Gemini",
                    &format!("HTTP {}: {}", status, error_text),
                );
                return Err(LLMError::Authentication(formatted_error));
            }

            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                "Gemini",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let completion_sender = event_tx.clone();

        tokio::spawn(async move {
            let mut processor = StreamingProcessor::new();
            let token_sender = completion_sender.clone();
            let mut aggregated_text = String::new();
            let mut on_chunk = |chunk: &str| -> Result<(), StreamingError> {
                if chunk.is_empty() {
                    return Ok(());
                }

                aggregated_text.push_str(chunk);

                token_sender
                    .send(Ok(LLMStreamEvent::Token {
                        delta: chunk.to_string(),
                    }))
                    .map_err(|_| StreamingError::StreamingError {
                        message: "Streaming consumer dropped".to_string(),
                        partial_content: Some(chunk.to_string()),
                    })?;
                Ok(())
            };

            let result = processor.process_stream(response, &mut on_chunk).await;
            match result {
                Ok(mut streaming_response) => {
                    if streaming_response.candidates.is_empty()
                        && !aggregated_text.trim().is_empty()
                    {
                        streaming_response.candidates.push(StreamingCandidate {
                            content: Content {
                                role: "model".to_string(),
                                parts: vec![Part::Text {
                                    text: aggregated_text.clone(),
                                }],
                            },
                            finish_reason: None,
                            index: Some(0),
                        });
                    }

                    match Self::convert_from_streaming_response(streaming_response) {
                        Ok(final_response) => {
                            let _ = completion_sender.send(Ok(LLMStreamEvent::Completed {
                                response: final_response,
                            }));
                        }
                        Err(err) => {
                            let _ = completion_sender.send(Err(err));
                        }
                    }
                }
                Err(error) => {
                    let mapped = Self::map_streaming_error(error);
                    let _ = completion_sender.send(Err(mapped));
                }
            }
        });

        drop(event_tx);

        let stream = {
            let mut receiver = event_rx;
            try_stream! {
                while let Some(event) = receiver.recv().await {
                    yield event?;
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            models::google::GEMINI_2_5_FLASH_PREVIEW.to_string(),
            models::google::GEMINI_2_5_PRO.to_string(),
        ]
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if !self.supported_models().contains(&request.model) {
            let formatted_error = error_display::format_llm_error(
                "Gemini",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted_error));
        }
        Ok(())
    }
}

impl GeminiProvider {
    fn convert_to_gemini_request(
        &self,
        request: &LLMRequest,
    ) -> Result<GenerateContentRequest, LLMError> {
        if self.prompt_cache_enabled
            && matches!(
                self.prompt_cache_settings.mode,
                GeminiPromptCacheMode::Explicit
            )
        {
            // Explicit cache handling requires separate cache lifecycle APIs which are
            // coordinated outside of the request payload. Placeholder ensures we surface
            // configuration usage even when implicit mode is active.
        }

        let mut call_map: HashMap<String, String> = HashMap::new();
        for message in &request.messages {
            if message.role == MessageRole::Assistant
                && let Some(tool_calls) = &message.tool_calls
            {
                for tool_call in tool_calls {
                    call_map.insert(tool_call.id.clone(), tool_call.function.name.clone());
                }
            }
        }

        let mut contents: Vec<Content> = Vec::new();
        for message in &request.messages {
            if message.role == MessageRole::System {
                continue;
            }

            let mut parts: Vec<Part> = Vec::new();
            if message.role != MessageRole::Tool && !message.content.is_empty() {
                parts.push(Part::Text {
                    text: message.content.clone(),
                });
            }

            if message.role == MessageRole::Assistant
                && let Some(tool_calls) = &message.tool_calls
            {
                for tool_call in tool_calls {
                    let parsed_args = serde_json::from_str(&tool_call.function.arguments)
                        .unwrap_or_else(|_| json!({}));
                    parts.push(Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: tool_call.function.name.clone(),
                            args: parsed_args,
                            id: Some(tool_call.id.clone()),
                        },
                    });
                }
            }

            if message.role == MessageRole::Tool {
                if let Some(tool_call_id) = &message.tool_call_id {
                    let func_name = call_map
                        .get(tool_call_id)
                        .cloned()
                        .unwrap_or_else(|| tool_call_id.clone());
                    let response_text = serde_json::from_str::<Value>(&message.content)
                        .map(|value| {
                            serde_json::to_string_pretty(&value)
                                .unwrap_or_else(|_| message.content.clone())
                        })
                        .unwrap_or_else(|_| message.content.clone());

                    let response_payload = json!({
                        "name": func_name.clone(),
                        "content": [{
                            "text": response_text
                        }]
                    });

                    parts.push(Part::FunctionResponse {
                        function_response: FunctionResponse {
                            name: func_name,
                            response: response_payload,
                        },
                    });
                } else if !message.content.is_empty() {
                    parts.push(Part::Text {
                        text: message.content.clone(),
                    });
                }
            }

            if !parts.is_empty() {
                contents.push(Content {
                    role: message.role.as_gemini_str().to_string(),
                    parts,
                });
            }
        }

        let tools: Option<Vec<Tool>> = request.tools.as_ref().map(|definitions| {
            definitions
                .iter()
                .map(|tool| Tool {
                    function_declarations: vec![FunctionDeclaration {
                        name: tool.function.name.clone(),
                        description: tool.function.description.clone(),
                        parameters: sanitize_function_parameters(tool.function.parameters.clone()),
                    }],
                })
                .collect()
        });

        let mut generation_config = Map::new();
        if let Some(max_tokens) = request.max_tokens {
            generation_config.insert("maxOutputTokens".to_string(), json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            generation_config.insert("temperature".to_string(), json!(temp));
        }
        let has_tools = request
            .tools
            .as_ref()
            .map(|defs| !defs.is_empty())
            .unwrap_or(false);
        let tool_config = if has_tools || request.tool_choice.is_some() {
            Some(match request.tool_choice.as_ref() {
                Some(ToolChoice::None) => ToolConfig {
                    function_calling_config: FunctionCallingConfig::none(),
                },
                Some(ToolChoice::Any) => ToolConfig {
                    function_calling_config: FunctionCallingConfig::any(),
                },
                Some(ToolChoice::Specific(spec)) => {
                    let mut config = FunctionCallingConfig::any();
                    if spec.tool_type == "function" {
                        config.allowed_function_names = Some(vec![spec.function.name.clone()]);
                    }
                    ToolConfig {
                        function_calling_config: config,
                    }
                }
                _ => ToolConfig::auto(),
            })
        } else {
            None
        };

        Ok(GenerateContentRequest {
            contents,
            tools,
            tool_config,
            system_instruction: request
                .system_prompt
                .as_ref()
                .map(|text| SystemInstruction::new(text.clone())),
            generation_config: if generation_config.is_empty() {
                None
            } else {
                Some(Value::Object(generation_config))
            },
            reasoning_config: None,
        })
    }

    fn convert_from_gemini_response(
        response: GenerateContentResponse,
    ) -> Result<LLMResponse, LLMError> {
        let mut candidates = response.candidates.into_iter();
        let candidate = candidates.next().ok_or_else(|| {
            let formatted_error =
                error_display::format_llm_error("Gemini", "No candidate in response");
            LLMError::Provider(formatted_error)
        })?;

        if candidate.content.parts.is_empty() {
            return Ok(LLMResponse {
                content: Some(String::new()),
                tool_calls: None,
                usage: None,
                finish_reason: FinishReason::Stop,
                reasoning: None,
            });
        }

        let mut text_content = String::new();
        let mut tool_calls = Vec::new();

        for part in candidate.content.parts {
            match part {
                Part::Text { text } => {
                    text_content.push_str(&text);
                }
                Part::FunctionCall { function_call } => {
                    let call_id = function_call.id.clone().unwrap_or_else(|| {
                        format!(
                            "call_{}_{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos(),
                            tool_calls.len()
                        )
                    });
                    tool_calls.push(ToolCall {
                        id: call_id,
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: function_call.name,
                            arguments: serde_json::to_string(&function_call.args)
                                .unwrap_or_else(|_| "{}".to_string()),
                        },
                    });
                }
                Part::FunctionResponse { .. } => {
                    // Ignore echoed tool responses to avoid duplicating tool output
                }
            }
        }

        let finish_reason = match candidate.finish_reason.as_deref() {
            Some("STOP") => FinishReason::Stop,
            Some("MAX_TOKENS") => FinishReason::Length,
            Some("SAFETY") => FinishReason::ContentFilter,
            Some("FUNCTION_CALL") => FinishReason::ToolCalls,
            Some(other) => FinishReason::Error(other.to_string()),
            None => FinishReason::Stop,
        };

        Ok(LLMResponse {
            content: if text_content.is_empty() {
                None
            } else {
                Some(text_content)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage: None,
            finish_reason,
            reasoning: None,
        })
    }

    fn convert_from_streaming_response(
        response: StreamingResponse,
    ) -> Result<LLMResponse, LLMError> {
        let converted_candidates: Vec<Candidate> = response
            .candidates
            .into_iter()
            .map(|candidate| Candidate {
                content: candidate.content,
                finish_reason: candidate.finish_reason,
            })
            .collect();

        let converted = GenerateContentResponse {
            candidates: converted_candidates,
            prompt_feedback: None,
            usage_metadata: response.usage_metadata,
        };

        Self::convert_from_gemini_response(converted)
    }

    fn map_streaming_error(error: StreamingError) -> LLMError {
        match error {
            StreamingError::NetworkError { message, .. } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!("Network error: {}", message),
                );
                LLMError::Network(formatted)
            }
            StreamingError::ApiError {
                status_code,
                message,
                ..
            } => {
                if status_code == 401 || status_code == 403 {
                    let formatted = error_display::format_llm_error(
                        "Gemini",
                        &format!("HTTP {}: {}", status_code, message),
                    );
                    LLMError::Authentication(formatted)
                } else if status_code == 429 {
                    LLMError::RateLimit
                } else {
                    let formatted = error_display::format_llm_error(
                        "Gemini",
                        &format!("API error ({}): {}", status_code, message),
                    );
                    LLMError::Provider(formatted)
                }
            }
            StreamingError::ParseError { message, .. } => {
                let formatted =
                    error_display::format_llm_error("Gemini", &format!("Parse error: {}", message));
                LLMError::Provider(formatted)
            }
            StreamingError::TimeoutError {
                operation,
                duration,
            } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!(
                        "Streaming timeout during {} after {:?}",
                        operation, duration
                    ),
                );
                LLMError::Network(formatted)
            }
            StreamingError::ContentError { message } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!("Content error: {}", message),
                );
                LLMError::Provider(formatted)
            }
            StreamingError::StreamingError { message, .. } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!("Streaming error: {}", message),
                );
                LLMError::Provider(formatted)
            }
        }
    }
}

pub fn sanitize_function_parameters(parameters: Value) -> Value {
    match parameters {
        Value::Object(mut map) => {
            // Remove ALL unsupported fields for Gemini API to prevent any schema violations
            map.remove("additionalProperties");
            map.remove("oneOf");
            map.remove("anyOf");
            map.remove("allOf");
            // Process remaining properties recursively
            let mut sanitized = Map::new();
            for (key, value) in map {
                sanitized.insert(key, sanitize_function_parameters(value));
            }
            Value::Object(sanitized)
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(sanitize_function_parameters)
                .collect(),
        ),
        other => other,
    }
}

#[async_trait]
impl LLMClient for GeminiProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        // Check if the prompt is a serialized GenerateContentRequest
        let request = if prompt.starts_with('{') && prompt.contains("\"contents\"") {
            // Try to parse as JSON GenerateContentRequest
            match serde_json::from_str::<crate::gemini::GenerateContentRequest>(prompt) {
                Ok(gemini_request) => {
                    // Convert GenerateContentRequest to LLMRequest
                    let mut messages = Vec::new();
                    let mut system_prompt = None;

                    // Convert contents to messages
                    for content in &gemini_request.contents {
                        let role = match content.role.as_str() {
                            crate::config::constants::message_roles::USER => MessageRole::User,
                            "model" => MessageRole::Assistant,
                            crate::config::constants::message_roles::SYSTEM => {
                                // Extract system message
                                let text = content
                                    .parts
                                    .iter()
                                    .filter_map(|part| part.as_text())
                                    .collect::<Vec<_>>()
                                    .join("");
                                system_prompt = Some(text);
                                continue;
                            }
                            _ => MessageRole::User, // Default to user
                        };

                        let content_text = content
                            .parts
                            .iter()
                            .filter_map(|part| part.as_text())
                            .collect::<Vec<_>>()
                            .join("");

                        messages.push(Message {
                            role,
                            content: content_text,
                            reasoning: None,
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }

                    // Convert tools if present
                    let tools = gemini_request.tools.as_ref().map(|gemini_tools| {
                        gemini_tools
                            .iter()
                            .flat_map(|tool| &tool.function_declarations)
                            .map(|decl| crate::llm::provider::ToolDefinition {
                                tool_type: "function".to_string(),
                                function: crate::llm::provider::FunctionDefinition {
                                    name: decl.name.clone(),
                                    description: decl.description.clone(),
                                    parameters: decl.parameters.clone(),
                                },
                            })
                            .collect::<Vec<_>>()
                    });

                    let llm_request = LLMRequest {
                        messages,
                        system_prompt,
                        tools,
                        model: self.model.clone(),
                        max_tokens: gemini_request
                            .generation_config
                            .as_ref()
                            .and_then(|config| config.get("maxOutputTokens"))
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32),
                        temperature: gemini_request
                            .generation_config
                            .as_ref()
                            .and_then(|config| config.get("temperature"))
                            .and_then(|v| v.as_f64())
                            .map(|v| v as f32),
                        stream: false,
                        tool_choice: None,
                        parallel_tool_calls: None,
                        parallel_tool_config: None,
                        reasoning_effort: None,
                    };

                    // Use the standard LLMProvider generate method
                    let response = LLMProvider::generate(self, llm_request).await?;

                    // If there are tool calls, include them in the response content as JSON
                    let content = if let Some(tool_calls) = &response.tool_calls {
                        if !tool_calls.is_empty() {
                            // Create a JSON structure that the agent can parse
                            let tool_call_json = json!({
                                "tool_calls": tool_calls.iter().map(|tc| {
                                    json!({
                                        "function": {
                                            "name": tc.function.name,
                                            "arguments": tc.function.arguments
                                        }
                                    })
                                }).collect::<Vec<_>>()
                            });
                            tool_call_json.to_string()
                        } else {
                            response.content.unwrap_or("".to_string())
                        }
                    } else {
                        response.content.unwrap_or("".to_string())
                    };

                    return Ok(llm_types::LLMResponse {
                        content,
                        model: self.model.clone(),
                        usage: response.usage.map(|u| llm_types::Usage {
                            prompt_tokens: u.prompt_tokens as usize,
                            completion_tokens: u.completion_tokens as usize,
                            total_tokens: u.total_tokens as usize,
                            cached_prompt_tokens: u.cached_prompt_tokens.map(|v| v as usize),
                            cache_creation_tokens: u.cache_creation_tokens.map(|v| v as usize),
                            cache_read_tokens: u.cache_read_tokens.map(|v| v as usize),
                        }),
                        reasoning: response.reasoning,
                    });
                }
                Err(_) => {
                    // Fallback: treat as regular prompt
                    LLMRequest {
                        messages: vec![Message {
                            role: MessageRole::User,
                            content: prompt.to_string(),
                            reasoning: None,
                            tool_calls: None,
                            tool_call_id: None,
                        }],
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
            }
        } else {
            // Fallback: treat as regular prompt
            LLMRequest {
                messages: vec![Message {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                    reasoning: None,
                    tool_calls: None,
                    tool_call_id: None,
                }],
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
        };

        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or("".to_string()),
            model: self.model.clone(),
            usage: response.usage.map(|u| llm_types::Usage {
                prompt_tokens: u.prompt_tokens as usize,
                completion_tokens: u.completion_tokens as usize,
                total_tokens: u.total_tokens as usize,
                cached_prompt_tokens: u.cached_prompt_tokens.map(|v| v as usize),
                cache_creation_tokens: u.cache_creation_tokens.map(|v| v as usize),
                cache_read_tokens: u.cache_read_tokens.map(|v| v as usize),
            }),
            reasoning: response.reasoning,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Gemini
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::models;
    use crate::llm::provider::{SpecificFunctionChoice, SpecificToolChoice, ToolDefinition};

    #[test]
    fn convert_to_gemini_request_maps_history_and_system_prompt() {
        let provider = GeminiProvider::new("test-key".to_string());
        let mut assistant_message = Message::assistant("Sure thing".to_string());
        assistant_message.tool_calls = Some(vec![ToolCall::function(
            "call_1".to_string(),
            "list_files".to_string(),
            json!({ "path": "." }).to_string(),
        )]);

        let tool_response =
            Message::tool_response("call_1".to_string(), json!({ "result": "ok" }).to_string());

        let tool_def = ToolDefinition::function(
            "list_files".to_string(),
            "List files".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        );

        let request = LLMRequest {
            messages: vec![
                Message::user("hello".to_string()),
                assistant_message,
                tool_response,
            ],
            system_prompt: Some("System prompt".to_string()),
            tools: Some(vec![tool_def]),
            model: models::google::GEMINI_2_5_FLASH_PREVIEW.to_string(),
            max_tokens: Some(256),
            temperature: Some(0.4),
            stream: false,
            tool_choice: Some(ToolChoice::Specific(SpecificToolChoice {
                tool_type: "function".to_string(),
                function: SpecificFunctionChoice {
                    name: "list_files".to_string(),
                },
            })),
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        };

        let gemini_request = provider
            .convert_to_gemini_request(&request)
            .expect("conversion should succeed");

        let system_instruction = gemini_request
            .system_instruction
            .expect("system instruction should be present");
        assert!(matches!(
            system_instruction.parts.as_slice(),
            [Part::Text { text }] if text == "System prompt"
        ));

        assert_eq!(gemini_request.contents.len(), 3);
        assert_eq!(gemini_request.contents[0].role, "user");
        assert!(
            gemini_request.contents[1]
                .parts
                .iter()
                .any(|part| matches!(part, Part::FunctionCall { .. }))
        );
        let tool_part = gemini_request.contents[2]
            .parts
            .iter()
            .find_map(|part| match part {
                Part::FunctionResponse { function_response } => Some(function_response),
                _ => None,
            })
            .expect("tool response part should exist");
        assert_eq!(tool_part.name, "list_files");
    }

    #[test]
    fn convert_from_gemini_response_extracts_tool_calls() {
        let response = GenerateContentResponse {
            candidates: vec![crate::gemini::Candidate {
                content: Content {
                    role: "model".to_string(),
                    parts: vec![
                        Part::Text {
                            text: "Here you go".to_string(),
                        },
                        Part::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: "list_files".to_string(),
                                args: json!({ "path": "." }),
                                id: Some("call_1".to_string()),
                            },
                        },
                    ],
                },
                finish_reason: Some("FUNCTION_CALL".to_string()),
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let llm_response = GeminiProvider::convert_from_gemini_response(response)
            .expect("conversion should succeed");

        assert_eq!(llm_response.content.as_deref(), Some("Here you go"));
        let calls = llm_response
            .tool_calls
            .expect("tool call should be present");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "list_files");
        assert!(calls[0].function.arguments.contains("path"));
        assert_eq!(llm_response.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn sanitize_function_parameters_removes_additional_properties() {
        let parameters = json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "additionalProperties": false
                }
            },
            "additionalProperties": false
        });

        let sanitized = sanitize_function_parameters(parameters);
        let root = sanitized
            .as_object()
            .expect("root parameters should remain an object");
        assert!(!root.contains_key("additionalProperties"));

        let nested = root
            .get("properties")
            .and_then(|value| value.as_object())
            .and_then(|props| props.get("input"))
            .and_then(|value| value.as_object())
            .expect("nested object should be preserved");
        assert!(!nested.contains_key("additionalProperties"));
    }
}
