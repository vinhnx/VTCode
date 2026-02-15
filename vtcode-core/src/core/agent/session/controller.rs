//! Unified controller for driving agent sessions.

use crate::core::agent::events::unified::AgentEvent;
use crate::core::agent::session::AgentSessionState;
use crate::exec::events::ThreadEvent; // Temporary compatibility
use crate::llm::provider::{LLMProvider, LLMRequest, LLMStreamEvent, Usage};
use anyhow::Result;
use std::sync::Arc;
use std::sync::Mutex;

fn merge_stream_and_completed_text(accumulated: &mut String, completed: Option<&str>) {
    let Some(completed_text) = completed else {
        return;
    };
    if completed_text.is_empty() {
        return;
    }
    if accumulated.is_empty() {
        accumulated.push_str(completed_text);
        return;
    }
    if completed_text == accumulated.as_str() {
        return;
    }
    if let Some(suffix) = completed_text.strip_prefix(accumulated.as_str()) {
        accumulated.push_str(suffix);
        return;
    }
    // If final payload differs from streamed deltas, prefer final provider payload.
    *accumulated = completed_text.to_string();
}

/// A sink for unified agent events.
pub type AgentEventSink = Arc<Mutex<Box<dyn FnMut(AgentEvent) + Send>>>;

/// Controller that drives an agent session using the unified event model.
pub struct AgentSessionController {
    /// The current state of the session.
    pub state: AgentSessionState,

    /// The event sink for emitting unified events.
    event_sink: Option<AgentEventSink>,

    /// Compatibility sink for old-style ThreadEvents (if needed).
    thread_event_sink: Option<crate::core::agent::events::EventSink>,
}

use futures::StreamExt;

impl AgentSessionController {
    /// Create a new controller with the given state
    pub fn new(
        state: AgentSessionState,
        event_sink: Option<AgentEventSink>,
        thread_event_sink: Option<crate::core::agent::events::EventSink>,
    ) -> Self {
        Self {
            state,
            event_sink,
            thread_event_sink,
        }
    }

    /// Set an event handler for this controller
    pub fn set_event_handler(&mut self, sink: AgentEventSink) {
        self.event_sink = Some(sink);
    }

    /// Set a legacy thread event handler for this controller
    pub fn set_thread_event_handler(&mut self, sink: crate::core::agent::events::EventSink) {
        self.thread_event_sink = Some(sink);
    }

