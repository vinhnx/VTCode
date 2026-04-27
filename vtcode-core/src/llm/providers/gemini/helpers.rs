use super::wire::{GenerationConfig, InlineData, StreamingError, ThinkingConfig};
use super::*;
use crate::config::constants::models;
use crate::llm::error_display;
use crate::llm::provider::LLMError;
use crate::llm::provider::{ContentPart, MessageContent, ToolDefinition};
use crate::llm::providers::common::{
    collect_history_system_directives, merge_system_prompt_with_history_directives,
};
use crate::prompts::system::default_system_prompt;
use serde_json::Map;
use std::collections::BTreeMap;

const GEMINI_PRESERVED_PARTS_PREFIX: &str = "__vtcode_gemini_parts__:";

struct GeminiToolSpec {
    generate_tools: Option<Vec<Tool>>,
    interaction_tools: Option<Vec<InteractionTool>>,
    uses_server_side_tools: bool,
    has_function_tools: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct InteractionStreamOutputBuilder {
    pub output_type: String,
    pub text: String,
    pub summary: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<Value>,
    pub signature: Option<String>,
}

impl InteractionStreamOutputBuilder {
    fn into_output(self) -> InteractionOutput {
        InteractionOutput {
            output_type: self.output_type,
            text: (!self.text.is_empty()).then_some(self.text),
            id: self.id,
            name: self.name,
            arguments: self.arguments,
            signature: self.signature,
            function_call: None,
            summary: (!self.summary.is_empty()).then_some(self.summary),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct InteractionStreamState {
    pub interaction_id: Option<String>,
    pub status: Option<String>,
    pub outputs: BTreeMap<usize, InteractionStreamOutputBuilder>,
    pub usage: Option<wire::interactions::InteractionUsage>,
    pub completed: bool,
}

impl GeminiProvider {
    const HISTORY_DIRECTIVES_SECTION_HEADER: &str = "[History Directives]";

    pub(super) fn is_gemini_3_pro_model(model: &str) -> bool {
        model.contains("gemini-3") && model.contains("pro") && !model.contains("flash")
    }

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
    /// Reference: <https://ai.google.dev/gemini-api/docs/gemini-3>
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
        let history_system_directives = collect_history_system_directives(request);
        for message in &request.messages {
            if message.role == MessageRole::System {
                continue;
            }

            let mut parts: Vec<Part> = preserved_gemini_parts_from_message(message)
                .unwrap_or_else(|| build_message_parts(message, request.model.as_str()));

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

        let tool_spec = collect_gemini_tool_spec(request.tools.as_deref());
        let tools = tool_spec.generate_tools;
        let uses_server_side_tools = tool_spec.uses_server_side_tools;

        let generation_config = build_generation_config(self, request);

        // For Gemini 3 Pro, Google recommends keeping temperature at 1.0 default
        if let Some(temp) = request.temperature {
            if Self::is_gemini_3_pro_model(&request.model) && temp < 1.0 {
                tracing::warn!(
                    "When using Gemini 3 Pro with temperature values below 1.0, be aware that this may cause looping or degraded performance on complex tasks. Consider using 1.0 or higher for optimal results."
                );
            }
        }

        let has_tools = request
            .tools
            .as_ref()
            .map(|defs| !defs.is_empty())
            .unwrap_or(false);
        let has_function_tools = tool_spec.has_function_tools;
        let tool_config = if has_tools || request.tool_choice.is_some() {
            let function_calling_config = if has_function_tools {
                Some(match request.tool_choice.as_ref() {
                    Some(ToolChoice::None) => FunctionCallingConfig::none(),
                    Some(ToolChoice::Any) => FunctionCallingConfig::any(),
                    Some(ToolChoice::Specific(spec)) => {
                        let mut config = if uses_server_side_tools {
                            FunctionCallingConfig::validated()
                        } else {
                            FunctionCallingConfig::any()
                        };
                        if spec.tool_type == "function" {
                            config.allowed_function_names = Some(vec![spec.function.name.clone()]);
                        }
                        config
                    }
                    _ => {
                        if uses_server_side_tools {
                            FunctionCallingConfig::validated()
                        } else {
                            FunctionCallingConfig::auto()
                        }
                    }
                })
            } else {
                None
            };

            Some(ToolConfig {
                function_calling_config,
                include_server_side_tool_invocations: uses_server_side_tools.then_some(true),
            })
        } else {
            None
        };

        Ok(GenerateContentRequest {
            contents,
            tools,
            tool_config,
            system_instruction: {
                let base_system_prompt = request
                    .system_prompt
                    .as_ref()
                    .map(|prompt| prompt.as_str())
                    .or_else(|| self.prompt_cache_enabled.then_some(default_system_prompt()));
                let merged_system_prompt = merge_system_prompt_with_history_directives(
                    base_system_prompt,
                    &history_system_directives,
                    Self::HISTORY_DIRECTIVES_SECTION_HEADER,
                );

                if self.prompt_cache_enabled
                    && matches!(
                        self.prompt_cache_settings.mode,
                        GeminiPromptCacheMode::Explicit
                    )
                {
                    if let Some(ttl) = self.prompt_cache_settings.explicit_ttl_seconds {
                        merged_system_prompt.map(|text| SystemInstruction::with_ttl(text, ttl))
                    } else {
                        merged_system_prompt.map(SystemInstruction::new)
                    }
                } else if request.system_prompt.is_some()
                    || self.prompt_cache_enabled
                    || !history_system_directives.is_empty()
                {
                    merged_system_prompt.map(SystemInstruction::new)
                } else {
                    None
                }
            },
            generation_config: Some(generation_config.into()),
        })
    }

    pub(super) fn should_use_interactions(&self, request: &LLMRequest) -> bool {
        if request.previous_response_id.is_some() {
            return true;
        }

        request.model.contains("gemini-3")
            && collect_gemini_tool_spec(request.tools.as_deref()).uses_server_side_tools
    }

    pub(super) fn convert_to_interaction_request(
        &self,
        request: &LLMRequest,
    ) -> Result<InteractionRequest, LLMError> {
        let history_system_directives = collect_history_system_directives(request);
        let base_system_prompt = request
            .system_prompt
            .as_ref()
            .map(|prompt| prompt.as_str())
            .or_else(|| self.prompt_cache_enabled.then_some(default_system_prompt()));
        let merged_system_prompt = merge_system_prompt_with_history_directives(
            base_system_prompt,
            &history_system_directives,
            Self::HISTORY_DIRECTIVES_SECTION_HEADER,
        );

        let tool_spec = collect_gemini_tool_spec(request.tools.as_deref());
        let generation_config = build_generation_config(self, request);
        let interaction_input = build_interaction_input(request)?;

        Ok(InteractionRequest {
            model: request.model.clone(),
            input: interaction_input,
            tools: tool_spec.interaction_tools,
            system_instruction: merged_system_prompt,
            response_format: request.output_format.clone(),
            response_mime_type: request
                .output_format
                .as_ref()
                .map(|_| "application/json".to_string()),
            stream: request.stream.then_some(true),
            store: request.response_store,
            generation_config: Some(generation_config.into()),
            tool_choice: build_interaction_tool_choice(
                request.tool_choice.as_ref(),
                tool_spec.has_function_tools,
                tool_spec.uses_server_side_tools,
            ),
            previous_interaction_id: request.previous_response_id.clone(),
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

        let raw_parts = candidate.content.parts.clone();
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
                            namespace: None,
                            name: function_call.name,
                            arguments: serde_json::to_string(&function_call.args)
                                .unwrap_or_else(|_| "{}".to_string()),
                        }),
                        text: None,
                        thought_signature: effective_signature,
                    });
                }
                Part::FunctionResponse { .. } => {}
                Part::ToolCall { .. } => {}
                Part::ToolResponse { .. } => {}
                Part::ExecutableCode { .. } => {}
                Part::CodeExecutionResult { .. } => {}
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
            reasoning_details: preserved_gemini_parts_detail(&raw_parts),
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }

