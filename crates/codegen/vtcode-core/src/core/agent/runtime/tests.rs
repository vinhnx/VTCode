use super::*;
use async_trait::async_trait;
use futures::stream;

use crate::llm::provider::{
    LLMError, LLMNormalizedStream, LLMStream, LLMStreamEvent, NormalizedStreamEvent, ReasoningSource,
};

#[derive(Clone)]
struct CompletedOnlyStreamProvider {
    response: LLMResponse,
}

#[derive(Clone)]
struct DeltaStreamProvider {
    response: LLMResponse,
    text_delta: String,
    reasoning_delta: String,
    reasoning_source: ReasoningSource,
    tool_call_start: bool,
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
        Ok(Box::pin(stream::iter(vec![Ok(LLMStreamEvent::Completed {
            response: Box::new(self.response.clone()),
        })])))
    }

    async fn stream_normalized(&self, _request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        Ok(Box::pin(stream::iter(vec![Ok(NormalizedStreamEvent::Done {
            response: Box::new(self.response.clone()),
        })])))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

#[async_trait]
impl LLMProvider for DeltaStreamProvider {
    fn name(&self) -> &str {
        "delta-provider"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(self.response.clone())
    }

    async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
        Ok(Box::pin(stream::iter(vec![Ok(LLMStreamEvent::Completed {
            response: Box::new(self.response.clone()),
        })])))
    }

    async fn stream_normalized(&self, _request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        let mut events = vec![Ok(NormalizedStreamEvent::ReasoningDelta {
            delta: self.reasoning_delta.clone(),
            source: self.reasoning_source,
        })];
        if self.tool_call_start {
            events.push(Ok(NormalizedStreamEvent::ToolCallStart {
                call_id: "call_test".to_string(),
                name: Some("read_file".to_string()),
            }));
        }
        events.extend([
            Ok(NormalizedStreamEvent::TextDelta { delta: self.text_delta.clone() }),
            Ok(NormalizedStreamEvent::Done { response: Box::new(self.response.clone()) }),
        ]);
        Ok(Box::pin(stream::iter(events)))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

#[tokio::test]
async fn queued_follow_up_inputs_are_applied_one_at_a_time() {
    let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = AgentRuntime::new(state, None, Some(receiver));

    sender
        .send(SteeringMessage::FollowUpInput("first".to_string()))
        .expect("first follow-up should queue");
    sender
        .send(SteeringMessage::FollowUpInput("second".to_string()))
        .expect("second follow-up should queue");

    assert_eq!(runtime.poll_turn_control().await, RuntimeControl::Continue);
    assert!(runtime.has_pending_follow_up_inputs());
    assert!(runtime.state.messages.is_empty());

    assert_eq!(runtime.run_until_idle().as_deref(), Some("first"));
    assert_eq!(
        runtime
            .state
            .messages
            .last()
            .map(|message| message.get_text_content().into_owned()),
        Some("first".to_string())
    );
    assert!(runtime.has_pending_follow_up_inputs());

    assert_eq!(runtime.run_until_idle().as_deref(), Some("second"));
    assert_eq!(
        runtime
            .state
            .messages
            .last()
            .map(|message| message.get_text_content().into_owned()),
        Some("second".to_string())
    );
    assert!(!runtime.has_pending_follow_up_inputs());
}

#[test]
fn follow_up_queue_has_bounded_identity_preserving_fifo() {
    let mut steering = RuntimeSteering::default();
    let total_capacity = MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS;
    for index in 0..total_capacity {
        assert!(
            steering.try_queue_follow_up_input(format!("input-{index}")).is_ok(),
            "input {index} must be accepted within the combined capacity"
        );
    }
    let error = steering
        .try_queue_follow_up_input("overflow".to_string())
        .expect_err("overflow must be reported once both buffers are full");
    assert_eq!(error.capacity, total_capacity);

    // Primary FIFO drains first in order; overflow parks then refill.
    let first = steering.pop_follow_up_intent().expect("first intent should exist");
    let second = steering.pop_follow_up_intent().expect("second intent should exist");
    assert_ne!(first.id(), second.id());
    assert_eq!(first.text(), "input-0");
    assert_eq!(second.text(), "input-1");
}

#[test]
fn follow_up_overflow_parks_instead_of_dropping_bursts() {
    let mut steering = RuntimeSteering::default();
    // Fill the primary FIFO.
    for index in 0..MAX_QUEUED_FOLLOW_UP_INTENTS {
        assert!(steering.try_queue_follow_up_input(format!("primary-{index}")).is_ok());
    }
    // Burst beyond primary parks in overflow rather than dropping.
    for index in 0..5 {
        assert!(steering.try_queue_follow_up_input(format!("overflow-{index}")).is_ok());
    }
    assert!(steering.has_pending_follow_up_inputs());

    // Drain the primary; overflow items must arrive afterwards in order.
    for index in 0..MAX_QUEUED_FOLLOW_UP_INTENTS {
        let intent = steering.pop_follow_up_intent().expect("primary intent");
        assert_eq!(intent.text(), format!("primary-{index}"));
    }
    for index in 0..5 {
        let intent = steering.pop_follow_up_intent().expect("overflow intent");
        assert_eq!(intent.text(), format!("overflow-{index}"));
    }
    assert!(!steering.has_pending_follow_up_inputs());
}

#[test]
fn pop_yields_overflow_when_primary_is_empty() {
    // In-flight occupancy can park new intents in overflow while the
    // primary FIFO is empty (admission counts in-flight). `has_pending`
    // staying true must never race a `None` pop.
    let mut steering = RuntimeSteering::default();
    for index in 0..MAX_QUEUED_FOLLOW_UP_INTENTS {
        assert!(steering.try_queue_follow_up_input(format!("inflight-{index}")).is_ok());
    }
    assert_eq!(steering.drain_follow_up_intents_to_in_flight().len(), MAX_QUEUED_FOLLOW_UP_INTENTS);
    assert!(steering.queued_follow_up_inputs.is_empty(), "primary must be empty after a full in-flight drain");
    assert!(steering.try_queue_follow_up_input("parked".to_string()).is_ok());
    assert!(steering.has_pending_follow_up_inputs());
    let intent = steering
        .pop_follow_up_intent()
        .expect("pop must yield overflow when primary is empty");
    assert_eq!(intent.text(), "parked");
}

#[test]
fn new_follow_up_does_not_overtake_parked_overflow() {
    let mut steering = RuntimeSteering::default();
    for index in 0..MAX_QUEUED_FOLLOW_UP_INTENTS {
        steering.try_queue_follow_up_input(format!("primary-{index}")).unwrap();
    }
    steering.try_queue_follow_up_input("older overflow".into()).unwrap();
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("primary-0"));
    steering.try_queue_follow_up_input("new input".into()).unwrap();
    for index in 1..MAX_QUEUED_FOLLOW_UP_INTENTS {
        assert_eq!(steering.pop_follow_up_input().as_deref(), Some(format!("primary-{index}").as_str()));
    }
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("older overflow"));
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("new input"));
}

