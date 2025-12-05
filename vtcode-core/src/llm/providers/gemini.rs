#![allow(clippy::collapsible_if)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{GeminiPromptCacheMode, GeminiPromptCacheSettings, PromptCachingConfig};
use crate::gemini::function_calling::{
    FunctionCall as GeminiFunctionCall, FunctionCallingConfig, FunctionResponse,
};
use crate::gemini::models::SystemInstruction;
use crate::gemini::streaming::{
    StreamingCandidate, StreamingConfig, StreamingError, StreamingProcessor, StreamingResponse,
};
use crate::gemini::{
    Candidate, Content, FunctionDeclaration, GenerateContentRequest, GenerateContentResponse, Part,
    Tool, ToolConfig,
};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, FunctionCall, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream,
    LLMStreamEvent, Message, MessageContent, MessageRole, ToolCall, ToolChoice,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing;
use vtcode_config::types::ReasoningEffortLevel;

use super::common::{extract_prompt_cache_settings, override_base_url, resolve_model};
use super::error_handling::{format_network_error, format_parse_error, is_rate_limit_error};

pub struct GeminiProvider {
    api_key: Arc<str>,
    http_client: HttpClient,
    base_url: Arc<str>,
    model: Arc<str>,
    prompt_cache_enabled: bool,
    prompt_cache_settings: GeminiPromptCacheSettings,
    timeouts: TimeoutsConfig,
}

