//! Provider capabilities and native generation/streaming entrypoints.

use super::OpenResponsesProvider;
use crate::provider::{
    LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, Message,
    ResponsesCompactionOptions,
};
use crate::providers::error_handling::{format_network_error, format_parse_error};
use crate::providers::shared::{
    ResponsesNormalizedStreamOptions, Utf8StreamDecoder, create_responses_normalized_stream,
};
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};

/// Borrowed fields used by the native Responses SSE hot path.
///
/// Responses events carry a discriminator and a small set of fields consumed
/// by this provider. Deserializing into `Value` first allocates the complete
/// event tree, including fields that are ignored by the streaming loop.
#[derive(Debug, Deserialize)]
struct NativeStreamEventWire<'a> {
    #[serde(rename = "type")]
    event_type: Option<&'a str>,
    #[serde(borrow)]
    delta: Option<&'a str>,
    #[serde(borrow)]
    item_id: Option<&'a str>,
    item: Option<Value>,
}

#[async_trait]
impl LLMProvider for OpenResponsesProvider {
    fn name(&self) -> &str {
        "openresponses"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        // Pinned so the stream-timeout fallback cannot silently regress.
        true
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning)
            .unwrap_or(true) // Open Responses usually implies reasoning support
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(true)
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        self.supports_compaction_endpoint()
    }

    // OpenResponses exposes the same standalone `/responses/compact` endpoint as
    // the native OpenAI provider, so it opts into the NativeStandalone dispatch
    // (`compact_history_with_options`). Without this, the unified dispatch would
    // route it to NativeInline (Anthropic `compact_20260112` via `generate`),
    // which OpenResponses cannot serve, silently downgrading auto/recovery
    // compaction to the local summarization fallback.
    fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
        self.supports_compaction_endpoint()
    }

    async fn compact_history(&self, model: &str, history: &[Message]) -> Result<Vec<Message>, LLMError> {
        if !self.supports_compaction_endpoint() {
            return Err(LLMError::Provider {
                message: "OpenResponses compact endpoint is not supported for this configured base URL".to_string(),
                metadata: None,
            });
        }

        self.compact_history_request(model, history).await
    }

    // The compact endpoint accepts the common instruction and routing fields;
    // output-shaping options are intentionally omitted because this endpoint
    // does not expose them in its request schema.
    async fn compact_history_with_options(
        &self,
        model: &str,
        history: &[Message],
        options: &ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        if !self.supports_compaction_endpoint() {
            return Err(LLMError::Provider {
                message: "OpenResponses compact endpoint is not supported for this configured base URL".to_string(),
                metadata: None,
            });
        }

        self.compact_history_request_with_options(model, history, options).await
    }

    fn supported_models(&self) -> Vec<String> {
        use vtcode_config::constants::models::openresponses::SUPPORTED_MODELS;
        SUPPORTED_MODELS.iter().map(|s| s.to_string()).collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.model.is_empty() {
            return Err(LLMError::Provider {
                message: "Model is required for OpenResponses provider".to_string(),
                metadata: None,
            });
        }

        let supported = self.supported_models();
        if !supported.contains(&request.model) {
            return Err(LLMError::Provider {
                message: format!(
                    "Model '{}' is not supported by OpenResponses provider. Supported models: {}",
                    request.model,
                    supported.join(", ")
                ),
                metadata: None,
            });
        }

        Ok(())
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        // Try native Open Responses endpoint first
        let payload = self.build_native_payload(&request, false)?;
        let response = self.send_native_request(&payload).await?;

        // If native endpoint fails with 404, fallback to chat/completions
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return self.generate_fallback(request).await;
        }

        let json: Value = response.json().await.map_err(|e| format_parse_error("OpenResponses", &e))?;

        Self::parse_native_response_payload(json, model)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        let payload = self.build_native_payload(&request, true)?;
        let response = self.send_native_request(&payload).await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return self.stream_fallback(request).await;
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            let mut offset = 0usize;
            let mut decoder = Utf8StreamDecoder::new();
            let mut aggregator = crate::providers::shared::StreamAggregator::new(model);

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|e| format_network_error("OpenResponses", &e))?;
                decoder.push_bytes(&chunk, &mut buf);

                while let Some(event) =
                    crate::providers::shared::next_sse_event(&buf, &mut offset).expect("valid utf-8 stream data")
                {

                    if let Some(data_payload) = crate::providers::shared::extract_data_payload(event) {
                        let trimmed = data_payload.trim();
                        if trimmed.is_empty() || trimmed == "[DONE]" {
                            continue;
                        }

                        if let Ok(event) = serde_json::from_str::<NativeStreamEventWire<'_>>(trimmed) {
                            let event_type = event.event_type.unwrap_or("");

                            match event_type {
                                "response.output_text.delta" => {
                                    if let Some(delta) = event.delta {
                                        // Use aggregator's sanitizer to extract reasoning tags from content
                                        for ev in aggregator.handle_content(delta) {
                                            yield ev;
                                        }
                                    }
                                }
                                "response.function_call_arguments.delta" => {
                                    if let Some(delta) = event.delta {
                                        let tc_json = json!([{
                                            "index": 0,
                                            "id": event.item_id,
                                            "function": { "arguments": delta }
                                        }]);
                                        if let Some(tool_calls) = tc_json.as_array() {
                                            aggregator.handle_tool_calls(tool_calls);
                                        }
                                    }
                                }
                                "response.reasoning.delta" => {
                                    // Legacy/simple reasoning event
                                    if let Some(delta) = event.delta {
                                        yield LLMStreamEvent::Reasoning { delta: delta.to_string() };
                                    }
                                }
                                "response.reasoning_content.delta" => {
                                    // Raw reasoning traces (preferred)
                                    if let Some(delta) = event.delta {
                                        yield LLMStreamEvent::Reasoning { delta: delta.to_string() };
                                    }
                                }
                                "response.reasoning_summary_text.delta" => {
                                    // Summary reasoning (fallback when raw not available)
                                    if let Some(delta) = event.delta {
                                        yield LLMStreamEvent::Reasoning { delta: delta.to_string() };
                                    }
                                }
                                // The added event may contain an incomplete
                                // opaque item. Only the done snapshot is safe
                                // to replay on the next request.
                                "response.output_item.done" => {
                                    if let Some(item) = event.item.as_ref()
                                        && item.get("type").and_then(Value::as_str) == Some("compaction")
                                    {
                                        aggregator.append_reasoning_detail(item);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }

                // Keep `buf` bounded to the unprocessed tail rather than
                // growing for the entire stream.
                crate::providers::shared::drain_consumed_sse(&mut buf, &mut offset);
            }

            yield LLMStreamEvent::Completed { response: Box::new(aggregator.finalize()) };
        };

        Ok(Box::pin(stream))
    }

    async fn stream_normalized(&self, mut request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        let payload = self.build_native_payload(&request, true)?;
        let response = self.send_native_request(&payload).await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            let mut legacy_stream = self.stream_fallback(request).await?;
            let stream = try_stream! {
                while let Some(event) = legacy_stream.next().await {
                    for normalized in event?.into_normalized() {
                        yield normalized;
                    }
                }
            };
            return Ok(Box::pin(stream));
        }

        let emit_reasoning = self.supports_reasoning(&model);
        Ok(create_responses_normalized_stream(
            response,
            ResponsesNormalizedStreamOptions {
                provider_name: "OpenResponses",
                model: model.clone(),
                emit_reasoning,
                include_cached_prompt_metrics: false,
            },
            move |value| Self::parse_native_response_payload(value, model.clone()),
        ))
    }
}
