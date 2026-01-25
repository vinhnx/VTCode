use super::super::stream_decoder::{
    OpenRouterStreamTelemetry, finalize_stream_response, parse_stream_payload,
};
use super::OpenRouterProvider;
use crate::config::constants::models;
use crate::config::models::ModelId;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, Usage,
};
use crate::llm::providers::{
    ReasoningBuffer, TagStreamSanitizer,
    shared::{
        StreamAssemblyError, StreamFragment, ToolCallBuilder, extract_data_payload,
        find_sse_boundary,
    },
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::Value;
use std::str::FromStr;

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