    /// Emit a unified agent event and handle legacy compatibility.
    pub fn emit(&mut self, event: AgentEvent) {
        // Emit legacy events first for compatibility
        self.emit_legacy(&event);

        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock().expect("mutex poisoned");
            callback(event);
        }
    }

    /// Translate `AgentEvent` to `ThreadEvent` for legacy sinks.
    fn emit_legacy(&mut self, event: &AgentEvent) {
        let Some(sink) = &self.thread_event_sink else {
            return;
        };

        let thread_event = match event {
            AgentEvent::TurnStarted { .. } => {
                ThreadEvent::TurnStarted(crate::exec::events::TurnStartedEvent::default())
            }
            AgentEvent::TurnCompleted { usage, .. } => {
                ThreadEvent::TurnCompleted(crate::exec::events::TurnCompletedEvent {
                    usage: usage.clone(),
                })
            }
            AgentEvent::Error { message } => {
                ThreadEvent::TurnFailed(crate::exec::events::TurnFailedEvent {
                    message: message.clone(),
                    usage: None,
                })
            }
            // Logic for mapping streaming tokens/reasoning to ThreadItems
            // would go here if we want to fully bridge.
            _ => return,
        };

        let mut callback = sink.lock();
        callback(&thread_event);
    }

    /// Drive a single turn in the session.
    pub async fn run_turn(
        &mut self,
        provider: &mut Box<dyn LLMProvider>,
        request: LLMRequest,
        steering: &mut Option<
            tokio::sync::mpsc::UnboundedReceiver<crate::core::agent::steering::SteeringMessage>,
        >,
        timeout: Option<std::time::Duration>,
    ) -> Result<(crate::llm::provider::LLMResponse, String, Option<String>)> {
        let turn_id = format!("turn_{}", self.state.stats.turns_executed);
        self.emit(AgentEvent::TurnStarted {
            id: turn_id.clone(),
        });
        let request_model = request.model.clone();

        let start_time = std::time::Instant::now();
        let mut stream = if let Some(t) = timeout {
            match tokio::time::timeout(t, provider.stream(request)).await {
                Ok(res) => res?,
                Err(_) => return Err(anyhow::anyhow!("Stream request timed out after {:?}", t)),
            }
        } else {
            provider.stream(request).await?
        };

        let mut full_text = String::new();
        let mut full_reasoning = String::new();
        let mut finish_reason = String::from("stop");
        let mut final_usage = Usage::default();
        let mut aggregated_tool_calls: Option<Vec<crate::llm::provider::ToolCall>> = None;
        let mut completed_response: Option<crate::llm::provider::LLMResponse> = None;

        while let Some(event_result) = stream.next().await {
            // Check steering
            if let Some(rx) = steering {
                match rx.try_recv() {
                    Ok(crate::core::agent::steering::SteeringMessage::SteerStop) => {
                        finish_reason = "cancelled".to_string();
                        break;
                    }
                    Ok(crate::core::agent::steering::SteeringMessage::Pause) => {
                        self.emit(AgentEvent::ThinkingStage {
                            stage: "Paused".to_string(),
                        });
                        // Wait for resume
                        loop {
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            match rx.try_recv() {
                                Ok(crate::core::agent::steering::SteeringMessage::Resume) => break,
                                Ok(crate::core::agent::steering::SteeringMessage::SteerStop) => {
                                    finish_reason = "cancelled".to_string();
                                    break;
                                }
                                _ => {}
                            }
                        }
                        if finish_reason == "cancelled" {
                            break;
                        }
                        self.emit(AgentEvent::ThinkingStage {
                            stage: "Resumed".to_string(),
                        });
                    }
                    Ok(crate::core::agent::steering::SteeringMessage::FollowUpInput(input)) => {
                        // Follow-up input is added to state, ensuring next turn handles it.
                        self.state.add_user_message(input);
                    }
                    _ => {}
                }
            }

            match event_result? {
                LLMStreamEvent::Token { delta } => {
                    full_text.push_str(&delta);
                    self.emit(AgentEvent::OutputDelta { delta });
                }
                LLMStreamEvent::Reasoning { delta } => {
                    full_reasoning.push_str(&delta);
                    self.emit(AgentEvent::ThinkingDelta { delta });
                }
                LLMStreamEvent::ReasoningStage { stage } => {
                    self.state.current_stage = Some(stage.clone());
                    self.emit(AgentEvent::ThinkingStage { stage });
                }
                LLMStreamEvent::Completed { response } => {
                    completed_response = Some((*response).clone());
                    finish_reason = match response.finish_reason.clone() {
                        crate::llm::provider::FinishReason::Stop => "stop".to_string(),
                        crate::llm::provider::FinishReason::ToolCalls => "tool_calls".to_string(),
                        crate::llm::provider::FinishReason::Length => "length".to_string(),
                        crate::llm::provider::FinishReason::Error(s) => s,
                        _ => "unknown".to_string(),
                    };
                    final_usage = response.usage.clone().unwrap_or_default();
                    aggregated_tool_calls = response.tool_calls.clone();

                    // Handle tool calls in the response
                    if let Some(tool_calls) = &response.tool_calls {
                        for call in tool_calls {
                            self.emit(AgentEvent::ToolCallStarted {
                                id: call.id.clone(),
                                name: call
                                    .function
                                    .as_ref()
                                    .map(|f| f.name.clone())
                                    .unwrap_or_default(),
                                args: call
                                    .function
                                    .as_ref()
                                    .map(|f| f.arguments.clone())
                                    .unwrap_or_else(|| "{}".to_string()),
                            });
                        }
                    }
                }
            }
        }

        merge_stream_and_completed_text(
            &mut full_text,
            completed_response
                .as_ref()
                .and_then(|resp| resp.content.as_deref()),
        );
        merge_stream_and_completed_text(
            &mut full_reasoning,
            completed_response
                .as_ref()
                .and_then(|resp| resp.reasoning.as_deref()),
        );

        let mut turn_recorded = false;
        self.state.record_turn(&start_time, &mut turn_recorded);

        // Update stats with final usage
        if final_usage.prompt_tokens > 0 || final_usage.completion_tokens > 0 {
            self.state.stats.merge_usage(final_usage.clone());
        }

        // Create and push assistant message
        let mut assistant_msg = crate::llm::provider::Message::assistant(full_text.clone());
        if !full_reasoning.is_empty() {
            assistant_msg = assistant_msg.with_reasoning(Some(full_reasoning.clone()));
        }

        // Handle tool calls in the response summary
        let mut tool_calls = None;
        if let Some(calls) = aggregated_tool_calls.clone() {
            assistant_msg = assistant_msg.with_tool_calls(calls.clone());
            tool_calls = Some(calls);
        }

        self.state.messages.push(assistant_msg.clone());

        // Handle Gemini content conversion if needed (legacy)
        let parts = vec![crate::gemini::Part::Text {
            text: full_text.clone(),
            thought_signature: None,
        }];
        self.state.conversation.push(crate::gemini::Content {
            role: "model".to_string(),
            parts,
        });
        self.state.last_processed_message_idx = self.state.conversation.len();

        self.emit(AgentEvent::TurnCompleted {
            finish_reason: finish_reason.clone(),
            usage: crate::exec::events::Usage {
                input_tokens: final_usage.prompt_tokens as u64,
                output_tokens: final_usage.completion_tokens as u64,
                cached_input_tokens: final_usage.cached_prompt_tokens.unwrap_or(0) as u64,
            },
        });

        let mut response = completed_response.unwrap_or_default();
        if response.model.is_empty() {
            response.model = request_model;
        }
        response.content = Some(full_text.clone());
        response.reasoning = if full_reasoning.is_empty() {
            None
        } else {
            Some(full_reasoning.clone())
        };
        response.tool_calls = tool_calls.clone();
        response.usage = Some(final_usage);
        response.finish_reason = if finish_reason == "tool_calls" {
            crate::llm::provider::FinishReason::ToolCalls
        } else if finish_reason == "cancelled" {
            crate::llm::provider::FinishReason::Error("Cancelled".to_string())
        } else {
            crate::llm::provider::FinishReason::Stop
        };

        Ok((
            response,
            full_text,
            if full_reasoning.is_empty() {
                None
            } else {
                Some(full_reasoning)
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;

    use crate::llm::provider::{LLMError, LLMResponse, LLMStream, LLMStreamEvent};

    #[derive(Clone)]
    struct CompletedOnlyStreamProvider {
        response: LLMResponse,
    }

    #[async_trait]
    impl LLMProvider for CompletedOnlyStreamProvider {
        fn name(&self) -> &str {
            "test-provider"
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(self.response.clone())
        }

        async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![Ok(
                LLMStreamEvent::Completed {
                    response: Box::new(self.response.clone()),
                },
            )])))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn run_turn_uses_completed_payload_when_no_deltas_exist() {
        let response = LLMResponse {
            content: Some("### Header\n- item".to_string()),
            model: "test-model".to_string(),
            finish_reason: crate::llm::provider::FinishReason::Stop,
            reasoning: Some("**why** this works".to_string()),
            ..Default::default()
        };
        let provider = CompletedOnlyStreamProvider {
            response: response.clone(),
        };
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut controller = AgentSessionController::new(state, None, None);
        let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
        let request = LLMRequest {
            model: "test-model".to_string(),
            ..Default::default()
        };
        let mut steering = None;

        let (resp, content, reasoning) = controller
            .run_turn(&mut provider_box, request, &mut steering, None)
            .await
            .expect("run_turn should succeed");

        assert_eq!(content, "### Header\n- item");
        assert_eq!(reasoning.as_deref(), Some("**why** this works"));
        assert_eq!(resp.content.as_deref(), Some("### Header\n- item"));
        assert_eq!(resp.reasoning.as_deref(), Some("**why** this works"));
    }

    #[tokio::test]
    async fn run_turn_merges_completed_payload_with_streamed_prefix() {
        #[derive(Clone)]
        struct PrefixThenCompletedProvider;

        #[async_trait]
        impl LLMProvider for PrefixThenCompletedProvider {
            fn name(&self) -> &str {
                "test-provider"
            }

            fn supports_streaming(&self) -> bool {
                true
            }

            async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
                Ok(LLMResponse::default())
            }

            async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
                let completed = LLMResponse {
                    content: Some("prefix **suffix**".to_string()),
                    reasoning: Some("reasoning _suffix_".to_string()),
                    model: "test-model".to_string(),
                    finish_reason: crate::llm::provider::FinishReason::Stop,
                    ..Default::default()
                };
                Ok(Box::pin(stream::iter(vec![
                    Ok(LLMStreamEvent::Token {
                        delta: "prefix ".to_string(),
                    }),
                    Ok(LLMStreamEvent::Reasoning {
                        delta: "reasoning ".to_string(),
                    }),
                    Ok(LLMStreamEvent::Completed {
                        response: Box::new(completed),
                    }),
                ])))
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["test-model".to_string()]
            }

            fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
                Ok(())
            }
        }

        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut controller = AgentSessionController::new(state, None, None);
        let mut provider_box: Box<dyn LLMProvider> = Box::new(PrefixThenCompletedProvider);
        let request = LLMRequest {
            model: "test-model".to_string(),
            ..Default::default()
        };
        let mut steering = None;

        let (resp, content, reasoning) = controller
            .run_turn(&mut provider_box, request, &mut steering, None)
            .await
            .expect("run_turn should succeed");

        assert_eq!(content, "prefix **suffix**");
        assert_eq!(reasoning.as_deref(), Some("reasoning _suffix_"));
        assert_eq!(resp.content.as_deref(), Some("prefix **suffix**"));
        assert_eq!(resp.reasoning.as_deref(), Some("reasoning _suffix_"));
    }
}
