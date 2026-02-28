use super::*;
use crate::config::constants::models;
use crate::gemini::streaming::StreamingError;
use crate::llm::error_display;
use crate::llm::provider::LLMError;
use crate::llm::provider::{ContentPart, MessageContent};
use crate::prompts::system::default_system_prompt;

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
        if model.contains("gemini-3.1") {
            1_048_576 // 1M tokens for Gemini 3.1 models
        } else if model.contains("3") || model.contains("1.5-pro") {
            2_097_152 // 2M tokens for Gemini 1.5 Pro and 3.x models
        } else {
            1_048_576 // 1M tokens for other current models
        }
    }

    /// Get maximum output token limit for a model
    pub fn max_output_tokens(model: &str) -> usize {
        if model.contains("3") {
            65_536 // 65K tokens for Gemini 3 models
        } else {
            8_192 // Conservative default
        }
    }

    /// Check if model supports extended thinking levels (minimal, medium)
    /// Only Gemini 3 Flash supports these additional levels
    pub fn supports_extended_thinking(model: &str) -> bool {
        model.contains("gemini-3-flash")
    }

    /// Get supported thinking levels for a model
    /// Reference: https://ai.google.dev/gemini-api/docs/gemini-3
    pub fn supported_thinking_levels(model: &str) -> Vec<&'static str> {
        if model.contains("gemini-3-flash") {
            // Gemini 3 Flash supports all levels
            vec!["minimal", "low", "medium", "high"]
        } else if model.contains("gemini-3") {
            // Gemini 3 Pro supports low and high
            vec!["low", "high"]
        } else {
            // Unknown model, conservative default
            vec!["low", "high"]
        }
    }
    pub(super) fn apply_stream_delta(accumulator: &mut String, chunk: &str) -> Option<String> {
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

    pub(super) fn convert_to_gemini_request(
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

            let mut parts: Vec<Part> = Vec::new();
            if message.role != MessageRole::Tool {
                parts.extend(parts_from_message_content(&message.content));
            }

            if message.role == MessageRole::Assistant
                && let Some(tool_calls) = &message.tool_calls
            {
                let is_gemini3 = request.model.contains("gemini-3");
                for tool_call in tool_calls {
                    if let Some(ref func) = tool_call.function {
                        let parsed_args =
                            serde_json::from_str(&func.arguments).unwrap_or_else(|_| json!({}));

                        // Gemini 3 models require thought_signature on function call parts.
                        // If the streaming response didn't include it, use the validator skip
                        // token to prevent 400 errors. This is documented by Google as a
                        // fallback for cases where signatures are unavailable.
                        // See: https://ai.google.dev/gemini-api/docs/thought-signatures
                        let thought_signature =
                            if is_gemini3 && tool_call.thought_signature.is_none() {
                                tracing::trace!(
                                    function_name = %func.name,
                                    "Gemini 3: using skip_thought_signature_validator fallback"
                                );
                                Some("skip_thought_signature_validator".to_string())
                            } else {
                                tool_call.thought_signature.clone()
                            };

                        parts.push(Part::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: func.name.clone(),
                                args: parsed_args,
                                id: Some(tool_call.id.clone()),
                            },
                            thought_signature,
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
                            id: Some(tool_call_id.clone()),
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
            let mut seen = std::collections::HashSet::new();
            definitions
                .iter()
                .filter_map(|tool| {
                    let func = tool.function.as_ref()?;
                    if !seen.insert(func.name.clone()) {
                        return None;
                    }
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

        let mut generation_config = crate::gemini::models::request::GenerationConfig {
            max_output_tokens: request.max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
            top_k: request.top_k,
            presence_penalty: request.presence_penalty,
            frequency_penalty: request.frequency_penalty,
            stop_sequences: request.stop_sequences.clone(),
            ..Default::default()
        };

        // For Gemini 3 Pro, Google recommends keeping temperature at 1.0 default
        if let Some(temp) = request.temperature {
            if request.model.contains("gemini-3") && temp < 1.0 {
                tracing::warn!(
                    "When using Gemini 3 Pro with temperature values below 1.0, be aware that this may cause looping or degraded performance on complex tasks. Consider using 1.0 or higher for optimal results."
                );
            }
        }

        // Support for structured output (JSON mode)
        if let Some(format) = &request.output_format {
            generation_config.response_mime_type = Some("application/json".to_string());
            if format.is_object() {
                generation_config.response_schema = Some(format.clone());
            }
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

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(&request.model) {
                let is_gemini3_flash = request.model.contains("gemini-3-flash");
                let thinking_level = match effort {
                    ReasoningEffortLevel::None => Some("low"),
                    ReasoningEffortLevel::Minimal => {
                        if is_gemini3_flash {
                            Some("minimal")
                        } else {
                            Some("low")
                        }
                    }
                    ReasoningEffortLevel::Low => Some("low"),
                    ReasoningEffortLevel::Medium => {
                        if is_gemini3_flash {
                            Some("medium")
                        } else {
                            Some("high")
                        }
                    }
                    ReasoningEffortLevel::High => Some("high"),
                    ReasoningEffortLevel::XHigh => Some("high"),
                };

                if let Some(level) = thinking_level {
                    generation_config.thinking_config =
                        Some(crate::gemini::models::ThinkingConfig {
                            thinking_level: Some(level.to_string()),
                        });
                }
            }
        }

        Ok(GenerateContentRequest {
            contents,
            tools,
            tool_config,
            system_instruction: {
                let text = request
                    .system_prompt
                    .as_ref()
                    .map(|text| (**text).clone())
                    .unwrap_or_else(|| default_system_prompt().to_string());

                if self.prompt_cache_enabled
                    && matches!(
                        self.prompt_cache_settings.mode,
                        GeminiPromptCacheMode::Explicit
                    )
                {
                    if let Some(ttl) = self.prompt_cache_settings.explicit_ttl_seconds {
                        Some(SystemInstruction::with_ttl(text, ttl))
                    } else {
                        Some(SystemInstruction::new(text))
                    }
                } else if request.system_prompt.is_some() || self.prompt_cache_enabled {
                    Some(SystemInstruction::new(text))
                } else {
                    None
                }
            },
            generation_config: Some(generation_config),
        })
    }

    pub(super) fn convert_from_gemini_response(
        response: GenerateContentResponse,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        let mut candidates = response.candidates.into_iter();
        let candidate = candidates.next().ok_or_else(|| {
            let formatted_error =
                error_display::format_llm_error("Gemini", "No candidate in response");
            LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        if candidate.content.parts.is_empty() {
            return Ok(LLMResponse {
                content: Some(String::new()),
                tool_calls: None,
                model,
                usage: None,
                finish_reason: FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                tool_references: Vec::new(),
                request_id: None,
                organization_id: None,
            });
        }

        let mut text_content = String::new();
        let mut tool_calls = Vec::new();
        // Track thought signature from text parts to attach to subsequent function calls
        // This is needed because Gemini 3 sometimes attaches the signature to the reasoning text
        // but requires it to be present on the function call when replayed in history.
        let mut last_text_thought_signature: Option<String> = None;

        for part in candidate.content.parts {
            match part {
                Part::Text {
                    text,
                    thought_signature,
                } => {
                    text_content.push_str(&text);
                    if thought_signature.is_some() {
                        last_text_thought_signature = thought_signature;
                    }
                }
                Part::InlineData { .. } => {}
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

                    // Use the signature from the function call, or fall back to the one from preceding text
                    let effective_signature =
                        thought_signature.or(last_text_thought_signature.clone());

                    tool_calls.push(ToolCall {
                        id: call_id,
                        call_type: "function".to_string(),
                        function: Some(FunctionCall {
                            name: function_call.name,
                            arguments: serde_json::to_string(&function_call.args)
                                .unwrap_or_else(|_| "{}".to_string()),
                        }),
                        text: None,
                        thought_signature: effective_signature,
                    });
                }
                Part::FunctionResponse { .. } => {}
                Part::CacheControl { .. } => {}
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

        let (cleaned_content, extracted_reasoning) = if !text_content.is_empty() {
            let (reasoning_segments, cleaned) =
                crate::llm::providers::split_reasoning_from_text(&text_content);
            let final_reasoning = if reasoning_segments.is_empty() {
                None
            } else {
                let combined_reasoning: Vec<String> =
                    reasoning_segments.into_iter().map(|s| s.text).collect();
                let combined_reasoning = combined_reasoning.join("\n");
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
            model,
            usage: None,
            finish_reason,
            reasoning: extracted_reasoning,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }

    pub(super) fn convert_from_streaming_response(
        response: StreamingResponse,
        model: String,
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

        Self::convert_from_gemini_response(converted, model)
    }

    pub(super) fn map_streaming_error(error: StreamingError) -> LLMError {
        match error {
            StreamingError::NetworkError { message, .. } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!("Network error: {}", message),
                );
                LLMError::Network {
                    message: formatted,
                    metadata: None,
                }
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
                    LLMError::Authentication {
                        message: formatted,
                        metadata: None,
                    }
                } else if status_code == 429 {
                    LLMError::RateLimit { metadata: None }
                } else {
                    let formatted = error_display::format_llm_error(
                        "Gemini",
                        &format!("API error ({}): {}", status_code, message),
                    );
                    LLMError::Provider {
                        message: formatted,
                        metadata: None,
                    }
                }
            }
            StreamingError::ParseError { message, .. } => {
                let formatted =
                    error_display::format_llm_error("Gemini", &format!("Parse error: {}", message));
                LLMError::Provider {
                    message: formatted,
                    metadata: None,
                }
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
                LLMError::Network {
                    message: formatted,
                    metadata: None,
                }
            }
            StreamingError::ContentError { message } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!("Content error: {}", message),
                );
                LLMError::Provider {
                    message: formatted,
                    metadata: None,
                }
            }
            StreamingError::StreamingError { message, .. } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!("Streaming error: {}", message),
                );
                LLMError::Provider {
                    message: formatted,
                    metadata: None,
                }
            }
        }
    }
}

fn parts_from_message_content(content: &MessageContent) -> Vec<Part> {
    match content {
        MessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Part::Text {
                    text: text.clone(),
                    thought_signature: None,
                }]
            }
        }
        MessageContent::Parts(parts) => {
            let mut converted = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if !text.is_empty() {
                            converted.push(Part::Text {
                                text: text.clone(),
                                thought_signature: None,
                            });
                        }
                    }
                    ContentPart::Image {
                        data, mime_type, ..
                    } => {
                        converted.push(Part::InlineData {
                            inline_data: crate::gemini::models::InlineData {
                                mime_type: mime_type.clone(),
                                data: data.clone(),
                            },
                        });
                    }
                    ContentPart::File {
                        filename,
                        file_id,
                        file_url,
                        ..
                    } => {
                        let fallback = filename
                            .clone()
                            .or_else(|| file_id.clone())
                            .or_else(|| file_url.clone())
                            .unwrap_or_else(|| "attached file".to_string());
                        converted.push(Part::Text {
                            text: format!("[File input not directly supported: {}]", fallback),
                            thought_signature: None,
                        });
                    }
                }
            }
            converted
        }
    }
}