    pub(super) fn convert_from_interaction_response(
        response: Interaction,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        let mut text_content = String::new();
        let mut tool_calls = Vec::new();
        let mut thought_summaries = Vec::new();
        let mut thought_details = Vec::new();

        for output in response.outputs {
            match output.output_type.as_str() {
                "text" => {
                    if let Some(text) = output.text {
                        text_content.push_str(&text);
                    }
                }
                "thought" => {
                    let summary = output.summary.or(output.text).unwrap_or_default();
                    if !summary.trim().is_empty() {
                        thought_summaries.push(summary.clone());
                    }
                    thought_details.push(
                        json!({
                            "type": "thought",
                            "signature": output.signature,
                            "summary": summary,
                        })
                        .to_string(),
                    );
                }
                "function_call" => {
                    let (name, arguments, id, signature) =
                        if let Some(function_call) = output.function_call {
                            (
                                function_call.name,
                                function_call.arguments,
                                function_call.id.or(output.id),
                                function_call.signature.or(output.signature),
                            )
                        } else {
                            (
                                output.name.unwrap_or_default(),
                                output.arguments.unwrap_or(Value::Null),
                                output.id,
                                output.signature,
                            )
                        };

                    let call_id = id.unwrap_or_else(|| {
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
                            namespace: None,
                            name,
                            arguments: serde_json::to_string(&arguments)
                                .unwrap_or_else(|_| "{}".to_string()),
                        }),
                        text: None,
                        thought_signature: signature,
                    });
                }
                _ => {}
            }
        }