#[test]
fn recovered_follow_up_releases_capacity_without_overtaking() {
    let mut steering = RuntimeSteering::default();
    let capacity = MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS;
    for index in 0..capacity {
        steering.try_queue_follow_up_input(format!("queued-{index}")).unwrap();
    }
    steering
        .try_queue_recovered_follow_up(QueuedFollowUpIntent::from_parts("id", "recovered"))
        .unwrap();
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("queued-0"));
    assert!(steering.try_queue_follow_up_input("too-soon".into()).is_err());
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("queued-1"));
    steering.try_queue_follow_up_input("newer".into()).unwrap();
    for index in 2..capacity {
        assert_eq!(steering.pop_follow_up_input().as_deref(), Some(format!("queued-{index}").as_str()));
    }
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("recovered"));
    assert_eq!(steering.pop_follow_up_input().as_deref(), Some("newer"));
}

#[test]
fn recovered_follow_up_stays_ordered_and_drains_mid_turn() {
    let mut steering = RuntimeSteering::default();
    let capacity = MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS;
    for index in 0..capacity {
        steering.try_queue_follow_up_input(format!("queued-{index}")).unwrap();
    }
    steering
        .try_queue_recovered_follow_up(QueuedFollowUpIntent::from_parts("recovered-id", "interrupted"))
        .unwrap();
    assert!(steering.try_queue_follow_up_input("newer".into()).is_err());
    let mut delivered = Vec::new();
    while steering.has_pending_follow_up_inputs() {
        delivered.extend(steering.drain_follow_up_intents_to_in_flight());
    }
    assert_eq!(delivered.len(), capacity + 1);
    for (index, intent) in delivered[..capacity].iter().enumerate() {
        assert_eq!(intent.text(), format!("queued-{index}"));
    }
    assert_eq!(delivered.last().unwrap().id(), "recovered-id");
}

