//! Compatibility wrapper over the shared agent runtime.

use crate::core::agent::events::unified::AgentEvent;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::session::AgentSessionState;
use crate::exec::events::{ThreadEvent, ThreadItemDetails, ToolCallStatus, Usage};
use crate::llm::provider::{LLMProvider, LLMRequest};
use anyhow::Result;
use parking_lot::Mutex as ParkingMutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

fn merged_delta(previous: &mut String, current: &str) -> Option<String> {
    if current.is_empty() || previous == current {
        return None;
    }

    let delta = current
        .strip_prefix(previous.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| current.to_string());
    previous.clear();
    previous.push_str(current);
    Some(delta)
}

#[derive(Default)]
struct LegacyEventAdapter {
    item_text: HashMap<String, String>,
    reasoning_stage: HashMap<String, Option<String>>,
}

impl LegacyEventAdapter {
    fn adapt(&mut self, event: &ThreadEvent) -> Vec<AgentEvent> {
        match event {
            ThreadEvent::ItemStarted(started) => {
                self.adapt_item_event(&started.item.id, &started.item.details, false, true)
            }
            ThreadEvent::ItemUpdated(updated) => {
                self.adapt_item_event(&updated.item.id, &updated.item.details, false, false)
            }
            ThreadEvent::ItemCompleted(completed) => {
                self.adapt_item_event(&completed.item.id, &completed.item.details, true, false)
            }
            ThreadEvent::Error(error) => vec![AgentEvent::Error {
                message: error.message.clone(),
            }],
            _ => Vec::new(),
        }
    }

    fn adapt_item_event(
        &mut self,
        item_id: &str,
        details: &ThreadItemDetails,
        completed: bool,
        emit_tool_start: bool,
    ) -> Vec<AgentEvent> {
        let mut events = Vec::new();

        match details {
            ThreadItemDetails::AgentMessage(message) => {
                let previous = self.item_text.entry(item_id.to_string()).or_default();
                if let Some(delta) = merged_delta(previous, &message.text) {
                    events.push(AgentEvent::OutputDelta { delta });
                }
            }
            ThreadItemDetails::Reasoning(reasoning) => {
                let previous_stage = self.reasoning_stage.entry(item_id.to_string()).or_default();
                if reasoning.stage != *previous_stage {
                    if let Some(stage) = reasoning.stage.clone() {
                        events.push(AgentEvent::ThinkingStage { stage });
                    }
                    *previous_stage = reasoning.stage.clone();
                }

                let previous = self.item_text.entry(item_id.to_string()).or_default();
                if let Some(delta) = merged_delta(previous, &reasoning.text) {
                    events.push(AgentEvent::ThinkingDelta { delta });
                }
            }
            ThreadItemDetails::ToolInvocation(tool) if !completed && emit_tool_start => {
                events.push(AgentEvent::ToolCallStarted {
                    id: tool
                        .tool_call_id
                        .clone()
                        .unwrap_or_else(|| item_id.to_string()),
                    name: tool.tool_name.clone(),
                    args: tool
                        .arguments
                        .as_ref()
                        .map(serde_json::Value::to_string)
                        .unwrap_or_else(|| "{}".to_string()),
                });
            }
            ThreadItemDetails::ToolInvocation(tool) if completed => {
                events.push(AgentEvent::ToolCallCompleted {
                    id: tool
                        .tool_call_id
                        .clone()
                        .unwrap_or_else(|| item_id.to_string()),
                    result: tool
                        .arguments
                        .as_ref()
                        .map(serde_json::Value::to_string)
                        .unwrap_or_else(|| "{}".to_string()),
                    is_success: matches!(tool.status, ToolCallStatus::Completed),
                });
            }
            ThreadItemDetails::Error(error) => {
                events.push(AgentEvent::Error {
                    message: error.message.clone(),
                });
            }
            _ => {}
        }

        if completed {
            self.item_text.remove(item_id);
            self.reasoning_stage.remove(item_id);
        }

        events
    }
}

/// A sink for legacy unified agent events.
pub type AgentEventSink = Arc<Mutex<Box<dyn FnMut(AgentEvent) + Send>>>;

/// Compatibility wrapper that forwards turn execution into `AgentRuntime`.
pub struct AgentSessionController {
    pub runtime: AgentRuntime,
    event_sink: Option<AgentEventSink>,
}

impl AgentSessionController {
    pub fn new(state: AgentSessionState, event_sink: Option<AgentEventSink>) -> Self {
        Self {
            runtime: AgentRuntime::new(state, None, None),
            event_sink,
        }
    }

    pub fn set_event_handler(&mut self, sink: AgentEventSink) {
        self.event_sink = Some(sink);
    }

