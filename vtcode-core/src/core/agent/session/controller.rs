//! Unified controller for driving agent sessions.

use crate::core::agent::events::unified::AgentEvent;
use crate::core::agent::session::AgentSessionState;
use crate::exec::events::ThreadEvent; // Temporary compatibility
use crate::llm::provider::{LLMProvider, LLMRequest, LLMStreamEvent, Usage};
use anyhow::Result;
use std::sync::Mutex;
use std::sync::Arc;

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
        steering: &mut Option<tokio::sync::mpsc::UnboundedReceiver<crate::core::agent::steering::SteeringMessage>>,
        timeout: Option<std::time::Duration>,
    ) -> Result<(crate::llm::provider::LLMResponse, String, Option<String>)> {
        let turn_id = format!("turn_{}", self.state.stats.turns_executed);
        self.emit(AgentEvent::TurnStarted {
            id: turn_id.clone(),
        });

        let mut start_time = std::time::Instant::now();
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

        while let Some(event_result) = stream.next().await {
            // Check steering
            if let Some(rx) = steering {
                match rx.try_recv() {
                    Ok(crate::core::agent::steering::SteeringMessage::Stop) => {
                        finish_reason = "cancelled".to_string();
                        break;
                    }
                    Ok(crate::core::agent::steering::SteeringMessage::Pause) => {
                        self.emit(AgentEvent::ThinkingStage { stage: "Paused".to_string() });
                        // Wait for resume
                        loop {
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            match rx.try_recv() {
                                Ok(crate::core::agent::steering::SteeringMessage::Resume) => break,
                                Ok(crate::core::agent::steering::SteeringMessage::Stop) => {
                                    finish_reason = "cancelled".to_string();
                                    break;
                                }
                                _ => {}
                            }
                        }
                        if finish_reason == "cancelled" { break; }
                        self.emit(AgentEvent::ThinkingStage { stage: "Resumed".to_string() });
                    }
                    Ok(crate::core::agent::steering::SteeringMessage::InjectInput(input)) => {
                        // For now, history injection happens after the turn in most VTCode logic
                        // but we could support it here if needed.
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

        Ok((
            crate::llm::provider::LLMResponse {
                content: Some(full_text.clone()),
                tool_calls: tool_calls.clone(),
                finish_reason: if finish_reason == "tool_calls" { 
                    crate::llm::provider::FinishReason::ToolCalls 
                } else if finish_reason == "cancelled" {
                    crate::llm::provider::FinishReason::Error("Cancelled".to_string())
                } else {
                    crate::llm::provider::FinishReason::Stop
                },
                usage: Some(final_usage),
                ..Default::default()
            },
            full_text,
            if full_reasoning.is_empty() { None } else { Some(full_reasoning) }
        ))
    }
}