#[test]
fn pending_snapshot_restores_in_flight_before_queued_intents() {
    let state = AgentSessionState::new("session".into(), 16, 4, 128_000);
    let mut runtime = AgentRuntime::new(state, None, None);
    runtime.try_queue_follow_up_input("first".into()).unwrap();
    runtime.try_queue_follow_up_input("second".into()).unwrap();
    assert_eq!(runtime.run_until_idle().as_deref(), Some("first"));
    let snapshot = runtime.steering.pending_follow_up_intents_snapshot();
    let mut restored = AgentRuntime::new(AgentSessionState::new("restored".into(), 16, 4, 128_000), None, None);
    restored.restore_follow_up_state(snapshot, Vec::new()).unwrap();
    assert_eq!(restored.run_until_idle().as_deref(), Some("first"));
    assert_eq!(restored.run_until_idle().as_deref(), Some("second"));
}

#[test]
fn run_until_idle_tags_and_acknowledges_follow_up_intent() {
    let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    let mut runtime = AgentRuntime::new(state, None, None);
    runtime
        .try_queue_follow_up_input("steer me".to_string())
        .expect("intent should be accepted");

    assert_eq!(runtime.run_until_idle().as_deref(), Some("steer me"));
    let message = runtime.state.messages.last().expect("tagged message should exist");
    let intent_id = message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.intent_id())
        .expect("follow-up message should carry intent identity")
        .to_string();
    assert_eq!(
        message.metadata.as_ref().and_then(|metadata| metadata.source()),
        Some("user_input"),
        "ordinary steering input must remain attributed to the user"
    );
    assert!(runtime.applied_follow_up_intent_ids().is_empty());
    assert_eq!(runtime.steering.pending_follow_up_intents_snapshot().len(), 1);
    runtime.steering.acknowledge_durable_follow_up_intents();
    assert_eq!(runtime.applied_follow_up_intent_ids().back(), Some(&intent_id));
}

#[test]
fn run_until_idle_preserves_synthetic_follow_up_provenance_at_turn_boundary() {
    let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    let mut runtime = AgentRuntime::new(state, None, None);
    runtime
        .try_queue_synthetic_follow_up_input("continue the saved plan".to_string())
        .expect("harness intent should be accepted");

    assert_eq!(runtime.run_until_idle().as_deref(), Some("continue the saved plan"));

    let message = runtime.state.messages.last().expect("turn-boundary message should be present");
    let metadata = message.metadata.as_ref().expect("turn-boundary message should have metadata");
    assert_eq!(metadata.source(), Some("synthetic"));
    assert!(metadata.is_synthetic());
    assert!(metadata.intent_id().is_some(), "synthetic message must retain its recovery identity");

    let pending = runtime.steering.pending_follow_up_intents_snapshot();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].provenance(), FollowUpProvenance::Synthetic);
}

#[test]
fn follow_up_provenance_round_trips_and_legacy_intents_default_to_user_input() {
    let synthetic =
        QueuedFollowUpIntent::new_with_provenance("auto continue".to_string(), FollowUpProvenance::Synthetic);
    let encoded = serde_json::to_value(&synthetic).expect("intent should serialize");
    assert_eq!(encoded["provenance"], "synthetic");
    let restored: QueuedFollowUpIntent = serde_json::from_value(encoded).expect("intent should deserialize");
    assert_eq!(restored.provenance(), FollowUpProvenance::Synthetic);

    let legacy = serde_json::json!({"id": "legacy", "text": "continue"});
    let restored_legacy: QueuedFollowUpIntent =
        serde_json::from_value(legacy).expect("legacy intent without provenance should deserialize");
    assert_eq!(restored_legacy.provenance(), FollowUpProvenance::UserInput);
}