        let finish_reason = if tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolCalls
        };
        let (reasoning_segments, cleaned) =
            crate::llm::providers::split_reasoning_from_text(&text_content);
        let extracted_reasoning = if reasoning_segments.is_empty() {
            None
        } else {
            Some(
                reasoning_segments
                    .into_iter()
                    .map(|segment| segment.text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .filter(|value| !value.trim().is_empty())
        };
        let content = cleaned
            .or_else(|| (!text_content.trim().is_empty()).then_some(text_content))
            .filter(|value| !value.trim().is_empty());
        let reasoning = if thought_summaries.is_empty() {
            extracted_reasoning
        } else {
            Some(thought_summaries.join("\n"))
        };
        let reasoning_details = if thought_details.is_empty() {
            None
        } else {
            Some(thought_details)
        };

        Ok(LLMResponse {
            content,
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            model,
            usage: response.usage.map(|usage| vtcode_commons::llm::Usage {
                prompt_tokens: usage.total_input_tokens.unwrap_or_default(),
                completion_tokens: usage.total_output_tokens.unwrap_or_default(),
                total_tokens: usage.total_tokens.unwrap_or_default(),
                cached_prompt_tokens: usage.total_cached_tokens,
                cache_creation_tokens: None,
                cache_read_tokens: usage.total_cached_tokens,
            }),
            finish_reason,
            reasoning,
            reasoning_details,
            tool_references: Vec::new(),
            request_id: Some(response.id),
            organization_id: None,
        })
    }

    pub(super) fn apply_interaction_stream_payload(
        state: &mut InteractionStreamState,
        payload: &Value,
    ) -> Result<Vec<LLMStreamEvent>, LLMError> {
        let mut events = Vec::new();
        let Some(event_type) = payload.get("event_type").and_then(Value::as_str) else {
            return Ok(events);
        };

        match event_type {
            "interaction.start" | "interaction.status_update" | "interaction.complete" => {
                let interaction = interaction_object(payload);
                if let Some(id) = interaction.get("id").and_then(Value::as_str) {
                    state.interaction_id = Some(id.to_string());
                }
                if let Some(status) = interaction.get("status").and_then(Value::as_str) {
                    state.status = Some(status.to_string());
                }
                if let Some(usage) = interaction.get("usage")
                    && let Ok(usage) = serde_json::from_value(usage.clone())
                {
                    state.usage = Some(usage);
                }
                if event_type == "interaction.complete" {
                    state.completed = true;
                }
            }
            "content.start" => {
                let index = payload
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or_default() as usize;
                let builder = state.outputs.entry(index).or_default();
                if let Some(output_type) = payload
                    .get("content")
                    .and_then(Value::as_object)
                    .and_then(|content| content.get("type"))
                    .and_then(Value::as_str)
                {
                    builder.output_type = output_type.to_string();
                }
            }
            "content.delta" => {
                let index = payload
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or_default() as usize;
                let Some(delta) = payload.get("delta").and_then(Value::as_object) else {
                    return Ok(events);
                };
                let builder = state.outputs.entry(index).or_default();
                apply_interaction_delta(builder, delta, &mut events);
            }
            "content.stop" => {}
            "error" => {
                let error_message = payload
                    .get("error")
                    .and_then(Value::as_object)
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown Gemini interactions streaming error");
                let formatted = error_display::format_llm_error("Gemini", error_message);
                return Err(LLMError::Provider {
                    message: formatted,
                    metadata: None,
                });
            }
            _ => {}
        }

        Ok(events)
    }

    pub(super) fn finalize_interaction_stream_state(
        state: InteractionStreamState,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        let interaction = Interaction {
            id: state
                .interaction_id
                .unwrap_or_else(|| "interaction_stream".to_string()),
            model: model.clone(),
            status: state.status,
            outputs: state
                .outputs
                .into_values()
                .map(InteractionStreamOutputBuilder::into_output)
                .collect(),
            usage: state.usage,
        };

        Self::convert_from_interaction_response(interaction, model)
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
                            inline_data: InlineData {
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

fn build_interaction_content(content: &MessageContent) -> Vec<InteractionContent> {
    match content {
        MessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![InteractionContent::Text { text: text.clone() }]
            }
        }
        MessageContent::Parts(parts) => {
            let mut converted = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if !text.is_empty() {
                            converted.push(InteractionContent::Text { text: text.clone() });
                        }
                    }
                    ContentPart::Image {
                        data, mime_type, ..
                    } => converted.push(InteractionContent::Image {
                        data: data.clone(),
                        mime_type: mime_type.clone(),
                    }),
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
                        converted.push(InteractionContent::Text {
                            text: format!("[File input not directly supported: {fallback}]"),
                        });
                    }
                }
            }
            converted
        }
    }
}