impl GeminiProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::google::GEMINI_2_5_FLASH.to_string(),
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
        timeouts: TimeoutsConfig,
        prompt_cache_enabled: bool,
        prompt_cache_settings: GeminiPromptCacheSettings,
    ) -> Self {
        Self {
            api_key: Arc::from(api_key.as_str()),
            http_client,
            base_url: Arc::from(base_url.as_str()),
            model: Arc::from(model.as_str()),
            prompt_cache_enabled,
            prompt_cache_settings,
            timeouts,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,

        timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::google::GEMINI_2_5_FLASH);

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
            api_key: Arc::from(api_key.as_str()),
            http_client: HttpClient::new(),
            base_url: Arc::from(
                override_base_url(
                    urls::GEMINI_API_BASE,
                    base_url,
                    Some(env_vars::GEMINI_BASE_URL),
                )
                .as_str(),
            ),
            model: Arc::from(model.as_str()),
            prompt_cache_enabled,
            prompt_cache_settings,
            timeouts,
        }
    }

    /// Handle HTTP response errors and convert to appropriate LLMError.
    /// Uses shared rate limit detection from error_handling module.
    #[inline]
    fn handle_http_error(status: reqwest::StatusCode, error_text: &str) -> LLMError {
        let status_code = status.as_u16();

        // Handle authentication errors
        if status_code == 401 || status_code == 403 {
            let formatted_error = error_display::format_llm_error(
                "Gemini",
                &format!(
                    "Authentication failed: {}. Check your GOOGLE_API_KEY or GEMINI_API_KEY environment variable.",
                    error_text
                ),
            );
            return LLMError::Authentication(formatted_error);
        }

        // Handle rate limit and quota errors using shared detection
        if is_rate_limit_error(status_code, error_text) {
            return LLMError::RateLimit;
        }

        // Handle invalid request errors
        if status_code == 400 {
            let formatted_error = error_display::format_llm_error(
                "Gemini",
                &format!("Invalid request: {}", error_text),
            );
            return LLMError::InvalidRequest(formatted_error);
        }

        // Generic error for other cases
        let formatted_error =
            error_display::format_llm_error("Gemini", &format!("HTTP {}: {}", status, error_text));
        LLMError::Provider(formatted_error)
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

    fn supports_reasoning(&self, model: &str) -> bool {
        // Gemini 2.5 models support thinking/reasoning capability
        // Reference: https://ai.google.dev/gemini-api/docs/models
        models::google::REASONING_MODELS.contains(&model)
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
            .map_err(|e| format_network_error("Gemini", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::handle_http_error(status, &error_text));
        }

        let gemini_response: GenerateContentResponse = response
            .json()
            .await
            .map_err(|e| format_parse_error("Gemini", &e))?;

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
            .map_err(|e| format_network_error("Gemini", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Self::handle_http_error(status, &error_text));
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let completion_sender = event_tx.clone();

        let streaming_timeout = self.timeouts.streaming_ceiling_seconds;

        tokio::spawn(async move {
            let config = StreamingConfig::with_total_timeout(streaming_timeout);
            let mut processor = StreamingProcessor::with_config(config);
            let event_sender = completion_sender.clone();
            let mut aggregated_text = String::new();
            let mut _reasoning_buffer = crate::llm::providers::ReasoningBuffer::default();

            #[allow(clippy::collapsible_if)]
            let mut on_chunk = |chunk: &str| -> Result<(), StreamingError> {
                if chunk.is_empty() {
                    return Ok(());
                }

                if let Some(delta) = Self::apply_stream_delta(&mut aggregated_text, chunk) {
                    if delta.is_empty() {
                        return Ok(());
                    }

                    // Split any reasoning content from the delta
                    let (reasoning_segments, cleaned_delta) =
                        crate::llm::providers::split_reasoning_from_text(&delta);

                    // Send any extracted reasoning content
                    for segment in reasoning_segments {
                        if !segment.is_empty() {
                            event_sender
                                .send(Ok(LLMStreamEvent::Reasoning { delta: segment }))
                                .map_err(|_| StreamingError::StreamingError {
                                    message: "Streaming consumer dropped".to_string(),
                                    partial_content: Some(chunk.to_string()),
                                })?;
                        }
                    }

                    // Send the cleaned content if any remains
                    if let Some(cleaned) = cleaned_delta {
                        if !cleaned.is_empty() {
                            event_sender
                                .send(Ok(LLMStreamEvent::Token { delta: cleaned }))
                                .map_err(|_| StreamingError::StreamingError {
                                    message: "Streaming consumer dropped".to_string(),
                                    partial_content: Some(chunk.to_string()),
                                })?;
                        }
                    }
                }
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
                                    thought_signature: None,
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
        // Order: stable models first, then preview/experimental
        models::google::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if !models::google::SUPPORTED_MODELS
            .iter()
            .any(|m| *m == request.model)
        {
            let formatted_error = error_display::format_llm_error(
                "Gemini",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        // Validate token limits based on model capabilities
        if let Some(max_tokens) = request.max_tokens {
            let model = request.model.as_str();
            let max_output_tokens = if model.contains("2.5") || model.contains("3") {
                65536 // Gemini 2.5 and 3 models support 65K output tokens
            } else {
                8192 // Conservative default
            };

            if max_tokens > max_output_tokens {
                let formatted_error = error_display::format_llm_error(
                    "Gemini",
                    &format!(
                        "Requested max_tokens ({}) exceeds model limit ({}) for {}",
                        max_tokens, max_output_tokens, model
                    ),
                );
                return Err(LLMError::InvalidRequest(formatted_error));
            }
        }

        Ok(())
    }
}

impl GeminiProvider {
    /// Check if model supports context caching
    pub fn supports_caching(model: &str) -> bool {
        models::google::CACHING_MODELS.contains(&model)
    }

    /// Check if model supports code execution
    pub fn supports_code_execution(model: &str) -> bool {
        models::google::CODE_EXECUTION_MODELS.contains(&model)
    }

    /// Get maximum input token limit for a model
    pub fn max_input_tokens(model: &str) -> usize {
        if model.contains("2.5") || model.contains("3") || model.contains("2.0") {
            1_048_576 // 1M tokens for Gemini 2.x and 3.x models
        } else {
            // Conservative default for unknown models
            32_768
        }
    }

    /// Get maximum output token limit for a model
    pub fn max_output_tokens(model: &str) -> usize {
        if model.contains("2.5") || model.contains("3") {
            65_536 // 65K tokens for Gemini 2.5 and 3 models
        } else {
            8_192 // Conservative default
        }
    }
    fn apply_stream_delta(accumulator: &mut String, chunk: &str) -> Option<String> {
        if chunk.is_empty() {
            return None;
        }

        if chunk.starts_with(accumulator.as_str()) {
            let delta = &chunk[accumulator.len()..];
            if delta.is_empty() {
                return None;
            }
            accumulator.clear();
            accumulator.push_str(chunk);
            return Some(delta.to_string());
        }

        if accumulator.starts_with(chunk) {
            accumulator.clear();
            accumulator.push_str(chunk);
            return None;
        }

        accumulator.push_str(chunk);
        Some(chunk.to_string())
    }

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
                    if let Some(ref func) = tool_call.function {
                        call_map.insert(tool_call.id.clone(), func.name.clone());
                    }
                }
            }
        }

        let mut contents: Vec<Content> = Vec::new();
        for message in &request.messages {
            if message.role == MessageRole::System {
                continue;
            }

            let content_text = message.content.as_text();
            let mut parts: Vec<Part> = Vec::new();
            if message.role != MessageRole::Tool && !message.content.is_empty() {
                parts.push(Part::Text {
                    text: content_text.into_owned(),
                    thought_signature: None,
                });
            }

            if message.role == MessageRole::Assistant
                && let Some(tool_calls) = &message.tool_calls
            {
                for tool_call in tool_calls {
                    if let Some(ref func) = tool_call.function {
                        let parsed_args =
                            serde_json::from_str(&func.arguments).unwrap_or_else(|_| json!({}));
                        parts.push(Part::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: func.name.clone(),
                                args: parsed_args,
                                id: Some(tool_call.id.clone()),
                            },
                            // Preserve thought signature from the tool call
                            // This is critical for Gemini 3 Pro to maintain reasoning context
                            thought_signature: tool_call.thought_signature.clone(),
                        });
                    }
                }
            }

            if message.role == MessageRole::Tool {
                if let Some(tool_call_id) = &message.tool_call_id {
                    let func_name = call_map
                        .get(tool_call_id)
                        .cloned()
                        .unwrap_or_else(|| tool_call_id.clone());
                    let response_text = serde_json::from_str::<Value>(&message.content.as_text())
                        .map(|value| {
                            serde_json::to_string_pretty(&value)
                                .unwrap_or_else(|_| message.content.as_text().into_owned())
                        })
                        .unwrap_or_else(|_| message.content.as_text().into_owned());

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
                        thought_signature: None, // Function responses don't carry thought signatures
                    });
                } else if !message.content.is_empty() {
                    parts.push(Part::Text {
                        text: message.content.as_text().into_owned(),
                        thought_signature: None,
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
                .filter_map(|tool| {
                    let func = tool.function.as_ref()?;
                    Some(Tool {
                        function_declarations: vec![FunctionDeclaration {
                            name: func.name.clone(),
                            description: func.description.clone(),
                            parameters: sanitize_function_parameters(func.parameters.clone()),
                        }],
                    })
                })
                .collect()
        });

        let mut generation_config = Map::new();
        if let Some(max_tokens) = request.max_tokens {
            generation_config.insert("maxOutputTokens".to_string(), json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            // For Gemini 3 Pro, Google recommends keeping temperature at 1.0 default
            // to avoid potential looping or degraded performance on complex tasks
            if request.model.contains("gemini-3") && temp < 1.0 {
                // Use a more formal log rather than eprintln to avoid mix-up with error messages
                tracing::warn!(
                    "When using Gemini 3 Pro with temperature values below 1.0, be aware that this may cause looping or degraded performance on complex tasks. Consider using 1.0 or higher for optimal results."
                );
            }
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

        let reasoning_config = if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(&request.model) {
                // Map our internal reasoning effort levels to Gemini's thinking_level
                // Note: For Gemini 3 Pro, Google recommends specific thinking levels
                // Note: "None" reasoning level is only for OpenAI GPT-5.1 models, not for Gemini
                let thinking_level = match effort {
                    // For Gemini models, treat None as Low for basic reasoning
                    ReasoningEffortLevel::None => {
                        Some("low")
                    }
                    // For Gemini 3 Pro, use the new thinking_level parameter
                    // According to Google's documentation: low=fast responses, high=maximum reasoning
                    ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => {
                        Some("low")
                    }
                    ReasoningEffortLevel::Medium => {
                        // Note: According to Google's docs, medium is coming soon for Gemini 3,
                        // so we'll map to high for now to maintain functionality
                        Some("high") // Default to high for Gemini 3 Pro since medium not fully available
                    }
                    ReasoningEffortLevel::High => {
                        Some("high") // Maintain compatibility for older models
                    }
                };

                thinking_level.map(|level| {
                    json!({
                        "thinkingLevel": level
                    })
                })
            } else {
                None
            }
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
            reasoning_config,
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
                reasoning_details: None,
            });
        }

        let mut text_content = String::new();
        let mut tool_calls = Vec::new();

        for part in candidate.content.parts {
            match part {
                Part::Text {
                    text,
                    thought_signature,
                } => {
                    text_content.push_str(&text);
                    // Store thought signature for non-function-call text responses
                    // This is used for maintaining context in multi-turn conversations
                    if thought_signature.is_some() && !text_content.is_empty() {
                        // Store in last position to be retrieved later if needed
                        // For text parts, thought signatures are typically in the last part
                    }
                }
                Part::FunctionCall {
                    function_call,
                    thought_signature,
                } => {
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
                        function: Some(FunctionCall {
                            name: function_call.name,
                            arguments: serde_json::to_string(&function_call.args)
                                .unwrap_or_else(|_| "{}".to_string()),
                        }),
                        text: None,
                        // Preserve thought signature from Gemini response
                        // Critical for Gemini 3 Pro to maintain reasoning context across function calls
                        thought_signature,
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

        // Extract reasoning content if present in the text based on markup tags
        let (cleaned_content, extracted_reasoning) = if !text_content.is_empty() {
            let (reasoning_segments, cleaned) =
                crate::llm::providers::split_reasoning_from_text(&text_content);
            let final_reasoning = if reasoning_segments.is_empty() {
                None
            } else {
                let combined_reasoning = reasoning_segments.join("\n");
                if combined_reasoning.trim().is_empty() {
                    None
                } else {
                    Some(combined_reasoning)
                }
            };
            let final_content = cleaned.unwrap_or_else(|| text_content.clone());
            (
                if final_content.trim().is_empty() {
                    None
                } else {
                    Some(final_content)
                },
                final_reasoning,
            )
        } else {
            (None, None)
        };

        Ok(LLMResponse {
            content: cleaned_content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage: None,
            finish_reason,
            reasoning: extracted_reasoning,
            reasoning_details: None,
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
        Value::Object(map) => {
            // List of unsupported fields for Gemini API
            // Reference: https://ai.google.dev/gemini-api/docs/function-calling
            const UNSUPPORTED_FIELDS: &[&str] = &[
                "additionalProperties",
                "oneOf",
                "anyOf",
                "allOf",
                "exclusiveMaximum",
                "exclusiveMinimum",
                "minimum",
                "maximum",
                "$schema",
                "$id",
                "$ref",
                "definitions",
                "patternProperties",
                "dependencies",
                "const",
                "if",
                "then",
                "else",
                "not",
                "contentMediaType",
                "contentEncoding",
            ];

            // Process all properties recursively, removing unsupported fields
            let mut sanitized = Map::new();
            for (key, value) in map {
                // Skip unsupported fields at this level
                if UNSUPPORTED_FIELDS.contains(&key.as_str()) {
                    continue;
                }
                // Recursively sanitize nested values
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

                        messages.push(Message::base(role, MessageContent::from(content_text)));
                    }

                    // Convert tools if present
                    let tools = gemini_request.tools.as_ref().map(|gemini_tools| {
                        gemini_tools
                            .iter()
                            .flat_map(|tool| &tool.function_declarations)
                            .map(|decl| crate::llm::provider::ToolDefinition {
                                tool_type: "function".to_string(),
                                function: Some(crate::llm::provider::FunctionDefinition {
                                    name: decl.name.clone(),
                                    description: decl.description.clone(),
                                    parameters: decl.parameters.clone(),
                                }),
                                shell: None,
                                grammar: None,
                                strict: None,
                            })
                            .collect::<Vec<_>>()
                    });

                    let llm_request = LLMRequest {
                        messages,
                        system_prompt,
                        tools,
                        model: self.model.to_string(),
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
                        output_format: None,
                        tool_choice: None,
                        parallel_tool_calls: None,
                        parallel_tool_config: None,
                        reasoning_effort: None,
                        verbosity: None,
                    };

                    // Use the standard LLMProvider generate method
                    let response = LLMProvider::generate(self, llm_request).await?;

                    // If there are tool calls, include them in the response content as JSON
                    let content = if let Some(tool_calls) = &response.tool_calls {
                        if !tool_calls.is_empty() {
                            // Create a JSON structure that the agent can parse
                            let tool_call_json = json!({
                                "tool_calls": tool_calls.iter().filter_map(|tc| {
                                    tc.function.as_ref().map(|func| {
                                        json!({
                                            "function": {
                                                "name": func.name,
                                                "arguments": func.arguments
                                            }
                                        })
                                    })
                                }).collect::<Vec<_>>()
                            });
                            tool_call_json.to_string()
                        } else {
                            response.content.unwrap_or_default()
                        }
                    } else {
                        response.content.unwrap_or_default()
                    };

                    return Ok(llm_types::LLMResponse {
                        content,
                        model: self.model.to_string(),
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
                            content: MessageContent::Text(prompt.to_string()),
                            reasoning: None,
                            reasoning_details: None,
                            tool_calls: None,
                            tool_call_id: None,
                            origin_tool: None,
                        }],
                        system_prompt: None,
                        tools: None,
                        model: self.model.to_string(),
                        max_tokens: None,
                        temperature: None,
                        stream: false,
                        output_format: None,
                        tool_choice: None,
                        parallel_tool_calls: None,
                        parallel_tool_config: None,
                        reasoning_effort: None,
                        verbosity: None,
                    }
                }
            }
        } else {
            // Fallback: treat as regular prompt
            LLMRequest {
                messages: vec![Message {
                    role: MessageRole::User,
                    content: MessageContent::Text(prompt.to_string()),
                    reasoning: None,
                    reasoning_details: None,
                    tool_calls: None,
                    tool_call_id: None,
                    origin_tool: None,
                }],
                system_prompt: None,
                tools: None,
                model: self.model.to_string(),
                max_tokens: None,
                temperature: None,
                stream: false,
                output_format: None,
                tool_choice: None,
                parallel_tool_calls: None,
                parallel_tool_config: None,
                reasoning_effort: None,
                verbosity: None,
            }
        };

        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: self.model.to_string(),
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
            output_format: None,
            tool_choice: Some(ToolChoice::Specific(SpecificToolChoice {
                tool_type: "function".to_string(),
                function: SpecificFunctionChoice {
                    name: "list_files".to_string(),
                },
            })),
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
        };

        let gemini_request = provider
            .convert_to_gemini_request(&request)
            .expect("conversion should succeed");

        let system_instruction = gemini_request
            .system_instruction
            .expect("system instruction should be present");
        assert!(matches!(
            system_instruction.parts.as_slice(),
            [Part::Text {
                text,
                thought_signature: _
            }] if text == "System prompt"
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
                            thought_signature: None,
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
        assert_eq!(calls[0].function.as_ref().unwrap().name, "list_files");
        assert!(
            calls[0]
                .function
                .as_ref()
                .unwrap()
                .arguments
                .contains("path")
        );
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

    #[test]
    fn sanitize_function_parameters_removes_exclusive_min_max() {
        // Test case for the bug: exclusiveMaximum and exclusiveMinimum in nested properties
        let parameters = json!({
            "type": "object",
            "properties": {
                "max_length": {
                    "type": "integer",
                    "exclusiveMaximum": 1000000,
                    "exclusiveMinimum": 0,
                    "minimum": 1,
                    "maximum": 999999,
                    "description": "Maximum number of characters"
                }
            }
        });

        let sanitized = sanitize_function_parameters(parameters);
        let props = sanitized
            .get("properties")
            .and_then(|v| v.as_object())
            .and_then(|p| p.get("max_length"))
            .and_then(|v| v.as_object())
            .expect("max_length property should exist");

        // These unsupported fields should be removed
        assert!(
            !props.contains_key("exclusiveMaximum"),
            "exclusiveMaximum should be removed"
        );
        assert!(
            !props.contains_key("exclusiveMinimum"),
            "exclusiveMinimum should be removed"
        );
        assert!(!props.contains_key("minimum"), "minimum should be removed");
        assert!(!props.contains_key("maximum"), "maximum should be removed");

        // These supported fields should be preserved
        assert_eq!(props.get("type").and_then(|v| v.as_str()), Some("integer"));
        assert_eq!(
            props.get("description").and_then(|v| v.as_str()),
            Some("Maximum number of characters")
        );
    }

    #[test]
    fn apply_stream_delta_handles_replayed_chunks() {
        let mut acc = String::new();
        assert_eq!(
            GeminiProvider::apply_stream_delta(&mut acc, "Hello"),
            Some("Hello".to_string())
        );
        assert_eq!(
            GeminiProvider::apply_stream_delta(&mut acc, "Hello world"),
            Some(" world".to_string())
        );
        assert_eq!(
            GeminiProvider::apply_stream_delta(&mut acc, "Hello world"),
            None
        );
        assert_eq!(acc, "Hello world");
    }

    #[test]
    fn apply_stream_delta_handles_incremental_chunks() {
        let mut acc = String::new();
        assert_eq!(
            GeminiProvider::apply_stream_delta(&mut acc, "Hello"),
            Some("Hello".to_string())
        );
        assert_eq!(
            GeminiProvider::apply_stream_delta(&mut acc, " there"),
            Some(" there".to_string())
        );
        assert_eq!(acc, "Hello there");
    }

    #[test]
    fn apply_stream_delta_handles_rewrites() {
        let mut acc = String::new();
        assert_eq!(
            GeminiProvider::apply_stream_delta(&mut acc, "Hello world"),
            Some("Hello world".to_string())
        );
        assert_eq!(GeminiProvider::apply_stream_delta(&mut acc, "Hello"), None);
        assert_eq!(acc, "Hello");
    }

    #[test]
    fn convert_to_gemini_request_includes_reasoning_config() {
        use crate::config::constants::models;
        use crate::config::types::ReasoningEffortLevel;

        let provider = GeminiProvider::new("test-key".to_string());

        // Test High effort level for Gemini 3 Pro
        let request = LLMRequest {
            messages: vec![Message::user("test".to_string())],
            system_prompt: None,
            tools: None,
            model: models::google::GEMINI_3_PRO_PREVIEW.to_string(),
            max_tokens: None,
            temperature: None,
            stream: false,
            output_format: None,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: Some(ReasoningEffortLevel::High),
            verbosity: None,
        };

        let gemini_request = provider
            .convert_to_gemini_request(&request)
            .expect("conversion should succeed");

        // Check that reasoning_config is present and has the correct value for High effort
        let reasoning_config = gemini_request
            .reasoning_config
            .expect("reasoning_config should be present");
        let thinking_level = reasoning_config
            .get("thinkingLevel")
            .expect("thinkingLevel should be present");
        assert_eq!(thinking_level.as_str().unwrap(), "high");

        // Test Low effort level for Gemini 3 Pro
        let request_low = LLMRequest {
            messages: vec![Message::user("test".to_string())],
            system_prompt: None,
            tools: None,
            model: models::google::GEMINI_3_PRO_PREVIEW.to_string(),
            max_tokens: None,
            temperature: None,
            stream: false,
            output_format: None,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: Some(ReasoningEffortLevel::Low),
            verbosity: None,
        };

        let gemini_request_low = provider
            .convert_to_gemini_request(&request_low)
            .expect("conversion should succeed");

        // Check that reasoning_config is present and has "low" value for Low effort
        let reasoning_config_low = gemini_request_low
            .reasoning_config
            .expect("reasoning_config should be present for low effort");
        let thinking_level_low = reasoning_config_low
            .get("thinkingLevel")
            .expect("thinkingLevel should be present");
        assert_eq!(thinking_level_low.as_str().unwrap(), "low");

        // Test that None effort results in low reasoning_config for Gemini (none is treated as low)
        let request_none = LLMRequest {
            messages: vec![Message::user("test".to_string())],
            system_prompt: None,
            tools: None,
            model: models::google::GEMINI_3_PRO_PREVIEW.to_string(),
            max_tokens: None,
            temperature: None,
            stream: false,
            output_format: None,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: Some(ReasoningEffortLevel::None),
            verbosity: None,
        };

        let gemini_request_none = provider
            .convert_to_gemini_request(&request_none)
            .expect("conversion should succeed");

        // Check that reasoning_config is present with low level when effort is None (for Gemini)
        let reasoning_config_none = gemini_request_none
            .reasoning_config
            .expect("reasoning_config should be present for None effort in Gemini");
        let thinking_level_none = reasoning_config_none
            .get("thinkingLevel")
            .expect("thinkingLevel should be present");
        assert_eq!(thinking_level_none.as_str().unwrap(), "low");
    }

    #[test]
    fn thought_signature_preserved_in_function_call_response() {
        use crate::gemini::function_calling::FunctionCall as GeminiFunctionCall;
        use crate::gemini::models::{Candidate, Content, GenerateContentResponse, Part};

        let test_signature = "encrypted_signature_xyz123".to_string();

        let response = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".to_string(),
                    parts: vec![Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "get_weather".to_string(),
                            args: json!({"city": "London"}),
                            id: Some("call_123".to_string()),
                        },
                        thought_signature: Some(test_signature.clone()),
                    }],
                },
                finish_reason: Some("FUNCTION_CALL".to_string()),
            }],
            prompt_feedback: None,
            usage_metadata: None,
        };

        let llm_response = GeminiProvider::convert_from_gemini_response(response)
            .expect("conversion should succeed");

        let tool_calls = llm_response.tool_calls.expect("should have tool calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(
            tool_calls[0].thought_signature,
            Some(test_signature),
            "thought signature should be preserved"
        );
    }

    #[test]
    fn thought_signature_roundtrip_in_request() {
        let provider = GeminiProvider::new("test-key".to_string());
        let test_signature = "sig_abc_def_123".to_string();

        let request = LLMRequest {
            messages: vec![
                Message::user("What's the weather?".to_string()),
                Message {
                    role: MessageRole::Assistant,
                    content: MessageContent::Text(String::new()),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_456".to_string(),
                        call_type: "function".to_string(),
                        function: Some(FunctionCall {
                            name: "get_weather".to_string(),
                            arguments: r#"{"city":"Paris"}"#.to_string(),
                        }),
                        text: None,
                        thought_signature: Some(test_signature.clone()),
                    }]),
                    tool_call_id: None,
                },
            ],
            system_prompt: None,
            tools: None,
            model: "gemini-3-pro-preview".to_string(),
            max_tokens: None,
            temperature: None,
            stream: false,
            output_format: None,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
        };

        let gemini_request = provider
            .convert_to_gemini_request(&request)
            .expect("conversion should succeed");

        // Find the FunctionCall part with thought signature
        let assistant_content = &gemini_request.contents[1];
        let has_signature = assistant_content.parts.iter().any(|part| match part {
            Part::FunctionCall {
                thought_signature, ..
            } => thought_signature.as_ref() == Some(&test_signature),
            _ => false,
        });

        assert!(
            has_signature,
            "thought signature should be preserved in request"
        );
    }

    #[test]
    fn parallel_function_calls_single_signature() {
        use crate::gemini::function_calling::FunctionCall as GeminiFunctionCall;
        use crate::gemini::models::{Candidate, Content, GenerateContentResponse, Part};

        let test_signature = "parallel_sig_123".to_string();

        let response = GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".to_string(),
                    parts: vec![
                        Part::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: "get_weather".to_string(),
                                args: json!({"city": "Paris"}),
                                id: Some("call_1".to_string()),
                            },
                            thought_signature: Some(test_signature.clone()),
                        },
                        Part::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: "get_weather".to_string(),
                                args: json!({"city": "London"}),
                                id: Some("call_2".to_string()),
                            },
                            thought_signature: None, // Only first has signature
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

        let tool_calls = llm_response.tool_calls.expect("should have tool calls");
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(
            tool_calls[0].thought_signature,
            Some(test_signature),
            "first call should have signature"
        );
        assert_eq!(
            tool_calls[1].thought_signature, None,
            "second call should not have signature"
        );
    }

    #[test]
    fn gemini_provider_supports_reasoning_effort_for_gemini3() {
        use crate::config::constants::models;
        use crate::config::models::ModelId;
        use crate::config::models::Provider;

        // Test that the provider correctly identifies Gemini 3 Pro as supporting reasoning effort
        assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_3_PRO_PREVIEW));
        assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_2_5_PRO));
        assert!(Provider::Gemini.supports_reasoning_effort(models::google::GEMINI_2_5_FLASH));

        // Test model IDs as well
        assert!(ModelId::Gemini3ProPreview.supports_reasoning_effort());
        assert!(ModelId::Gemini25Pro.supports_reasoning_effort());
    }
}
