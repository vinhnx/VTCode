use super::AgentRunner;
use super::constants::STREAMING_COOLDOWN_SECS;
use super::types::ProviderResponseSummary;
use crate::core::agent::events::ExecEventRecorder;
use crate::llm::provider::{LLMRequest, LLMStreamEvent};
use crate::utils::colors::style;
use anyhow::{Context, Result, anyhow};
use futures::StreamExt;
use tracing::warn;

impl AgentRunner {
    pub(super) async fn collect_provider_response(
        &mut self,
        request: &LLMRequest,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        warnings: &mut Vec<String>,
        turn_index: usize,
    ) -> Result<ProviderResponseSummary> {
        // Pre-flight validation: fail fast before API call
        self.validate_llm_request(request)
            .context("LLM request validation failed")?;

        let supports_streaming = self.provider_client.supports_streaming();
        let streaming_deadline = self
            .config()
            .timeouts
            .ceiling_duration(self.config().timeouts.streaming_ceiling_seconds);
        let generation_deadline = self
            .config()
            .timeouts
            .ceiling_duration(self.config().timeouts.default_ceiling_seconds);
        let mut streaming_disabled = false;
        if supports_streaming {
            if let Some(last_failure) = *self.streaming_last_failure.lock()
                && last_failure.elapsed().as_secs() >= STREAMING_COOLDOWN_SECS
            {
                *self.streaming_failures.lock() = 0;
                self.streaming_last_failure.lock().take();
            }
            streaming_disabled =
                *self.streaming_failures.lock() >= super::constants::MAX_STREAMING_FAILURES;
        }
        let mut agent_message_streamed = false;
        let mut reasoning_recorded = false;
        // Optimize: Pre-allocate with capacity to reduce reallocations during streaming
        // Typical response: 500-2000 chars, reasoning: 200-1000 chars
        let mut aggregated_text = String::with_capacity(2048);
        let mut aggregated_reasoning = String::with_capacity(1024);
        let mut streaming_response: Option<Box<crate::llm::provider::LLMResponse>> = None;

        if supports_streaming && !streaming_disabled {
            let stream_result = if let Some(limit) = streaming_deadline {
                tokio::time::timeout(limit, self.provider_client.stream(request.clone())).await
            } else {
                Ok(self.provider_client.stream(request.clone()).await)
            };

            match stream_result {
                Ok(Ok(mut stream)) => {
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(LLMStreamEvent::Token { delta }) => {
                                if delta.is_empty() {
                                    continue;
                                }
                                aggregated_text.push_str(&delta);
                                if event_recorder.agent_message_stream_update(&aggregated_text) {
                                    agent_message_streamed = true;
                                }
                            }
                            Ok(LLMStreamEvent::Reasoning { delta }) => {
                                aggregated_reasoning.push_str(&delta);
                                if event_recorder.reasoning_stream_update(&aggregated_reasoning) {
                                    reasoning_recorded = true;
                                }
                            }
                            Ok(LLMStreamEvent::Completed { response }) => {
                                streaming_response = Some(response);
                                break;
                            }
                            Err(err) => {
                                let mut failures = self.streaming_failures.lock();
                                *failures = failures.saturating_add(1);
                                *self.streaming_last_failure.lock() = Some(std::time::Instant::now());
                                self.failure_tracker.lock().record_failure();
                                if !self.quiet {
                                    println!(
                                        "{} {} Streaming error: {}",
                                        agent_prefix,
                                        style("(WARN)").red().bold(),
                                        err
                                    );
                                }
                                let warning = format!("Streaming response interrupted: {}", err);
                                event_recorder.warning(&warning);
                                warnings.push(warning);
                                if agent_message_streamed {
                                    event_recorder.agent_message_stream_complete();
                                }
                                break;
                            }
                        }
                    }
                }
                Ok(Err(err)) => {
                    let mut failures = self.streaming_failures.lock();
                    *failures = failures.saturating_add(1);
                    *self.streaming_last_failure.lock() = Some(std::time::Instant::now());
                    self.failure_tracker.lock().record_failure();
                    if !self.quiet {
                        println!(
                            "{} {} Streaming fallback: {}",
                            agent_prefix,
                            style("(WARN)").red().bold(),
                            err
                        );
                    }
                    let warning = format!("Streaming request failed: {}", err);
                    event_recorder.warning(&warning);
                    warnings.push(warning);
                }
                Err(_) => {
                    let mut failures = self.streaming_failures.lock();
                    *failures = failures.saturating_add(1);
                    *self.streaming_last_failure.lock() = Some(std::time::Instant::now());
                    self.failure_tracker.lock().record_failure();
                    let timeout_display = streaming_deadline
                        .map(|d| format!("{d:?}"))
                        .unwrap_or_else(|| "configured streaming timeout".to_string());
                    if !self.quiet {
                        println!(
                            "{} {} Streaming timed out after {}",
                            agent_prefix,
                            style("(WARN)").red().bold(),
                            timeout_display
                        );
                    }
                    let warning = format!("Streaming request timed out after {}", timeout_display);
                    event_recorder.warning(&warning);
                    warnings.push(warning);
                }
            }
        } else if streaming_disabled {
            let warning = "Skipping streaming after repeated streaming failures";
            warnings.push(warning.to_string());
            event_recorder.warning(warning);
        }

        if let Some(mut response) = streaming_response {
            *self.streaming_failures.lock() = 0;
            self.streaming_last_failure.lock().take();
            let response_text = response.content.take().unwrap_or_default();
            if !response_text.is_empty() {
                aggregated_text = response_text;
            }

            if !aggregated_text.trim().is_empty() {
                if event_recorder.agent_message_stream_update(&aggregated_text) {
                    agent_message_streamed = true;
                }
                if agent_message_streamed {
                    event_recorder.agent_message_stream_complete();
                }
                // Ensure the agent reply is always visible even if the TUI misses streaming updates
                Self::print_compact_response(&self.agent_type, &aggregated_text, self.quiet);
                if !self.quiet {
                    println!(
                        "{} {} {}",
                        agent_prefix,
                        style("(ASSISTANT)").green().bold(),
                        aggregated_text.trim()
                    );
                }
            } else if agent_message_streamed {
                event_recorder.agent_message_stream_complete();
            }

            if reasoning_recorded {
                event_recorder.reasoning_stream_complete();
                if response.reasoning.is_none() && !aggregated_reasoning.is_empty() {
                    response.reasoning = Some(aggregated_reasoning);
                }
            } else if let Some(ref reasoning) = response.reasoning {
                event_recorder.reasoning(reasoning);
                reasoning_recorded = true;
            }

            let reasoning = response.reasoning.clone();
            return Ok(ProviderResponseSummary {
                response: *response,
                content: aggregated_text,
                reasoning,
                agent_message_streamed,
                reasoning_recorded,
            });
        }

        if agent_message_streamed {
            event_recorder.agent_message_stream_complete();
        }

        // Check circuit breaker before fallback
        if self.failure_tracker.lock().should_circuit_break() {
            let backoff = self.failure_tracker.lock().backoff_duration();
            warn!(
                "Circuit breaker active after {} consecutive failures. Waiting {:?} before retry.",
                self.failure_tracker.lock().consecutive_failures,
                backoff
            );
            tokio::time::sleep(backoff).await;
        }

        // Optimize: Create fallback request without cloning if possible
        // We only need to change stream=false, so we can reuse the request
        let fallback_request = LLMRequest {
            stream: false,
            ..request.clone()
        };

        let generation_result = if let Some(limit) = generation_deadline {
            tokio::time::timeout(limit, self.provider_client.generate(fallback_request)).await
        } else {
            Ok(self.provider_client.generate(fallback_request).await)
        };

        let mut response = match generation_result {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                // Record failure for exponential backoff
                self.failure_tracker.lock().record_failure();
                if !self.quiet {
                    println!(
                        "{} {} Failed",
                        agent_prefix,
                        style("(ERROR)").red().bold().on_black()
                    );
                }
                return Err(anyhow!(
                    "Agent {} execution failed at turn {}: {}",
                    self.agent_type,
                    turn_index,
                    e
                ));
            }
            Err(_) => {
                self.failure_tracker.lock().record_failure();
                let warning = match generation_deadline {
                    Some(limit) => format!("LLM request timed out after {:?}", limit),
                    None => "LLM request timed out".to_string(),
                };
                event_recorder.warning(&warning);
                warnings.push(warning.clone());
                if !self.quiet {
                    println!(
                        "{} {} {}",
                        agent_prefix,
                        style("(WARN)").red().bold(),
                        warning
                    );
                }
                return Err(anyhow!(
                    "Agent {} execution failed at turn {}: request timed out",
                    self.agent_type,
                    turn_index
                ));
            }
        };

        let content = response.content.take().unwrap_or_default();
        let reasoning = response.reasoning.clone();

        // Reset failure tracker on success
        self.failure_tracker.lock().reset();
        *self.streaming_failures.lock() = self.streaming_failures.lock().saturating_sub(1);
        self.streaming_last_failure.lock().take();

        Ok(ProviderResponseSummary {
            response,
            content,
            reasoning,
            agent_message_streamed,
            reasoning_recorded,
        })
    }
}