fn build_message_parts(message: &Message, model: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    if message.role != MessageRole::Tool {
        parts.extend(parts_from_message_content(&message.content));
    }

    if message.role == MessageRole::Assistant
        && let Some(tool_calls) = &message.tool_calls
    {
        let is_gemini3 = model.contains("gemini-3");
        for tool_call in tool_calls {
            if let Some(ref func) = tool_call.function {
                let parsed_args = tool_call.parsed_arguments().unwrap_or_else(|_| json!({}));

                let thought_signature = if is_gemini3 && tool_call.thought_signature.is_none() {
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

    parts
}

fn preserved_gemini_parts_from_message(message: &Message) -> Option<Vec<Part>> {
    let details = message.reasoning_details.as_ref()?;
    for detail in details {
        let Some(text) = detail.as_str() else {
            continue;
        };
        let Some(payload) = text.strip_prefix(GEMINI_PRESERVED_PARTS_PREFIX) else {
            continue;
        };
        if let Ok(parts) = serde_json::from_str::<Vec<Part>>(payload) {
            return Some(parts);
        }
    }
    None
}

fn preserved_gemini_parts_detail(parts: &[Part]) -> Option<Vec<String>> {
    if !parts_require_roundtrip_history(parts) {
        return None;
    }

    serde_json::to_string(parts)
        .ok()
        .map(|serialized| vec![format!("{GEMINI_PRESERVED_PARTS_PREFIX}{serialized}")])
}

fn parts_require_roundtrip_history(parts: &[Part]) -> bool {
    parts.iter().any(|part| {
        part.thought_signature().is_some()
            || matches!(
                part,
                Part::ToolCall { .. }
                    | Part::ToolResponse { .. }
                    | Part::ExecutableCode { .. }
                    | Part::CodeExecutionResult { .. }
                    | Part::FunctionResponse { .. }
                    | Part::InlineData { .. }
            )
    })
}

fn gemini_built_in_tool(tool: &ToolDefinition) -> Option<Tool> {
    match tool.tool_type.as_str() {
        "web_search" | "google_search" => Some(Tool {
            google_search: Some(tool.web_search.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "google_maps" => Some(Tool {
            google_maps: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "url_context" => Some(Tool {
            url_context: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "file_search" => Some(Tool {
            file_search: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        "code_execution" => Some(Tool {
            code_execution: Some(tool.hosted_tool_config.clone().unwrap_or_else(|| json!({}))),
            ..Tool::default()
        }),
        other if other.starts_with("code_execution_") => Some(Tool {
            code_execution: Some(json!({})),
            ..Tool::default()
        }),
        _ => None,
    }
}

fn gemini_interaction_built_in_tool(tool: &ToolDefinition) -> Option<InteractionTool> {
    let (tool_type, config) = match tool.tool_type.as_str() {
        "web_search" | "google_search" => ("google_search", tool.web_search.as_ref()),
        "google_maps" => ("google_maps", tool.hosted_tool_config.as_ref()),
        "url_context" => ("url_context", tool.hosted_tool_config.as_ref()),
        "file_search" => ("file_search", tool.hosted_tool_config.as_ref()),
        "code_execution" => ("code_execution", tool.hosted_tool_config.as_ref()),
        other if other.starts_with("code_execution_") => ("code_execution", None),
        _ => return None,
    };

    Some(InteractionTool::built_in(tool_type, config))
}

fn collect_gemini_tool_spec(definitions: Option<&Vec<ToolDefinition>>) -> GeminiToolSpec {
    let Some(definitions) = definitions else {
        return GeminiToolSpec {
            generate_tools: None,
            interaction_tools: None,
            uses_server_side_tools: false,
            has_function_tools: false,
        };
    };

    let mut generate_tools = Vec::new();
    let mut interaction_tools = Vec::new();
    let mut function_declarations = Vec::new();
    let mut seen = hashbrown::HashSet::new();
    let mut uses_server_side_tools = false;
    let mut has_function_tools = false;

    for tool in definitions {
        if let Some(built_in_tool) = gemini_built_in_tool(tool) {
            uses_server_side_tools = true;
            generate_tools.push(built_in_tool);
        }
        if let Some(interaction_tool) = gemini_interaction_built_in_tool(tool) {
            interaction_tools.push(interaction_tool);
        }

        let Some(func) = tool.function.as_ref() else {
            continue;
        };
        has_function_tools = true;
        if !seen.insert(func.name.clone()) {
            continue;
        }

        let parameters = sanitize_function_parameters(func.parameters.clone());
        function_declarations.push(FunctionDeclaration {
            name: func.name.clone(),
            description: func.description.clone(),
            parameters: parameters.clone(),
        });
        interaction_tools.push(InteractionTool::function(
            func.name.clone(),
            func.description.clone(),
            parameters,
        ));
    }

    if !function_declarations.is_empty() {
        generate_tools.push(Tool {
            function_declarations: Some(function_declarations),
            ..Tool::default()
        });
    }

    GeminiToolSpec {
        generate_tools: (!generate_tools.is_empty()).then_some(generate_tools),
        interaction_tools: (!interaction_tools.is_empty()).then_some(interaction_tools),
        uses_server_side_tools,
        has_function_tools,
    }
}

fn build_generation_config(provider: &GeminiProvider, request: &LLMRequest) -> GenerationConfig {
    let mut generation_config = GenerationConfig {
        max_output_tokens: request.max_tokens,
        temperature: request.temperature,
        top_p: request.top_p,
        top_k: request.top_k,
        presence_penalty: request.presence_penalty,
        frequency_penalty: request.frequency_penalty,
        stop_sequences: request.stop_sequences.clone(),
        ..Default::default()
    };

    if let Some(format) = &request.output_format {
        generation_config.response_mime_type = Some("application/json".to_string());
        if format.is_object() {
            generation_config.response_schema = Some(format.clone());
        }
    }

    if let Some(effort) = request.reasoning_effort
        && provider.supports_reasoning_effort(&request.model)
    {
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
            ReasoningEffortLevel::High
            | ReasoningEffortLevel::XHigh
            | ReasoningEffortLevel::Max => Some("high"),
        };

        if let Some(level) = thinking_level {
            generation_config.thinking_config = Some(ThinkingConfig {
                thinking_level: Some(level.to_string()),
            });
        }
    }

    generation_config
}

fn build_interaction_tool_choice(
    tool_choice: Option<&ToolChoice>,
    has_function_tools: bool,
    uses_server_side_tools: bool,
) -> Option<InteractionToolChoice> {
    if !has_function_tools {
        return None;
    }

    let mut choice = match tool_choice {
        Some(ToolChoice::None) => InteractionToolChoice::new("none"),
        Some(ToolChoice::Any) => InteractionToolChoice::new("any"),
        Some(ToolChoice::Specific(spec)) => {
            let mut choice = InteractionToolChoice::new("validated");
            if spec.tool_type == "function" {
                choice.tools = Some(vec![spec.function.name.clone()]);
            }
            choice
        }
        _ => {
            if uses_server_side_tools {
                InteractionToolChoice::new("validated")
            } else {
                InteractionToolChoice::new("auto")
            }
        }
    };

    if choice.tools.as_ref().is_some_and(|tools| tools.is_empty()) {
        choice.tools = None;
    }

    Some(choice)
}

fn build_interaction_input(request: &LLMRequest) -> Result<InteractionInput, LLMError> {
    let relevant_messages = if request.previous_response_id.is_some() {
        interaction_delta_messages(&request.messages)
    } else {
        request.messages.clone()
    };
    let turns = build_interaction_turns(&relevant_messages, &request.messages)?;

    if request.previous_response_id.is_none() {
        if let [turn] = turns.as_slice()
            && turn.role == "user"
        {
            return Ok(match &turn.content {
                InteractionTurnContent::Text(text) => InteractionInput::Text(text.clone()),
                InteractionTurnContent::Content(content) => {
                    InteractionInput::Content(content.clone())
                }
            });
        }
        return Ok(InteractionInput::Turns(turns));
    }

    if let [turn] = turns.as_slice()
        && turn.role == "user"
    {
        return Ok(match &turn.content {
            InteractionTurnContent::Text(text) => InteractionInput::Text(text.clone()),
            InteractionTurnContent::Content(content) => InteractionInput::Content(content.clone()),
        });
    }

    Ok(InteractionInput::Turns(turns))
}

fn interaction_delta_messages(messages: &[Message]) -> Vec<Message> {
    let start = messages
        .iter()
        .rposition(|message| message.role == MessageRole::Assistant)
        .map_or(0, |index| index.saturating_add(1));
    let delta = messages[start..].to_vec();
    if delta.is_empty() {
        messages.to_vec()
    } else {
        delta
    }
}

fn build_interaction_turns(
    messages: &[Message],
    full_messages: &[Message],
) -> Result<Vec<InteractionTurn>, LLMError> {
    let mut call_map: HashMap<String, String> = HashMap::new();
    for message in full_messages {
        if message.role == MessageRole::Assistant
            && let Some(tool_calls) = &message.tool_calls
        {
            for tool_call in tool_calls {
                if let Some(func) = &tool_call.function {
                    call_map.insert(tool_call.id.clone(), func.name.clone());
                }
            }
        }
    }

    let mut turns = Vec::new();
    for message in messages {
        if message.role == MessageRole::System {
            continue;
        }

        let mut content = if message.role == MessageRole::Tool {
            Vec::new()
        } else {
            build_interaction_content(&message.content)
        };
        if message.role == MessageRole::Assistant
            && let Some(tool_calls) = &message.tool_calls
        {
            for tool_call in tool_calls {
                if let Some(func) = &tool_call.function {
                    content.push(InteractionContent::FunctionCall {
                        id: tool_call.id.clone(),
                        name: func.name.clone(),
                        arguments: tool_call.parsed_arguments().unwrap_or(Value::Null),
                        signature: tool_call.thought_signature.clone(),
                    });
                }
            }
        }
        if message.role == MessageRole::Tool {
            let tool_call_id =
                message
                    .tool_call_id
                    .clone()
                    .ok_or_else(|| LLMError::InvalidRequest {
                        message: "Gemini interactions require tool_call_id for tool messages"
                            .to_string(),
                        metadata: None,
                    })?;
            content.push(InteractionContent::FunctionResult {
                call_id: tool_call_id.clone(),
                name: call_map.get(&tool_call_id).cloned(),
                result: interaction_result_from_message_content(&message.content),
                is_error: None,
                signature: None,
            });
        }
        if content.is_empty() {
            continue;
        }

        let role = if message.role == MessageRole::Assistant {
            "model"
        } else {
            "user"
        };
        let content = match content.as_slice() {
            [InteractionContent::Text { text }] => InteractionTurnContent::Text(text.clone()),
            _ => InteractionTurnContent::Content(content),
        };
        turns.push(InteractionTurn {
            role: role.to_string(),
            content,
        });
    }

    Ok(turns)
}

fn interaction_result_from_message_content(content: &MessageContent) -> InteractionResult {
    match content {
        MessageContent::Text(text) => interaction_result_from_text(text),
        MessageContent::Parts(_) => {
            let parts = build_interaction_content(content);
            if let [InteractionContent::Text { text }] = parts.as_slice() {
                interaction_result_from_text(text)
            } else {
                InteractionResult::Content(parts)
            }
        }
    }
}

fn interaction_result_from_text(text: &str) -> InteractionResult {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return InteractionResult::String(String::new());
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(content) = interaction_result_content_array(&value) {
            return InteractionResult::Content(content);
        }
        if value.is_object() {
            return InteractionResult::Json(value);
        }
    }

    InteractionResult::String(text.to_string())
}

fn interaction_result_content_array(value: &Value) -> Option<Vec<InteractionContent>> {
    let items = value.as_array()?;
    let mut content = Vec::with_capacity(items.len());
    for item in items {
        let item_type = item.get("type")?.as_str()?;
        match item_type {
            "text" => content.push(InteractionContent::Text {
                text: item.get("text")?.as_str()?.to_string(),
            }),
            "image" => {
                let mime_type = item.get("mime_type")?.as_str()?.to_string();
                let data = item.get("data")?.as_str()?.to_string();
                content.push(InteractionContent::Image { data, mime_type });
            }
            _ => return None,
        }
    }

    Some(content)
}

fn interaction_object(payload: &Value) -> &Map<String, Value> {
    payload
        .get("interaction")
        .and_then(Value::as_object)
        .or_else(|| payload.as_object())
        .expect("stream payload should be an object")
}

fn apply_interaction_delta(
    builder: &mut InteractionStreamOutputBuilder,
    delta: &Map<String, Value>,
    events: &mut Vec<LLMStreamEvent>,
) {
    let delta_type = delta
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match delta_type {
        "text" => {
            builder.output_type = "text".to_string();
            if let Some(text) = delta.get("text").and_then(Value::as_str) {
                builder.text.push_str(text);
                events.push(LLMStreamEvent::Token {
                    delta: text.to_string(),
                });
            }
        }
        "thought" => {
            builder.output_type = "thought".to_string();
            if let Some(text) = delta
                .get("thought")
                .and_then(Value::as_str)
                .or_else(|| delta.get("text").and_then(Value::as_str))
            {
                builder.summary.push_str(text);
                events.push(LLMStreamEvent::Reasoning {
                    delta: text.to_string(),
                });
            }
        }
        "thought_summary" => {
            builder.output_type = "thought".to_string();
            if let Some(text) = delta
                .get("content")
                .and_then(Value::as_object)
                .and_then(|content| content.get("text"))
                .and_then(Value::as_str)
                .or_else(|| delta.get("text").and_then(Value::as_str))
            {
                builder.summary.push_str(text);
                events.push(LLMStreamEvent::Reasoning {
                    delta: text.to_string(),
                });
            }
        }
        "thought_signature" => {
            builder.output_type = "thought".to_string();
            if let Some(signature) = delta.get("signature").and_then(Value::as_str) {
                builder.signature = Some(signature.to_string());
            }
        }
        "function_call" => {
            builder.output_type = "function_call".to_string();
            if let Some(id) = delta.get("id").and_then(Value::as_str) {
                builder.id = Some(id.to_string());
            }
            if let Some(name) = delta.get("name").and_then(Value::as_str) {
                builder.name = Some(name.to_string());
            }
            if let Some(arguments) = delta.get("arguments") {
                builder.arguments = Some(arguments.clone());
            }
            if let Some(signature) = delta.get("signature").and_then(Value::as_str) {
                builder.signature = Some(signature.to_string());
            }
        }
        _ => {}
    }
}