    pub fn state(&self) -> &AgentSessionState {
        &self.runtime.state
    }

    pub fn state_mut(&mut self) -> &mut AgentSessionState {
        &mut self.runtime.state
    }

    fn emit(&mut self, event: AgentEvent) {
        if let Some(sink) = &self.event_sink {
            let mut callback = match sink.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            callback(event);
        }
    }

    pub async fn run_turn(
        &mut self,
        provider: &mut Box<dyn LLMProvider>,
        request: LLMRequest,
        steering: &mut Option<
            tokio::sync::mpsc::UnboundedReceiver<crate::core::agent::steering::SteeringMessage>,
        >,
        timeout: Option<std::time::Duration>,
    ) -> Result<(crate::llm::provider::LLMResponse, String, Option<String>)> {
        self.emit(AgentEvent::TurnStarted {
            id: format!("turn_{}", self.runtime.state.stats.turns_executed + 1),
        });

        let thread_sink = self.event_sink.as_ref().map(|legacy_sink| {
            let legacy_sink = Arc::clone(legacy_sink);
            let adapter = Arc::new(Mutex::new(LegacyEventAdapter::default()));
            let adapter_ref = Arc::clone(&adapter);
            Arc::new(ParkingMutex::new(Box::new(move |event: &ThreadEvent| {
                let mut adapter = match adapter_ref.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let mapped = adapter.adapt(event);
                drop(adapter);

                for mapped_event in mapped {
                    let mut callback = match legacy_sink.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    callback(mapped_event);
                }
            })
                as Box<dyn FnMut(&ThreadEvent) + Send>))
        });

        self.runtime.set_event_handler(thread_sink);
        self.runtime.set_steering_receiver(steering.take());
        let result = self.runtime.run_turn_once(provider, request, timeout).await;
        *steering = self.runtime.take_steering_receiver();
        self.runtime.set_event_handler(None);

        match result {
            Ok(turn) => {
                let usage = turn
                    .response
                    .usage
                    .clone()
                    .map(|usage| Usage {
                        input_tokens: usage.prompt_tokens as u64,
                        output_tokens: usage.completion_tokens as u64,
                        cached_input_tokens: usage.cache_read_tokens_or_fallback() as u64,
                    })
                    .unwrap_or_default();
                let finish_reason = match turn.response.finish_reason.clone() {
                    crate::llm::provider::FinishReason::Stop => "stop".to_string(),
                    crate::llm::provider::FinishReason::ToolCalls => "tool_calls".to_string(),
                    crate::llm::provider::FinishReason::Length => "length".to_string(),
                    crate::llm::provider::FinishReason::Error(message) => message,
                    _ => "unknown".to_string(),
                };
                self.emit(AgentEvent::TurnCompleted {
                    finish_reason,
                    usage,
                });
                Ok((turn.response, turn.content, turn.reasoning))
            }
            Err(err) => {
                self.emit(AgentEvent::Error {
                    message: err.to_string(),
                });
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use std::sync::{Arc, Mutex};

    use crate::llm::provider::{
        AssistantPhase, LLMError, LLMNormalizedStream, LLMResponse, LLMStream, LLMStreamEvent,
        NormalizedStreamEvent, ToolCall, Usage,
    };

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

        async fn stream_normalized(
            &self,
            _request: LLMRequest,
        ) -> Result<LLMNormalizedStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![Ok(
                NormalizedStreamEvent::Done {
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
        let mut controller = AgentSessionController::new(state, None);
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

            async fn stream_normalized(
                &self,
                _request: LLMRequest,
            ) -> Result<LLMNormalizedStream, LLMError> {
                Ok(Box::pin(stream::iter(vec![
                    Ok(NormalizedStreamEvent::TextDelta {
                        delta: "prefix ".to_string(),
                    }),
                    Ok(NormalizedStreamEvent::ReasoningDelta {
                        delta: "reasoning ".to_string(),
                    }),
                    Ok(NormalizedStreamEvent::Done {
                        response: Box::new(LLMResponse {
                            content: Some("prefix **suffix**".to_string()),
                            reasoning: Some("reasoning _suffix_".to_string()),
                            model: "test-model".to_string(),
                            finish_reason: crate::llm::provider::FinishReason::Stop,
                            ..Default::default()
                        }),
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
        let mut controller = AgentSessionController::new(state, None);
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

    #[tokio::test]
    async fn run_turn_marks_tool_call_responses_as_commentary() {
        let response = LLMResponse {
            content: Some("Checking prerequisites".to_string()),
            model: "test-model".to_string(),
            finish_reason: crate::llm::provider::FinishReason::ToolCalls,
            tool_calls: Some(vec![ToolCall::function(
                "call_1".to_string(),
                "unified_search".to_string(),
                r#"{"action":"grep","pattern":"phase"}"#.to_string(),
            )]),
            ..Default::default()
        };
        let provider = CompletedOnlyStreamProvider {
            response: response.clone(),
        };
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut controller = AgentSessionController::new(state, None);
        let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
        let request = LLMRequest {
            model: "test-model".to_string(),
            ..Default::default()
        };
        let mut steering = None;

        let (resp, _, _) = controller
            .run_turn(&mut provider_box, request, &mut steering, None)
            .await
            .expect("run_turn should succeed");

        let last = controller
            .state()
            .messages
            .last()
            .expect("assistant message should be recorded");
        assert_eq!(last.phase, Some(AssistantPhase::Commentary));
        assert_eq!(resp.tool_calls.as_ref().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn run_turn_normalizes_empty_tool_call_lists_to_final_answer() {
        let response = LLMResponse {
            content: Some("Done".to_string()),
            model: "test-model".to_string(),
            finish_reason: crate::llm::provider::FinishReason::Stop,
            tool_calls: Some(vec![]),
            ..Default::default()
        };
        let provider = CompletedOnlyStreamProvider {
            response: response.clone(),
        };
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut controller = AgentSessionController::new(state, None);
        let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
        let request = LLMRequest {
            model: "test-model".to_string(),
            ..Default::default()
        };
        let mut steering = None;

        let (resp, _, _) = controller
            .run_turn(&mut provider_box, request, &mut steering, None)
            .await
            .expect("run_turn should succeed");

        let last = controller
            .state()
            .messages
            .last()
            .expect("assistant message should be recorded");
        assert_eq!(last.phase, Some(AssistantPhase::FinalAnswer));
        assert!(last.tool_calls.is_none());
        assert!(resp.tool_calls.is_none());
    }

    #[tokio::test]
    async fn run_turn_consumes_normalized_tool_and_usage_events() {
        #[derive(Clone)]
        struct NormalizedProvider;

        #[async_trait]
        impl LLMProvider for NormalizedProvider {
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
                panic!("legacy stream should not be used for normalized consumer")
            }

            async fn stream_normalized(
                &self,
                _request: LLMRequest,
            ) -> Result<LLMNormalizedStream, LLMError> {
                Ok(Box::pin(stream::iter(vec![
                    Ok(NormalizedStreamEvent::ToolCallStart {
                        call_id: "call_1".to_string(),
                        name: Some("unified_search".to_string()),
                    }),
                    Ok(NormalizedStreamEvent::TextDelta {
                        delta: "Searching...".to_string(),
                    }),
                    Ok(NormalizedStreamEvent::Usage {
                        usage: Usage {
                            prompt_tokens: 12,
                            completion_tokens: 7,
                            total_tokens: 19,
                            cached_prompt_tokens: None,
                            cache_creation_tokens: None,
                            cache_read_tokens: None,
                        },
                    }),
                    Ok(NormalizedStreamEvent::Done {
                        response: Box::new(LLMResponse {
                            content: Some("Searching...".to_string()),
                            model: "test-model".to_string(),
                            finish_reason: crate::llm::provider::FinishReason::ToolCalls,
                            tool_calls: Some(vec![ToolCall::function(
                                "call_1".to_string(),
                                "unified_search".to_string(),
                                r#"{"pattern":"phase"}"#.to_string(),
                            )]),
                            usage: None,
                            ..Default::default()
                        }),
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

        let captured_events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&captured_events);
        let sink: AgentEventSink = Arc::new(Mutex::new(Box::new(move |event| {
            sink_events
                .lock()
                .expect("event capture mutex should not be poisoned")
                .push(event);
        })));

        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut controller = AgentSessionController::new(state, Some(sink));
        let mut provider_box: Box<dyn LLMProvider> = Box::new(NormalizedProvider);
        let request = LLMRequest {
            model: "test-model".to_string(),
            ..Default::default()
        };
        let mut steering = None;

        let (resp, content, _) = controller
            .run_turn(&mut provider_box, request, &mut steering, None)
            .await
            .expect("run_turn should succeed");

        assert_eq!(content, "Searching...");
        assert_eq!(
            resp.usage.as_ref().map(|usage| usage.total_tokens),
            Some(19)
        );

        let events = captured_events
            .lock()
            .expect("event capture mutex should not be poisoned");
        let tool_call_started = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::ToolCallStarted { id, name, .. } => Some((id.as_str(), name.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_call_started, vec![("call_1", "unified_search")]);
    }
}
