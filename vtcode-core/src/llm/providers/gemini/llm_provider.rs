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
        if model.contains("gemini-3.1") {
            1_048_576
        } else if model.contains("3") || model.contains("1.5-pro") {
            2_097_152
        } else {
            1_048_576
        }
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = request.model.clone();
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

        Self::convert_from_gemini_response(gemini_response, model)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let model = request.model.clone();
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

        let model_clone = model.clone();
        tokio::spawn(async move {
            let config = StreamingConfig::with_total_timeout(streaming_timeout);
            let mut processor = StreamingProcessor::with_config(config);
            let event_sender = completion_sender.clone();
            let mut aggregator =
                crate::llm::providers::shared::StreamAggregator::new(model_clone.clone());

            #[allow(clippy::collapsible_if)]
            let mut on_chunk = |chunk: &str| -> Result<(), StreamingError> {
                if chunk.is_empty() {
                    return Ok(());
                }

                if let Some(delta) = Self::apply_stream_delta(&mut aggregator.content, chunk) {
                    if delta.is_empty() {
                        return Ok(());
                    }

                    for event in aggregator.sanitizer.process_chunk(&delta) {
                        event_sender.send(Ok(event)).map_err(|_| {
                            StreamingError::StreamingError {
                                message: "Streaming consumer dropped".to_string(),
                                partial_content: Some(chunk.to_string()),
                            }
                        })?;
                    }
                }
                Ok(())
            };

            let result = processor.process_stream(response, &mut on_chunk).await;
            match result {
                Ok(mut streaming_response) => {
                    if streaming_response.candidates.is_empty()
                        && !aggregator.content.trim().is_empty()
                    {
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

        if let Some(max_tokens) = request.max_tokens {
            let model = request.model.as_str();
            let max_output_tokens = if model.contains("3") { 65536 } else { 8192 };

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