#[test]
fn recovery_replays_only_unrepresented_intent_ids() {
    let pending = QueuedFollowUpIntent::from_parts_with_provenance(
        "intent-pending",
        "finish the request",
        FollowUpProvenance::Synthetic,
    );
    let same_text_different_id = QueuedFollowUpIntent::from_parts("intent-distinct", pending.text());

    let mut interrupted = AgentRuntime::new(AgentSessionState::new("session".to_string(), 16, 4, 128_000), None, None);
    interrupted
        .restore_follow_up_state([pending.clone(), same_text_different_id.clone()], Vec::new())
        .expect("unwritten intents should be recoverable");
    assert_eq!(interrupted.steering.pending_follow_up_intents().len(), 2);
    assert_eq!(interrupted.steering.pending_follow_up_intents()[0], pending);
    assert_eq!(interrupted.steering.pending_follow_up_intents()[1], same_text_different_id);
    interrupted
        .restore_follow_up_state([pending.clone(), pending.clone(), same_text_different_id.clone()], Vec::new())
        .expect("repeated restore should not duplicate queued IDs");
    assert_eq!(interrupted.steering.pending_follow_up_intents().len(), 2);

    let mut duplicate_envelope = AgentRuntime::new(AgentSessionState::new("other".into(), 16, 4, 128_000), None, None);
    duplicate_envelope
        .restore_follow_up_state([pending.clone(), pending.clone(), same_text_different_id.clone()], Vec::new())
        .unwrap();
    assert_eq!(duplicate_envelope.steering.pending_follow_up_intents().len(), 2);

    let mut acknowledged = AgentRuntime::new(AgentSessionState::new("acknowledged".into(), 16, 4, 128_000), None, None);
    acknowledged.steering.acknowledge_follow_up_intent(pending.id());
    acknowledged
        .restore_follow_up_state([pending.clone(), same_text_different_id.clone()], Vec::new())
        .unwrap();
    assert_eq!(acknowledged.steering.pending_follow_up_intents().len(), 1);
    assert_eq!(acknowledged.steering.pending_follow_up_intents()[0].id(), "intent-distinct");

    let mut restarted_state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    restarted_state.add_user_message_with_intent(pending.text().to_string(), Some(pending.id().to_string()));
    let mut restarted = AgentRuntime::new(restarted_state, None, None);
    restarted
        .restore_follow_up_state([pending.clone(), same_text_different_id], Vec::new())
        .expect("distinct pending intent should fit the recovery queue");

    assert_eq!(restarted.steering.pending_follow_up_intents().len(), 1);
    assert_eq!(restarted.steering.pending_follow_up_intents()[0].id(), "intent-distinct");
    assert_eq!(restarted.steering.pending_follow_up_intents()[0].provenance(), FollowUpProvenance::UserInput);
}

#[test]
fn no_archive_release_reuses_follow_up_capacity_without_applied_ids() {
    let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    let mut runtime = AgentRuntime::new(state, None, None);
    let total_capacity = MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS;
    for index in 0..total_capacity {
        runtime
            .try_queue_follow_up_input(format!("input-{index}"))
            .expect("intent should be accepted");
    }

    for _ in 0..total_capacity {
        runtime.run_until_idle().expect("queued intent should become a user message");
    }
    // In-flight holds every accepted intent, so the primary FIFO is full
    // and a new intent parks in overflow instead of failing.
    assert!(runtime.try_queue_follow_up_input("parked-0".to_string()).is_ok());
    for index in 1..MAX_OVERFLOW_FOLLOW_UP_INTENTS {
        assert!(runtime.try_queue_follow_up_input(format!("parked-{index}")).is_ok());
    }
    assert!(runtime.try_queue_follow_up_input("rejected".to_string()).is_err());

    runtime.steering.release_in_flight_follow_up_intents_without_persistence();
    assert!(runtime.try_queue_follow_up_input("after-release".to_string()).is_ok());
    assert!(runtime.applied_follow_up_intent_ids().is_empty());
}

#[tokio::test]
async fn paused_runtime_resumes_and_preserves_follow_up_inputs() {
    let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = AgentRuntime::new(state, None, Some(receiver));

    sender.send(SteeringMessage::Pause).expect("pause should send");
    sender
        .send(SteeringMessage::FollowUpInput("queued while paused".to_string()))
        .expect("follow-up should send");
    sender.send(SteeringMessage::Resume).expect("resume should send");

    assert_eq!(runtime.poll_turn_control().await, RuntimeControl::Resumed);
    assert!(runtime.has_pending_follow_up_inputs());
    assert_eq!(runtime.run_until_idle().as_deref(), Some("queued while paused"));
}

