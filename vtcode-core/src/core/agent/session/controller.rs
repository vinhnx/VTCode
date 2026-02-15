//! Unified controller for driving agent sessions.

use crate::core::agent::events::unified::AgentEvent;
use crate::core::agent::session::AgentSessionState;
use crate::exec::events::ThreadEvent; // Temporary compatibility
use crate::llm::provider::{LLMProvider, LLMRequest, LLMStreamEvent, Usage};
use anyhow::Result;
use parking_lot::Mutex;
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

    /// Emit a unified agent event and handle legacy compatibility.
    pub fn emit(&mut self, event: AgentEvent) {
        // Emit legacy events first for compatibility
        self.emit_legacy(&event);

        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
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
    ) -> Result<()> {
        let turn_id = format!("turn_{}", self.state.stats.turns_executed);
        self.emit(AgentEvent::TurnStarted {
            id: turn_id.clone(),
        });

        let start_time = std::time::Instant::now();
        let mut stream = provider.stream(request).await?;

        let mut full_text = String::new();
        let mut full_reasoning = String::new();
        let mut finish_reason = String::from("stop");
        let mut final_usage = Usage::default();

        while let Some(event_result) = stream.next().await {
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
        self.state.stats.merge_usage(final_usage.clone());

        self.emit(AgentEvent::TurnCompleted {
            finish_reason,
            usage: vtcode_exec_events::Usage {
                input_tokens: final_usage.prompt_tokens as u64,
                output_tokens: final_usage.completion_tokens as u64,
                cached_input_tokens: final_usage.cached_prompt_tokens.unwrap_or(0) as u64,
            },
        });

        Ok(())
    }
}
