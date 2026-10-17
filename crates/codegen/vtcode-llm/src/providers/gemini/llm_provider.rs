use super::helpers::InteractionStreamState;
use super::*;
use crate::providers::shared::{StreamAssemblyError, extract_data_payload, next_sse_event};

fn normalize_stream_event(event: LLMStreamEvent, interaction_reasoning: bool) -> Vec<NormalizedStreamEvent> {
    match event {
        LLMStreamEvent::Reasoning { delta } if interaction_reasoning => {
            vec![NormalizedStreamEvent::ReasoningDelta { delta, source: ReasoningSource::ProviderSummary }]
        }
        LLMStreamEvent::Completed { response } => normalize_completed_event(response),
        event => event.into_normalized(),
    }
}

fn normalize_completed_event(response: Box<LLMResponse>) -> Vec<NormalizedStreamEvent> {
    let mut events = Vec::new();
    if let Some(tool_calls) = response.tool_calls.as_ref() {
        for tool_call in tool_calls {
            events.push(NormalizedStreamEvent::ToolCallStart {
                call_id: tool_call.id.clone(),
                name: tool_call.tool_name().map(ToOwned::to_owned),
            });
            if let Some(arguments) = tool_call
                .raw_input()
                .filter(|arguments| !arguments.trim().is_empty() && arguments.trim() != "{}")
            {
                events.push(NormalizedStreamEvent::ToolCallDelta {
                    call_id: tool_call.id.clone(),
                    delta: arguments.to_string(),
                });
            }
        }
    }
    events.extend(LLMStreamEvent::Completed { response }.into_normalized());
    events
}

/// Shared transport for `generateContent` and `streamGenerateContent`.
impl GeminiProvider {
    async fn post_generate_content(
        &self,
        url: &str,
        body: &GenerateContentRequest,
    ) -> Result<reqwest::Response, LLMError> {
        self.http_client
            .post(url)
            .header("x-goog-api-key", self.api_key.as_ref())
            .json(body)
            .send()
            .await
            .map_err(|e| format_network_error("Gemini", &e))
    }