#[tokio::test]
async fn paused_runtime_stop_request_wins() {
    let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = AgentRuntime::new(state, None, Some(receiver));

    sender.send(SteeringMessage::Pause).expect("pause should send");
    sender.send(SteeringMessage::SteerStop).expect("stop should send");

    assert_eq!(runtime.poll_turn_control().await, RuntimeControl::StopRequested);
    assert!(!runtime.has_pending_follow_up_inputs());
}

#[tokio::test]
async fn run_turn_once_uses_completed_payload_when_no_deltas_exist() {
    let response = LLMResponse {
        content: Some("### Header\n- item".to_string()),
        model: "test-model".to_string(),
        finish_reason: FinishReason::Stop,
        reasoning: Some("**why** this works".to_string()),
        reasoning_details: Some(vec!["continuation detail".to_string()]),
        ..Default::default()
    };
    let provider = CompletedOnlyStreamProvider { response: response.clone() };
    let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
    let mut runtime = AgentRuntime::new(state, None, None);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
    let request = LLMRequest {
        model: "test-model".to_string(),
        ..Default::default()
    };

    let turn = runtime
        .run_turn_once(&mut provider_box, request, None)
        .await
        .expect("run_turn_once should succeed");

    assert_eq!(turn.content, "### Header\n- item");
    assert_eq!(turn.reasoning, None);
    assert_eq!(turn.response.content.as_deref(), Some("### Header\n- item"));
    assert_eq!(turn.response.reasoning.as_deref(), Some("**why** this works"));
    assert_eq!(turn.response.reasoning_details.as_deref(), Some(["continuation detail".to_string()].as_slice()));
}

#[tokio::test]
async fn provider_runtime_model_adapter_emits_delta_progress() {
    let response = LLMResponse {
        content: Some("hello world".to_string()),
        model: "test-model".to_string(),
        finish_reason: FinishReason::Stop,
        reasoning: Some("trace".to_string()),
        ..Default::default()
    };
    let provider = DeltaStreamProvider {
        response,
        text_delta: "hello world".to_string(),
        reasoning_delta: "trace".to_string(),
        reasoning_source: ReasoningSource::ProviderSummary,
        tool_call_start: false,
    };
    let mut steering = RuntimeSteering::default();
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
    let request = LLMRequest {
        model: "test-model".to_string(),
        ..Default::default()
    };

    let mut adapter = ProviderRuntimeModelAdapter::new(&mut provider_box, &mut steering);
    let mut seen_progress = Vec::new();
    let mut callback = |event| seen_progress.push(event);
    let output = adapter
        .execute(request, None, &mut callback)
        .await
        .expect("adapter execution should succeed");

    assert_eq!(output.response.content.as_deref(), Some("hello world"));
    assert_eq!(output.response.reasoning.as_deref(), Some("trace"));
    assert_eq!(
        seen_progress,
        vec![
            RuntimeModelProgress::ReasoningDelta("trace".to_string()),
            RuntimeModelProgress::OutputDelta("hello world".to_string()),
        ]
    );
}

#[tokio::test]
async fn adapter_hides_unclassified_reasoning_but_keeps_text_and_metadata() {
    let response = LLMResponse {
        content: Some("hello".to_string()),
        model: "test-model".to_string(),
        finish_reason: FinishReason::Stop,
        reasoning: Some("private trace".to_string()),
        reasoning_details: Some(vec!["continuation detail".to_string()]),
        ..Default::default()
    };
    let provider = DeltaStreamProvider {
        response,
        text_delta: "hello".to_string(),
        reasoning_delta: "private trace".to_string(),
        reasoning_source: ReasoningSource::Unknown,
        tool_call_start: false,
    };
    let mut steering = RuntimeSteering::default();
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
    let mut seen_progress = Vec::new();
    let mut callback = |event| seen_progress.push(event);
    let output = ProviderRuntimeModelAdapter::new(&mut provider_box, &mut steering)
        .execute(LLMRequest::default(), None, &mut callback)
        .await
        .expect("adapter execution should succeed");

    assert_eq!(output.response.reasoning.as_deref(), Some("private trace"));
    assert_eq!(output.response.reasoning_details.as_deref(), Some(["continuation detail".to_string()].as_slice()));
    assert_eq!(
        seen_progress,
        vec![
            RuntimeModelProgress::ProviderActivity,
            RuntimeModelProgress::OutputDelta("hello".to_string())
        ]
    );
    assert!(
        !seen_progress
            .iter()
            .any(|event| matches!(event, RuntimeModelProgress::ToolCallStarted { .. }))
    );
}

