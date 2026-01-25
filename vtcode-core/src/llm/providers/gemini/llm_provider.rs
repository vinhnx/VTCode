use super::*;

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

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        // All Gemini 3 and Gemini 2.5 models support configurable thinking_level
        // Reference: https://ai.google.dev/gemini-api/docs/gemini-3
        // Gemini 3 Pro/Flash: supports thinking_level (low, high)
        // Gemini 3 Flash: additionally supports minimal, medium
        // Gemini 2.5: supports thinking_level for reasoning models
        models::google::REASONING_MODELS.contains(&model)
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        // Context caching supported on all Gemini 3 and most Gemini 2.5 models
        // Requires minimum 2048 cached tokens
        // Reference: https://ai.google.dev/gemini-api/docs/caching
        models::google::CACHING_MODELS.contains(&model)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        // Gemini 3 and Gemini 2.5 models have 1M input context window
        if model.contains("2.5")
            || model.contains("3")
            || model.contains("2.0")
            || model.contains("1.5-pro")
        {
            2_097_152 // 2M tokens for Gemini 1.5 Pro, 2.x and 3.x models
        } else {
            1_048_576 // 1M tokens for other current models
        }
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let gemini_request = self.convert_to_gemini_request(&request)?;

        let url = format!("{}/models/{}:generateContent", self.base_url, request.model);

        let response = self
            .http_client
            .post(&url)
            .header("x-goog-api-key", self.api_key.as_ref())
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
            "{}/models/{}:streamGenerateContent",
            self.base_url, request.model
        );

        let response = self
            .http_client
            .post(&url)
            .header("x-goog-api-key", self.api_key.as_ref())
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
                                response: Box::new(final_response),
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
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
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
                return Err(LLMError::InvalidRequest {
                    message: formatted_error,
                    metadata: None,
                });
            }
        }

        Ok(())
    }
}