    /// Send a generate request and return only a successful response.
    ///
    /// A stale `cachedContent` name (expired or evicted) is retried once
    /// without the cache and with the full system instruction/tools resent.
    /// Both `generate` and `stream` share this path so recovery cannot drift
    /// between them. The dead slot is dropped before the retry so the next
    /// turn rebuilds it.
    async fn send_generate_request_with_cache_recovery(
        &self,
        url: &str,
        gemini_request: &GenerateContentRequest,
        request: &LLMRequest,
    ) -> Result<reqwest::Response, LLMError> {
        let response = self.post_generate_content(url, gemini_request).await?;
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let error_text = crate::providers::common::read_provider_error_body(response).await;
        if gemini_request.cached_content.is_none()
            || !explicit_cache::is_stale_cache_error(status.as_u16(), &error_text)
        {
            return Err(Self::handle_http_error(status, &error_text));
        }

        self.explicit_cache.clear();
        let full_request = self.convert_to_gemini_request(request)?;
        let retry = self.post_generate_content(url, &full_request).await?;
        if retry.status().is_success() {
            return Ok(retry);
        }

        let retry_status = retry.status();
        let retry_error_text = crate::providers::common::read_provider_error_body(retry).await;
        Err(Self::handle_http_error(retry_status, &retry_error_text))
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

    fn supports_non_streaming(&self, _model: &str) -> bool {
        // Pinned so the stream-timeout fallback cannot silently regress.
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        // Codex-inspired robustness: Setting model_supports_reasoning to false
        // does NOT disable it for known reasoning models.
        models::google::REASONING_MODELS.contains(&model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning)
                .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        // Same robustness logic for reasoning effort
        models::google::REASONING_MODELS.contains(&model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning_effort)
                .unwrap_or(false)
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        models::google::CACHING_MODELS.contains(&model)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        let fallback = if model.contains("gemini-4-argon") || model.contains("gemini-3.1") {
            // Gemini 4 Argon: unified 1M limit shared by input, thinking, and output
            1_048_576
        } else if model.contains("3") || model.contains("1.5-pro") {
            2_097_152
        } else {
            1_048_576
        };
        crate::provider::catalog_context_window("gemini", model, fallback)
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = request.model.clone();
        if self.should_use_interactions(&request) {
            let interaction_request = self.convert_to_interaction_request(&request)?;
            let url = format!("{}/interactions", self.base_url);
            let response = self
                .http_client
                .post(&url)
                .header("x-goog-api-key", self.api_key.as_ref())
                .json(&interaction_request)
                .send()
                .await
                .map_err(|e| format_network_error("Gemini", &e))?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = crate::providers::common::read_provider_error_body(response).await;
                return Err(Self::handle_http_error(status, &error_text));
            }

            let interaction_response: Interaction =
                response.json().await.map_err(|e| format_parse_error("Gemini", &e))?;

            return Self::convert_from_interaction_response(interaction_response, model);
        }

        let mut gemini_request = self.convert_to_gemini_request(&request)?;
        if let Some(cache_name) = self.ensure_explicit_cache(&request, &gemini_request).await? {
            gemini_request = self.apply_explicit_cache_to_request(gemini_request, &cache_name);
        }

        let url = format!("{}/models/{}:generateContent", self.base_url, request.model);

        let response = self
            .send_generate_request_with_cache_recovery(&url, &gemini_request, &request)
            .await?;

        let gemini_response: GenerateContentResponse =
            response.json().await.map_err(|e| format_parse_error("Gemini", &e))?;

        Self::convert_from_gemini_response(gemini_response, model)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        if self.should_use_interactions(&request) {
            let model = request.model.clone();
            let interaction_request = self.convert_to_interaction_request(&request)?;
            let url = format!("{}/interactions?alt=sse", self.base_url);
            let response = self
                .http_client
                .post(&url)
                .header("x-goog-api-key", self.api_key.as_ref())
                .json(&interaction_request)
                .send()
                .await
                .map_err(|e| format_network_error("Gemini", &e))?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = crate::providers::common::read_provider_error_body(response).await;
                return Err(Self::handle_http_error(status, &error_text));
            }

            let stream = {
                try_stream! {
                    let mut body_stream = response.bytes_stream();
                    let mut buf: Vec<u8> = Vec::new();
                    let mut offset = 0usize;
                    let mut decoder = crate::providers::shared::Utf8StreamDecoder::new();
                    let mut state = InteractionStreamState::default();

                    while let Some(chunk_result) = body_stream.next().await {
                        let chunk = chunk_result
                            .map_err(|err| format_network_error("Gemini", &err))?;

                        decoder.push_bytes(&chunk, &mut buf);

                        while let Some(event) = next_sse_event(&buf, &mut offset)
                            .map_err(|e| {
                                StreamAssemblyError::InvalidPayload(format!("non-utf-8 stream data: {e}"))
                                    .into_llm_error("Gemini")
                            })?
                        {

                            let Some(data_payload) = extract_data_payload(event) else {
                                continue;
                            };

                            let trimmed_payload = data_payload.trim();
                            if trimmed_payload.is_empty() || trimmed_payload == "[DONE]" {
                                continue;
                            }

                            let payload: Value = serde_json::from_str(trimmed_payload)
                                .map_err(|err| {
                                    StreamAssemblyError::InvalidPayload(err.to_string())
                                        .into_llm_error("Gemini")
                                })?;

                            for stream_event in Self::apply_interaction_stream_payload(&mut state, &payload)? {
                                yield stream_event;
                            }
                        }

                        // Drain the consumed prefix so `buf` stays bounded to
                        // the unprocessed tail rather than growing for the
                        // entire stream lifetime.
                        if offset > 0 {
                            buf.drain(..offset);
                            offset = 0;
                        }
                    }

                    if !state.completed {
                        let formatted_error = error_display::format_llm_error(
                            "Gemini",
                            "Interactions stream ended without an interaction.complete event",
                        );
                        Err(LLMError::Provider {
                            message: formatted_error,
                            metadata: None,
                        })?;
                    }

                    let response =
                        Self::finalize_interaction_stream_state(state, model)?;
                    yield LLMStreamEvent::Completed { response: Box::new(response) };
                }
            };
            return Ok(Box::pin(stream));
        }

        let model = request.model.clone();
        let mut gemini_request = self.convert_to_gemini_request(&request)?;
        if let Some(cache_name) = self.ensure_explicit_cache(&request, &gemini_request).await? {
            gemini_request = self.apply_explicit_cache_to_request(gemini_request, &cache_name);
        }

        let url = format!("{}/models/{}:streamGenerateContent", self.base_url, request.model);

        let response = self
            .send_generate_request_with_cache_recovery(&url, &gemini_request, &request)
            .await?;

        let (event_tx, event_rx) = mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let completion_sender = event_tx.clone();

        let streaming_timeout = self.timeouts.streaming_ceiling_seconds;