#[tokio::test]
async fn adapter_emits_structured_tool_call_status_without_textual_trigger() {
    let provider = DeltaStreamProvider {
        response: LLMResponse {
            model: "test-model".to_string(),
            finish_reason: FinishReason::ToolCalls,
            ..Default::default()
        },
        text_delta: String::new(),
        reasoning_delta: String::new(),
        reasoning_source: ReasoningSource::Unknown,
        tool_call_start: true,
    };
    let mut steering = RuntimeSteering::default();
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);
    let mut seen_progress = Vec::new();
    let mut callback = |event| seen_progress.push(event);
    ProviderRuntimeModelAdapter::new(&mut provider_box, &mut steering)
        .execute(LLMRequest::default(), None, &mut callback)
        .await
        .expect("adapter execution should succeed");

    assert!(seen_progress.iter().any(|event| matches!(
        event,
        RuntimeModelProgress::ToolCallStarted { call_id, name }
            if call_id == "call_test" && name.as_deref() == Some("read_file")
    )));
}

#[test]
fn streaming_lifecycle_bridge_tracks_tool_call_item_ids() {
    let mut bridge = StreamingLifecycleBridge::new(None, "turn_tool_map", 5, 2);
    bridge.on_progress(RuntimeModelProgress::ToolCallStarted {
        call_id: "call_42".to_string(),
        name: Some("shell".to_string()),
    });

    let item_ids = bridge.take_streamed_tool_call_items();
    assert_eq!(
        item_ids.get("call_42").map(|(item_id, _)| item_id.as_str()),
        Some("turn_tool_map-step-5-assistant-stream-2-tool-call-call_42")
    );
    assert_eq!(item_ids.get("call_42").map(|(_, name)| name.as_str()), Some("shell"));
}

#[test]
fn update_throttle_first_update_is_eager() {
    let mut throttle = UpdateThrottle::new();
    assert!(throttle.is_first());
    // No new bytes, but eager (first update) should still emit.
    assert!(throttle.should_emit(0, throttle.is_first(), 1024, 64));
    throttle.record(0);
    assert!(!throttle.is_first());
}

#[test]
fn update_throttle_requires_min_bytes_after_first() {
    let mut throttle = UpdateThrottle::new();
    throttle.record(0);
    // 500 bytes is below the 1024 threshold and not eager -> no emit.
    assert!(!throttle.should_emit(500, false, 1024, 64));
    // 1024 bytes meets the threshold.
    assert!(throttle.should_emit(1024, false, 1024, 64));
}

#[test]
fn update_throttle_stops_at_max_events() {
    let mut throttle = UpdateThrottle::new();
    for _ in 0..2 {
        assert!(throttle.should_emit(0, true, 256, 2));
        throttle.record(0);
    }
    // Cap reached; even an eager signal cannot emit.
    assert!(!throttle.should_emit(0, true, 256, 2));
}

#[test]
fn update_throttle_advance_to_does_not_count_event() {
    let mut throttle = UpdateThrottle::new();
    throttle.advance_to(100);
    assert!(throttle.is_first());
    // last_emit_len is now 100; 100 new bytes is below 256 threshold.
    assert!(!throttle.should_emit(200, false, 256, 2));
    // 256 new bytes past 100 meets the threshold.
    assert!(throttle.should_emit(356, false, 256, 2));
}

#[test]
fn update_throttle_reset_restores_initial_state() {
    let mut throttle = UpdateThrottle::new();
    throttle.record(500);
    assert!(!throttle.is_first());
    throttle.reset();
    assert!(throttle.is_first());
    assert!(throttle.should_emit(0, true, 1024, 64));
}
