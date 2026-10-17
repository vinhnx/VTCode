//! Response conversion and interaction stream assembly.

use super::request::GeminiToolSpec;
use super::*;
use crate::provider::{ContentPart, MessageContent, ToolDefinition};
use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
struct InteractionStreamOutputBuilder {
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
pub(crate) struct InteractionStreamState {
    pub interaction_id: Option<String>,
    pub status: Option<String>,
    outputs: BTreeMap<usize, InteractionStreamOutputBuilder>,
    pub usage: Option<wire::interactions::InteractionUsage>,
    pub completed: bool,
}

impl GeminiProvider {
    pub(crate) fn apply_stream_delta(accumulator: &mut String, chunk: &str) -> Option<String> {
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

    pub(crate) fn convert_from_gemini_response(
        response: GenerateContentResponse,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        let mut candidates = response.candidates.into_iter();
        let candidate = candidates.next().ok_or_else(|| {
            let formatted_error = error_display::format_llm_error("Gemini", "No candidate in response");
            LLMError::Provider { message: formatted_error, metadata: None }
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
                compaction: None,
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
                Part::Text { text, thought_signature } => {
                    text_content.push_str(&text);
                    if thought_signature.is_some() {
                        last_text_thought_signature = thought_signature;
                    }
                }
                Part::InlineData { .. } => {}
                Part::FunctionCall { function_call, thought_signature } => {
                    let call_id = function_call
                        .id
                        .clone()
                        .unwrap_or_else(crate::providers::shared::generate_tool_call_id);

                    // Use the signature from the function call, or fall back to the one from preceding text
                    let effective_signature = thought_signature.or(last_text_thought_signature.clone());

                    tool_calls.push(ToolCall {
                        id: call_id,
                        call_type: "function".to_string(),
                        function: Some(FunctionCall {
                            namespace: None,
                            name: function_call.name,
                            arguments: serde_json::to_string(&function_call.args).unwrap_or_else(|_| "{}".to_string()),
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
            let (reasoning_segments, cleaned) = crate::providers::split_reasoning_from_text(&text_content);
            let final_reasoning = if reasoning_segments.is_empty() {
                None
            } else {
                let combined_reasoning: Vec<String> = reasoning_segments.into_iter().map(|s| s.text).collect();
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
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            model,
            usage: None,
            finish_reason,
            reasoning: extracted_reasoning,
            reasoning_details: preserved_gemini_parts_detail(&raw_parts),
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
            compaction: None,
        })
    }

    pub(crate) fn convert_from_interaction_response(
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
                    if let Some(summary) = output.summary.as_deref().filter(|summary| !summary.trim().is_empty()) {
                        thought_summaries.push(summary.to_string());
                    }
                    thought_details.push(
                        json!({
                            "type": "thought",
                            "signature": output.signature,
                            "summary": output.summary,
                            "text": output.text,
                        })
                        .to_string(),
                    );
                }
                "function_call" => {
                    let (name, arguments, id, signature) = if let Some(function_call) = output.function_call {
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

                    let call_id = id.unwrap_or_else(crate::providers::shared::generate_tool_call_id);

                    tool_calls.push(ToolCall {
                        id: call_id,
                        call_type: "function".to_string(),
                        function: Some(FunctionCall {
                            namespace: None,
                            name,
                            arguments: serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".to_string()),
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
        let (reasoning_segments, cleaned) = crate::providers::split_reasoning_from_text(&text_content);
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
                iterations: None,
            }),
            finish_reason,
            reasoning,
            reasoning_details,
            tool_references: Vec::new(),
            request_id: Some(response.id),
            organization_id: None,
            compaction: None,
        })
    }

    pub(crate) fn apply_interaction_stream_payload(
        state: &mut InteractionStreamState,
        payload: &Value,
    ) -> Result<Vec<LLMStreamEvent>, LLMError> {
        let mut events = Vec::new();
        let Some(event_type) = payload.get("event_type").and_then(Value::as_str) else {
            return Ok(events);
        };

        match event_type {
            "interaction.start" | "interaction.status_update" | "interaction.complete" => {
                let interaction = interaction_object(payload)?;
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
                let index = payload.get("index").and_then(Value::as_u64).unwrap_or_default() as usize;
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
                let index = payload.get("index").and_then(Value::as_u64).unwrap_or_default() as usize;
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
                return Err(LLMError::Provider { message: formatted, metadata: None });
            }
            _ => {}
        }

        Ok(events)
    }

    pub(crate) fn finalize_interaction_stream_state(
        state: InteractionStreamState,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        let interaction = Interaction {
            id: state.interaction_id.unwrap_or_else(|| "interaction_stream".to_string()),
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

    pub(crate) fn convert_from_streaming_response(
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

    #[cold]
    pub(crate) fn map_streaming_error(error: StreamingError) -> LLMError {
        match error {
            StreamingError::NetworkError { message, .. } => {
                let formatted = error_display::format_llm_error("Gemini", &format!("Network error: {message}"));
                LLMError::Network { message: formatted, metadata: None }
            }
            StreamingError::ApiError { status_code, message, .. } => {
                if status_code == 401 || status_code == 403 {
                    let formatted =
                        error_display::format_llm_error("Gemini", &format!("HTTP {status_code}: {message}"));
                    LLMError::Authentication { message: formatted, metadata: None }
                } else if status_code == 429 {
                    LLMError::RateLimit { metadata: None }
                } else {
                    let formatted =
                        error_display::format_llm_error("Gemini", &format!("API error ({status_code}): {message}"));
                    LLMError::Provider { message: formatted, metadata: None }
                }
            }
            StreamingError::ParseError { message, .. } => {
                let formatted = error_display::format_llm_error("Gemini", &format!("Parse error: {message}"));
                LLMError::Provider { message: formatted, metadata: None }
            }
            StreamingError::TimeoutError { operation, duration } => {
                let formatted = error_display::format_llm_error(
                    "Gemini",
                    &format!("Streaming timeout during {operation} after {duration:?}"),
                );
                LLMError::Network { message: formatted, metadata: None }
            }
            StreamingError::ContentError { message } => {
                let formatted = error_display::format_llm_error("Gemini", &format!("Content error: {message}"));
                LLMError::Provider { message: formatted, metadata: None }
            }
            StreamingError::StreamingError { message, .. } => {
                let formatted = error_display::format_llm_error("Gemini", &format!("Streaming error: {message}"));
                LLMError::Provider { message: formatted, metadata: None }
            }
        }
    }
}

fn interaction_object(payload: &Value) -> Result<&Map<String, Value>, LLMError> {
    payload
        .get("interaction")
        .and_then(Value::as_object)
        .or_else(|| payload.as_object())
        .ok_or_else(|| LLMError::Provider {
            message: "Gemini interaction stream payload is not an object".into(),
            metadata: None,
        })
}

fn apply_interaction_delta(
    builder: &mut InteractionStreamOutputBuilder,
    delta: &Map<String, Value>,
    events: &mut Vec<LLMStreamEvent>,
) {
    let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or_default();

    match delta_type {
        "text" => {
            builder.output_type = "text".to_string();
            if let Some(text) = delta.get("text").and_then(Value::as_str) {
                builder.text.push_str(text);
                events.push(LLMStreamEvent::Token { delta: text.to_string() });
            }
        }
        "thought" => {
            builder.output_type = "thought".to_string();
            if let Some(text) = delta
                .get("thought")
                .and_then(Value::as_str)
                .or_else(|| delta.get("text").and_then(Value::as_str))
            {
                // `thought` is provider-private reasoning. Keep it in the
                // completed response for continuation, but do not turn it
                // into a public reasoning event.
                builder.text.push_str(text);
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
                events.push(LLMStreamEvent::Reasoning { delta: text.to_string() });
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

/// Format a slice of tool definitions for the Gemini wire shape.
///
/// This is the entry point used by
/// [`crate::providers::tool_format::GeminiFormatter`] and lets the runloop project
/// tool definitions without going through a full `LLMRequest`. Returns the JSON
/// representation of the `generateContent.tools` array (a bare array when tools
/// are present, `None` when the slice is empty).
///
/// Callers that need access to `interaction_tools` or the
/// `uses_server_side_tools` flag should use `collect_gemini_tool_spec` directly
/// via the build helpers in [`crate::providers::gemini`].
pub fn serialize_gemini_tools(tools: &[ToolDefinition]) -> Result<Option<Value>, LLMError> {
    if tools.is_empty() {
        return Ok(None);
    }

    let spec = collect_gemini_tool_spec(Some(tools))?;
    let Some(generate_tools) = spec.generate_tools else {
        return Ok(None);
    };
    serde_json::to_value(generate_tools)
        .map(Some)
        .map_err(|err| LLMError::Provider {
            message: format!("failed to serialize Gemini tools: {err}"),
            metadata: None,
        })
}

#[cfg(test)]
mod fabricated_id_tests {
    use super::*;
    use std::collections::HashSet;

    fn id_less_generate_content_response(function_name: &str) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: "model".to_string(),
                    parts: vec![Part::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: function_name.to_string(),
                            args: json!({}),
                            id: None,
                        },
                        thought_signature: None,
                    }],
                },
                finish_reason: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
        }
    }

    #[test]
    fn convert_from_gemini_response_fabricates_unique_ids_when_missing() {
        let first = GeminiProvider::convert_from_gemini_response(
            id_less_generate_content_response("foo"),
            "gemini-test".to_string(),
        )
        .expect("conversion should succeed");
        let second = GeminiProvider::convert_from_gemini_response(
            id_less_generate_content_response("bar"),
            "gemini-test".to_string(),
        )
        .expect("conversion should succeed");

        let first_id = first.tool_calls.expect("tool call expected")[0].id.clone();
        let second_id = second.tool_calls.expect("tool call expected")[0].id.clone();

        assert_ne!(first_id, second_id, "fabricated ids must differ across responses");
        assert!(first_id.starts_with("call_"));
        assert!(second_id.starts_with("call_"));
    }

    fn id_less_interaction(function_name: &str) -> Interaction {
        Interaction {
            id: "interaction-1".to_string(),
            model: "gemini-test".to_string(),
            status: None,
            outputs: vec![InteractionOutput {
                output_type: "function_call".to_string(),
                text: None,
                summary: None,
                id: None,
                name: Some(function_name.to_string()),
                arguments: Some(json!({})),
                signature: None,
                function_call: None,
            }],
            usage: None,
        }
    }

    #[test]
    fn convert_from_interaction_response_fabricates_unique_ids_when_missing() {
        let first =
            GeminiProvider::convert_from_interaction_response(id_less_interaction("foo"), "gemini-test".to_string())
                .expect("conversion should succeed");
        let second =
            GeminiProvider::convert_from_interaction_response(id_less_interaction("bar"), "gemini-test".to_string())
                .expect("conversion should succeed");

        let first_id = first.tool_calls.expect("tool call expected")[0].id.clone();
        let second_id = second.tool_calls.expect("tool call expected")[0].id.clone();

        let mut unique = HashSet::new();
        unique.insert(first_id.clone());
        unique.insert(second_id.clone());
        assert_eq!(unique.len(), 2, "fabricated ids must differ across responses");
        assert!(first_id.starts_with("call_"));
        assert!(second_id.starts_with("call_"));
    }

    #[test]
    fn gemini_formatter_rejects_reserved_built_in_extension_keys() {
        let tool = ToolDefinition::google_maps(json!({"type": "unexpected"}));
        let error = serialize_gemini_tools(&[tool]).expect_err("reserved extension keys must be rejected");
        assert!(error.to_string().contains("collides with a reserved wire field"));
    }
}