        let model_clone = model.clone();
        tokio::spawn(async move {
            let config = StreamingConfig::with_total_timeout(streaming_timeout);
            let mut processor = StreamingProcessor::with_config(config);
            let event_sender = completion_sender.clone();
            let mut aggregator = crate::providers::shared::StreamAggregator::new(model_clone.clone());

            let mut on_chunk = |chunk: &str| -> Result<(), StreamingError> {
                if chunk.is_empty() {
                    return Ok(());
                }

                if let Some(delta) = Self::apply_stream_delta(&mut aggregator.content, chunk) {
                    if delta.is_empty() {
                        return Ok(());
                    }

                    for event in aggregator.sanitizer.process_chunk(&delta) {
                        event_sender.send(Ok(event)).map_err(|_e| StreamingError::StreamingError {
                            message: "Streaming consumer dropped".to_string(),
                            partial_content: Some(chunk.to_string()),
                        })?;
                    }
                }
                Ok(())
            };

            let result = processor.process_stream(response, &mut on_chunk).await;
            match result {
                Ok(mut streaming_response) => {
                    if streaming_response.candidates.is_empty() && !aggregator.content.trim().is_empty() {
                        streaming_response.candidates.push(StreamingCandidate {
                            content: Content {
                                role: "model".to_string(),
                                parts: vec![Part::Text {
                                    text: aggregator.content.clone(),
                                    thought_signature: None,
                                }],
                            },
                            finish_reason: None,
                            index: Some(0),
                        });
                    }

                    match Self::convert_from_streaming_response(streaming_response, model_clone) {
                        Ok(mut final_response) => {
                            let aggregator_response = aggregator.finalize();
                            if final_response.reasoning.is_none() {
                                final_response.reasoning = aggregator_response.reasoning;
                            }
                            if final_response.content.is_none() {
                                final_response.content = aggregator_response.content;
                            }

                            let _ = completion_sender
                                .send(Ok(LLMStreamEvent::Completed { response: Box::new(final_response) }));
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

    async fn stream_normalized(&self, request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        let interaction_reasoning = self.should_use_interactions(&request);
        let mut legacy_stream = self.stream(request).await?;
        let stream = try_stream! {
            while let Some(event) = legacy_stream.next().await {
                for normalized in normalize_stream_event(event?, interaction_reasoning) {
                    yield normalized;
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        models::google::SUPPORTED_MODELS.iter().map(|s| s.to_string()).collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if GeminiProvider::uses_latest_gemini_api(&request.model) {
            if request.temperature.is_some() || request.top_p.is_some() || request.top_k.is_some() {
                tracing::warn!(
                    model = %request.model,
                    temperature = ?request.temperature,
                    top_p = ?request.top_p,
                    top_k = ?request.top_k,
                    "Sampling parameters (temperature, top_p, top_k) are deprecated for this Gemini model and will be ignored by the API"
                );
            }
        }

        if request.previous_response_id.is_some() && request.response_store == Some(false) {
            let formatted_error = error_display::format_llm_error(
                "Gemini",
                "Interactions with previous_interaction_id cannot set store=false",
            );
            return Err(LLMError::InvalidRequest { message: formatted_error, metadata: None });
        }

        if !models::google::SUPPORTED_MODELS.iter().any(|m| *m == request.model) {
            let formatted_error =
                error_display::format_llm_error("Gemini", &format!("Unsupported model: {}", request.model));
            return Err(LLMError::InvalidRequest { message: formatted_error, metadata: None });
        }

        if let Some(max_tokens) = request.max_tokens {
            let model = request.model.as_str();
            let max_output_tokens = if model.contains("3") || model.contains("4") {
                65536
            } else {
                8192
            };

            if max_tokens > max_output_tokens {
                let formatted_error = error_display::format_llm_error(
                    "Gemini",
                    &format!(
                        "Requested max_tokens ({max_tokens}) exceeds model limit ({max_output_tokens}) for {model}"
                    ),
                );
                return Err(LLMError::InvalidRequest { message: formatted_error, metadata: None });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{LLMStreamEvent, NormalizedStreamEvent, ReasoningSource, normalize_stream_event};
    use crate::provider::{LLMResponse, ToolCall};

    #[test]
    fn interaction_reasoning_is_marked_as_public_summary() {
        let events = normalize_stream_event(LLMStreamEvent::Reasoning { delta: "summary".to_string() }, true);

        assert!(matches!(
            events.as_slice(),
            [NormalizedStreamEvent::ReasoningDelta { delta, source }]
                if delta == "summary" && *source == ReasoningSource::ProviderSummary
        ));
    }

    #[test]
    fn standard_reasoning_remains_unclassified() {
        let events = normalize_stream_event(LLMStreamEvent::Reasoning { delta: "trace".to_string() }, false);

        assert!(matches!(
            events.as_slice(),
            [NormalizedStreamEvent::ReasoningDelta { delta, source }]
                if delta == "trace" && *source == ReasoningSource::Unknown
        ));
    }

    #[test]
    fn completed_tool_calls_become_structured_events() {
        let events = normalize_stream_event(
            LLMStreamEvent::Completed {
                response: Box::new(LLMResponse {
                    tool_calls: Some(vec![ToolCall::function(
                        "call_1".to_string(),
                        "search_workspace".to_string(),
                        "{\"query\":\"vtcode\"}".to_string(),
                    )]),
                    ..Default::default()
                }),
            },
            false,
        );

        assert!(matches!(
            events.as_slice(),
            [
                NormalizedStreamEvent::ToolCallStart { call_id, name },
                NormalizedStreamEvent::ToolCallDelta { call_id: delta_call_id, delta },
                NormalizedStreamEvent::Done { .. }
            ] if call_id == "call_1"
                && delta_call_id == "call_1"
                && name.as_deref() == Some("search_workspace")
                && delta == "{\"query\":\"vtcode\"}"
        ));
    }
}
