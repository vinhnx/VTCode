use crate::core::agent::events::{EventSink, SharedLifecycleEmitter};
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::steering::{
    FollowUpProvenance, FollowUpQueueFull, MAX_APPLIED_FOLLOW_UP_INTENT_IDS, MAX_OVERFLOW_FOLLOW_UP_INTENTS,
    MAX_QUEUED_FOLLOW_UP_INTENTS, QueuedFollowUpIntent, SteeringMessage,
};
use crate::exec::events::{ThreadEvent, ToolCallStatus, ToolOutcome};
use crate::llm::provider::{
    AssistantPhase, FinishReason, LLMProvider, LLMRequest, LLMResponse, NormalizedStreamEvent, ToolCall,
    Usage as ProviderUsage,
};
use crate::llm::providers::gemini::wire::{Content, Part};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::error::TryRecvError;

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
    accumulated.clear();
    accumulated.push_str(completed_text);
}

mod steering;
pub use steering::RuntimeSteering;

/// Control signal returned when polling the steering channel between turns or tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeControl {
    /// No steering message pending; execution may proceed normally.
    Continue,
    /// Execution was paused and has now been resumed.
    Resumed,
    /// A stop request was received; the current turn should be cancelled.
    StopRequested,
}

/// Progress event emitted by the model adapter during a streaming LLM response.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeModelProgress {
    /// A chunk of assistant output text.
    OutputDelta(String),
    /// A chunk of reasoning/thinking text.
    ReasoningDelta(String),
    /// The active reasoning stage label changed.
    ReasoningStage(String),
    /// The provider produced progress that is intentionally not user-visible.
    ProviderActivity,
    /// A new tool call began streaming.
    ToolCallStarted {
        /// Identifier of the tool call.
        call_id: String,
        /// Optional name of the tool being invoked.
        name: Option<String>,
    },
    /// A chunk of arguments for an in-progress tool call.
    ToolCallDelta {
        /// Identifier of the tool call receiving the delta.
        call_id: String,
        /// Serialized argument fragment.
        delta: String,
    },
}

#[derive(Debug, Clone)]
struct RuntimeModelOutput {
    response: LLMResponse,
}

#[async_trait]
trait RuntimeModelAdapter {
    async fn execute(
        &mut self,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
        on_progress: &mut (dyn FnMut(RuntimeModelProgress) + Send),
    ) -> Result<RuntimeModelOutput>;
}

struct ProviderRuntimeModelAdapter<'a> {
    provider: &'a mut Box<dyn LLMProvider>,
    steering: &'a mut RuntimeSteering,
}

impl<'a> ProviderRuntimeModelAdapter<'a> {
    fn new(provider: &'a mut Box<dyn LLMProvider>, steering: &'a mut RuntimeSteering) -> Self {
        Self { provider, steering }
    }
}

#[async_trait]
impl RuntimeModelAdapter for ProviderRuntimeModelAdapter<'_> {
    async fn execute(
        &mut self,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
        on_progress: &mut (dyn FnMut(RuntimeModelProgress) + Send),
    ) -> Result<RuntimeModelOutput> {
        let started_at = Instant::now();
        let request_model = request.model.clone();
        let mut stream = if let Some(duration) = timeout {
            match tokio::time::timeout(duration, self.provider.stream_normalized(request)).await {
                Ok(result) => result?,
                Err(_) => {
                    tracing::warn!(model = %request_model, elapsed_ms = started_at.elapsed().as_millis() as u64, "model stream timed out");
                    return Err(anyhow::anyhow!("Stream request timed out after {duration:?}"));
                }
            }
        } else {
            self.provider.stream_normalized(request).await?
        };

        let mut final_usage = ProviderUsage::default();
        let mut completed_response: Option<LLMResponse> = None;
        let mut provider_activity_reported = false;
        while let Some(event_result) = stream.next().await {
            if matches!(self.steering.poll_turn_control().await, RuntimeControl::StopRequested) {
                tracing::info!(model = %request_model, elapsed_ms = started_at.elapsed().as_millis() as u64, "model stream cancelled");
                let mut response = LLMResponse {
                    model: request_model.clone(),
                    finish_reason: FinishReason::Error("Cancelled".to_string()),
                    usage: Some(final_usage.clone()),
                    ..Default::default()
                };
                if response.usage.as_ref().is_some_and(|usage| {
                    usage.prompt_tokens == 0 && usage.completion_tokens == 0 && usage.total_tokens == 0
                }) {
                    response.usage = None;
                }
                return Ok(RuntimeModelOutput { response });
            }

            match event_result? {
                NormalizedStreamEvent::TextDelta { delta } => {
                    on_progress(RuntimeModelProgress::OutputDelta(delta));
                }
                NormalizedStreamEvent::ReasoningDelta { delta, source } if source.is_public_summary() => {
                    on_progress(RuntimeModelProgress::ReasoningDelta(delta));
                }
                NormalizedStreamEvent::ReasoningDelta { .. } => {
                    if !provider_activity_reported {
                        provider_activity_reported = true;
                        on_progress(RuntimeModelProgress::ProviderActivity);
                    }
                }
                NormalizedStreamEvent::ReasoningStage { stage } => {
                    on_progress(RuntimeModelProgress::ReasoningStage(stage));
                }
                NormalizedStreamEvent::ToolCallStart { call_id, name } => {
                    on_progress(RuntimeModelProgress::ToolCallStarted { call_id, name });
                }
                NormalizedStreamEvent::ToolCallDelta { call_id, delta } => {
                    on_progress(RuntimeModelProgress::ToolCallDelta { call_id, delta });
                }
                NormalizedStreamEvent::Usage { usage } => {
                    final_usage = usage;
                }
                NormalizedStreamEvent::Done { response } => {
                    let mut response = *response;
                    if response.usage.is_none()
                        && (final_usage.prompt_tokens > 0
                            || final_usage.completion_tokens > 0
                            || final_usage.total_tokens > 0)
                    {
                        response.usage = Some(final_usage.clone());
                    }
                    completed_response = Some(response);
                    break;
                }
            }
        }

        let mut response = completed_response.unwrap_or_default();
        if response.model.is_empty() {
            response.model = request_model;
        }
        if response.usage.is_none()
            && (final_usage.prompt_tokens > 0 || final_usage.completion_tokens > 0 || final_usage.total_tokens > 0)
        {
            response.usage = Some(final_usage);
        }

        tracing::debug!(
            model = %response.model,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            input_tokens = response.usage.as_ref().map_or(0, |usage| usage.prompt_tokens),
            output_tokens = response.usage.as_ref().map_or(0, |usage| usage.completion_tokens),
            finish_reason = ?response.finish_reason,
            "model stream completed"
        );
        Ok(RuntimeModelOutput { response })
    }
}

/// Result of executing a single LLM turn, containing the full response and extracted fields.
pub struct TurnExecution {
    /// The complete LLM response including usage and finish reason.
    pub response: LLMResponse,
    /// The accumulated assistant output text.
    pub content: String,
    /// The accumulated reasoning/thinking text, if present.
    pub reasoning: Option<String>,
}

const MIN_REASONING_UPDATE_BYTES: usize = 256;
const MAX_REASONING_UPDATE_EVENTS: usize = 2;
const MIN_OUTPUT_UPDATE_BYTES: usize = 1024;
const MAX_OUTPUT_UPDATE_EVENTS: usize = 64;

/// Bounded update throttle shared by the streaming lifecycle paths.
///
/// Limits how many intermediate snapshot events are emitted for a single
/// streaming text/reasoning buffer within one turn:
///
/// - emission stops once `max_events` snapshots have been produced;
/// - an update is emitted eagerly when the caller's `eager` signal is true
///   (e.g. the first update, or a reasoning stage change);
/// - otherwise an update requires at least `min_bytes` of new content since
///   the last emitted snapshot.
///
/// The throttle owns only the accounting; the authoritative accumulated text
/// lives on the lifecycle emitter, which always emits a final lossless
/// snapshot on completion. This keeps live and replay streams consistent.
#[derive(Debug, Clone, Default)]
struct UpdateThrottle {
    events: usize,
    last_emit_len: usize,
}

impl UpdateThrottle {
    const fn new() -> Self {
        Self { events: 0, last_emit_len: 0 }
    }

    /// Whether an intermediate snapshot should be emitted now.
    fn should_emit(&self, current_len: usize, eager: bool, min_bytes: usize, max_events: usize) -> bool {
        if self.events >= max_events {
            return false;
        }
        eager || current_len.saturating_sub(self.last_emit_len) >= min_bytes
    }

    /// Record that a snapshot was emitted at `current_len`.
    fn record(&mut self, current_len: usize) {
        self.events += 1;
        self.last_emit_len = current_len;
    }

    /// Advance the last-emitted length marker without counting an event.
    /// Used when a snapshot is emitted outside the throttle's accounting
    /// (e.g. the first reasoning snapshot before reasoning has "started").
    fn advance_to(&mut self, current_len: usize) {
        self.last_emit_len = current_len;
    }

    /// Whether no snapshot has been emitted yet — the first-update eager signal.
    fn is_first(&self) -> bool {
        self.events == 0
    }

    /// Reset the throttle for a new turn.
    fn reset(&mut self) {
        self.events = 0;
        self.last_emit_len = 0;
    }
}

/// Bridges streaming model progress events to the lifecycle event sink for real-time UI updates.
#[doc(hidden)]
pub struct StreamingLifecycleBridge {
    event_sink: Option<EventSink>,
    assistant_item_id: String,
    reasoning_item_id: String,
    lifecycle: SharedLifecycleEmitter,
    /// Streamed tool calls keyed by provider call id, valued as
    /// `(harness item id, tool name)` so turn teardown can close items whose
    /// dispatch never happened with their real tool label.
    tool_call_item_ids: hashbrown::HashMap<String, (String, String)>,
    reasoning_stage: Option<String>,
    output: UpdateThrottle,
    reasoning: UpdateThrottle,
}

impl StreamingLifecycleBridge {
    /// Create a new bridge that emits lifecycle events to the provided sink.
    #[must_use]
    pub fn new(event_sink: Option<EventSink>, turn_id: &str, step: usize, attempt: usize) -> Self {
        Self {
            event_sink,
            assistant_item_id: format!("{turn_id}-step-{step}-assistant-stream-{attempt}"),
            reasoning_item_id: format!("{turn_id}-step-{step}-reasoning-stream-{attempt}"),
            lifecycle: SharedLifecycleEmitter::default(),
            tool_call_item_ids: hashbrown::HashMap::new(),
            reasoning_stage: None,
            output: UpdateThrottle::new(),
            reasoning: UpdateThrottle::new(),
        }
    }

    /// Forward a streaming progress event to the lifecycle emitter and sink.
    pub fn on_progress(&mut self, event: RuntimeModelProgress) {
        match event {
            RuntimeModelProgress::OutputDelta(delta) => self.push_assistant_delta(&delta),
            RuntimeModelProgress::ReasoningDelta(delta) => self.push_reasoning_delta(&delta),
            RuntimeModelProgress::ReasoningStage(stage) => self.update_reasoning_stage(stage),
            RuntimeModelProgress::ProviderActivity => {}
            RuntimeModelProgress::ToolCallStarted { call_id, name } => {
                self.start_tool_call(call_id, name);
            }
            RuntimeModelProgress::ToolCallDelta { call_id, delta } => {
                self.push_tool_call_delta(call_id, &delta);
            }
        }
    }

    /// Abort the streaming turn, marking all open items as failed and flushing events.
    pub fn abort(&mut self) {
        self.lifecycle.complete_open_text_items();
        self.lifecycle.complete_open_tool_calls_with_status(ToolCallStatus::Failed);
        self.emit_pending_events();
    }

    /// Complete all open text items gracefully and flush pending events.
    pub fn complete_open_items(&mut self) {
        // Flush the authoritative accumulated tool-call arguments before
        // closing text items so the UI sees the full streamed arguments even
        // when intermediate deltas were throttled (small tool calls).
        self.lifecycle.flush_open_tool_call_arguments();
        self.lifecycle.complete_open_text_items();
        self.emit_pending_events();
    }

    /// Take the mapping of tool call IDs to their `(lifecycle item id, tool
    /// name)`, leaving the map empty.
    #[must_use]
    pub fn take_streamed_tool_call_items(&mut self) -> hashbrown::HashMap<String, (String, String)> {
        std::mem::take(&mut self.tool_call_item_ids)
    }

    fn push_assistant_delta(&mut self, delta: &str) {
        if !self.lifecycle.append_assistant_delta(delta) {
            return;
        }

        let len = self.lifecycle.assistant_len();
        if self
            .output
            .should_emit(len, self.output.is_first(), MIN_OUTPUT_UPDATE_BYTES, MAX_OUTPUT_UPDATE_EVENTS)
            && self.lifecycle.emit_assistant_snapshot(Some(self.assistant_item_id.clone()))
        {
            self.output.record(len);
            self.emit_pending_events();
        }
    }

    fn push_reasoning_delta(&mut self, delta: &str) {
        if !self.lifecycle.append_reasoning_delta(delta) {
            return;
        }

        if !self.lifecycle.reasoning_started() {
            if self.lifecycle.emit_reasoning_snapshot(Some(self.reasoning_item_id.clone())) {
                self.reasoning.advance_to(self.lifecycle.reasoning_len());
                self.emit_pending_events();
            }
            return;
        }

        let len = self.lifecycle.reasoning_len();
        if self
            .reasoning
            .should_emit(len, false, MIN_REASONING_UPDATE_BYTES, MAX_REASONING_UPDATE_EVENTS)
            && self.lifecycle.emit_reasoning_snapshot(Some(self.reasoning_item_id.clone()))
        {
            self.reasoning.record(len);
            self.emit_pending_events();
        }
    }

    fn update_reasoning_stage(&mut self, stage: String) {
        let stage_changed = self.reasoning_stage.as_deref() != Some(stage.as_str());
        if !stage_changed || !self.lifecycle.set_reasoning_stage(Some(stage.clone())) {
            self.reasoning_stage = Some(stage);
            return;
        }
        self.reasoning_stage = Some(stage);

        if !self.lifecycle.reasoning_started() {
            return;
        }

        let len = self.lifecycle.reasoning_len();
        if self
            .reasoning
            .should_emit(len, true, MIN_REASONING_UPDATE_BYTES, MAX_REASONING_UPDATE_EVENTS)
            && self.lifecycle.emit_reasoning_stage_update()
        {
            self.reasoning.record(len);
            self.emit_pending_events();
        }
    }

    fn start_tool_call(&mut self, call_id: String, name: Option<String>) {
        let item_id = format!("{}-tool-call-{call_id}", self.assistant_item_id);
        let tool_name = name.clone().unwrap_or_default();
        self.tool_call_item_ids.insert(call_id.clone(), (item_id.clone(), tool_name));
        let _ = self.lifecycle.start_tool_call(&call_id, name, Some(item_id));
        self.emit_pending_events();
    }

    fn push_tool_call_delta(&mut self, call_id: String, delta: &str) {
        if !self.lifecycle.append_tool_call_delta(
            &call_id,
            delta,
            None,
            Some(format!("{}-tool-call-{call_id}", self.assistant_item_id)),
        ) {
            return;
        }
        self.emit_pending_events();
    }

    fn emit_pending_events(&mut self) {
        let Some(sink) = &self.event_sink else {
            let _ = self.lifecycle.drain_events();
            return;
        };

        for event in self.lifecycle.drain_events() {
            let mut callback = sink.lock();
            callback(&event);
        }
    }
}

/// Orchestrates a single agent turn: manages steering, lifecycle events, and the LLM call.
pub struct AgentRuntime {
    /// Mutable session state including conversation history and statistics.
    pub state: AgentSessionState,
    steering: RuntimeSteering,
    event_sink: Option<EventSink>,
    lifecycle: SharedLifecycleEmitter,
    emitted_events: Vec<ThreadEvent>,
    output: UpdateThrottle,
    reasoning: UpdateThrottle,
}

impl AgentRuntime {
    /// Create a new runtime with the given session state, optional event sink, and steering receiver.
    pub fn new(
        state: AgentSessionState,
        event_sink: Option<EventSink>,
        steering_receiver: Option<UnboundedReceiver<SteeringMessage>>,
    ) -> Self {
        Self {
            state,
            steering: RuntimeSteering::new(steering_receiver),
            event_sink,
            lifecycle: SharedLifecycleEmitter::default(),
            emitted_events: Vec::new(),
            output: UpdateThrottle::new(),
            reasoning: UpdateThrottle::new(),
        }
    }

    /// Replace the structured event sink used for lifecycle event emission.
    pub fn set_event_handler(&mut self, sink: Option<EventSink>) {
        self.event_sink = sink;
    }

    /// Replace the steering message receiver channel for this runtime.
    pub fn set_steering_receiver(&mut self, receiver: Option<UnboundedReceiver<SteeringMessage>>) {
        self.steering.set_receiver(receiver);
    }

    /// Take ownership of the steering receiver, leaving `None` in its place.
    pub fn take_steering_receiver(&mut self) -> Option<UnboundedReceiver<SteeringMessage>> {
        self.steering.take_receiver()
    }

    /// Borrow the session state and steering controller mutably at the same time.
    pub fn split_mut(&mut self) -> (&mut AgentSessionState, &mut RuntimeSteering) {
        (&mut self.state, &mut self.steering)
    }

    /// Returns `true` if there are queued follow-up user inputs waiting to be consumed.
    #[must_use]
    pub fn has_pending_follow_up_inputs(&self) -> bool {
        self.steering.has_pending_follow_up_inputs()
    }

    /// Pop the next queued follow-up input, add a tagged user message, and
    /// retain the intent until the enclosing history checkpoint succeeds.
    pub fn run_until_idle(&mut self) -> Option<String> {
        let intent = self.steering.pop_follow_up_intent()?;
        let (intent_id, input, provenance) = intent.into_parts_with_provenance();
        match provenance {
            FollowUpProvenance::UserInput => {
                self.state.add_user_message_with_intent(input.clone(), Some(intent_id.clone()));
            }
            FollowUpProvenance::Synthetic => {
                self.state
                    .add_synthetic_user_message_with_intent(input.clone(), Some(intent_id.clone()));
            }
        }
        self.steering
            .in_flight_follow_up_intents
            .push_back(QueuedFollowUpIntent::from_parts_with_provenance(intent_id, input.clone(), provenance));
        Some(input)
    }

    /// Queue a follow-up input to be consumed by `run_until_idle` on the next
    /// iteration, causing the session loop to start a new turn with this input
    /// without waiting for the user to type anything.
    pub fn queue_follow_up_input(&mut self, input: String) {
        let _ = self.steering.queue_follow_up_input(input);
    }

    /// Queue a follow-up input while exposing FIFO overflow to the caller.
    pub fn try_queue_follow_up_input(&mut self, input: String) -> Result<(), FollowUpQueueFull> {
        self.steering.try_queue_follow_up_input(input)
    }

    /// Queue a harness-generated continuation or recovery instruction.
    pub fn try_queue_synthetic_follow_up_input(&mut self, input: String) -> Result<(), FollowUpQueueFull> {
        self.steering.try_queue_synthetic_follow_up_input(input)
    }

    /// Restore the durable steering snapshot after loading session history.
    /// Pending intents already represented by a tagged user message, or
    /// already in the applied-ID window, are skipped; identical text with a
    /// different ID is intentionally retained.
    pub fn restore_follow_up_state(
        &mut self,
        pending_intents: impl IntoIterator<Item = QueuedFollowUpIntent>,
        applied_intent_ids: impl IntoIterator<Item = String>,
    ) -> Result<(), FollowUpQueueFull> {
        let mut applied = HashSet::new();
        for intent_id in applied_intent_ids {
            if applied.insert(intent_id.clone()) {
                self.steering.acknowledge_follow_up_intent(intent_id);
            }
        }
        let represented: HashSet<String> = self
            .state
            .messages
            .iter()
            .filter_map(|message| message.metadata.as_ref().and_then(|metadata| metadata.intent_id()))
            .map(ToOwned::to_owned)
            .collect();

        for intent in pending_intents {
            if applied.contains(intent.id()) || represented.contains(intent.id()) {
                continue;
            }
            self.steering.try_queue_follow_up_intent(intent)?;
        }
        Ok(())
    }

    /// Access the intent IDs acknowledged by this runtime for checkpointing.
    #[must_use]
    pub fn applied_follow_up_intent_ids(&self) -> &VecDeque<String> {
        self.steering.applied_follow_up_intent_ids()
    }

    /// Drop follow-up inputs belonging to the discarded execution context.
    pub fn clear_pending_follow_up_inputs(&mut self) {
        self.steering.clear_pending_follow_up_inputs();
    }

    /// Poll the steering channel for control signals during the current turn.
    pub async fn poll_turn_control(&mut self) -> RuntimeControl {
        self.steering.poll_turn_control().await
    }

    /// Poll the steering channel for control signals during tool execution.
    pub async fn poll_tool_control(&mut self) -> RuntimeControl {
        self.steering.poll_tool_control().await
    }

    /// Take all emitted lifecycle events, leaving the internal buffer empty.
    pub fn take_emitted_events(&mut self) -> Vec<ThreadEvent> {
        std::mem::take(&mut self.emitted_events)
    }

    /// Look up the lifecycle item ID for a given tool call identifier.
    #[must_use]
    pub fn tool_call_item_id(&self, call_id: &str) -> Option<String> {
        self.lifecycle.tool_call_item_id(call_id).map(str::to_string)
    }

    /// Mark a specific tool call as completed with the given status and emit lifecycle events.
    pub fn complete_tool_call(&mut self, call_id: &str, status: ToolCallStatus, outcome: Option<ToolOutcome>) {
        let _ = self.lifecycle.complete_tool_call(call_id, status, outcome);
        self.emit_pending_lifecycle_events();
    }

    /// Mark all still-open tool calls as completed with the given status and emit lifecycle events.
    pub fn complete_open_tool_calls(&mut self, status: ToolCallStatus) {
        self.lifecycle.complete_open_tool_calls_with_status(status);
        self.emit_pending_lifecycle_events();
    }

    fn emit_event(&mut self, event: ThreadEvent) {
        self.emitted_events.push(event.clone());
        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
            callback(&event);
        }
    }

    fn emit_pending_lifecycle_events(&mut self) {
        for event in self.lifecycle.drain_events() {
            self.emit_event(event);
        }
    }

    fn finalize_assistant_lifecycle(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let should_emit_snapshot = !self.lifecycle.assistant_started() || self.lifecycle.replace_assistant_text(text);
        if should_emit_snapshot {
            let _ = self.lifecycle.emit_assistant_snapshot(None);
        }
        let _ = self.lifecycle.complete_assistant_stream();
    }

    fn finalize_reasoning_lifecycle(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let should_emit_snapshot = !self.lifecycle.reasoning_started() || self.lifecycle.replace_reasoning_text(text);
        if should_emit_snapshot {
            let _ = self.lifecycle.emit_reasoning_snapshot(None);
        }
        let _ = self.lifecycle.complete_reasoning_stream();
    }

    fn finalize_tool_call_lifecycle(&mut self, tool_calls: Option<&[ToolCall]>, _finish_reason: &str) {
        if let Some(tool_calls) = tool_calls {
            for call in tool_calls {
                let tool_name = call.function.as_ref().map(|function| function.name.clone());
                let _ = self.lifecycle.start_tool_call(&call.id, tool_name.clone(), None);
                if let Some(function) = call.function.as_ref() {
                    let _ = self
                        .lifecycle
                        .sync_tool_call_arguments(&call.id, &function.arguments, tool_name, None);
                }
            }
            return;
        }

        self.lifecycle.complete_open_tool_calls_with_status(ToolCallStatus::Failed);
    }

    fn record_model_progress(
        &mut self,
        event: RuntimeModelProgress,
        full_text: &mut String,
        full_reasoning: &mut String,
    ) {
        match event {
            RuntimeModelProgress::OutputDelta(delta) => {
                full_text.push_str(&delta);
                if self.lifecycle.append_assistant_delta(&delta) {
                    let len = self.lifecycle.assistant_len();
                    if self.output.should_emit(
                        len,
                        self.output.is_first(),
                        MIN_OUTPUT_UPDATE_BYTES,
                        MAX_OUTPUT_UPDATE_EVENTS,
                    ) && self.lifecycle.emit_assistant_snapshot(None)
                    {
                        self.output.record(len);
                        self.emit_pending_lifecycle_events();
                    }
                }
            }
            RuntimeModelProgress::ReasoningDelta(delta) => {
                full_reasoning.push_str(&delta);
                if self.lifecycle.append_reasoning_delta(&delta) {
                    let len = self.lifecycle.reasoning_len();
                    if self.reasoning.should_emit(
                        len,
                        self.reasoning.is_first(),
                        MIN_REASONING_UPDATE_BYTES,
                        MAX_REASONING_UPDATE_EVENTS,
                    ) && self.lifecycle.emit_reasoning_snapshot(None)
                    {
                        self.reasoning.record(len);
                        self.emit_pending_lifecycle_events();
                    }
                }
            }
            RuntimeModelProgress::ReasoningStage(stage) => {
                if self.lifecycle.set_reasoning_stage(Some(stage)) {
                    let _ = self.lifecycle.emit_reasoning_stage_update();
                    self.emit_pending_lifecycle_events();
                }
            }
            RuntimeModelProgress::ProviderActivity => {}
            RuntimeModelProgress::ToolCallStarted { call_id, name } => {
                let _ = self.lifecycle.start_tool_call(&call_id, name, None);
                self.emit_pending_lifecycle_events();
            }
            RuntimeModelProgress::ToolCallDelta { call_id, delta } => {
                if self.lifecycle.append_tool_call_delta(&call_id, &delta, None, None) {
                    self.emit_pending_lifecycle_events();
                }
            }
        }
    }

    async fn run_turn_once_with_adapter<A: RuntimeModelAdapter + ?Sized>(
        &mut self,
        adapter: &mut A,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
    ) -> Result<TurnExecution> {
        let request_model = request.model.clone();
        let start_time = Instant::now();
        self.output.reset();
        self.reasoning.reset();
        let mut full_text = String::new();
        let mut full_reasoning = String::new();
        let mut on_progress = |event| self.record_model_progress(event, &mut full_text, &mut full_reasoning);
        let RuntimeModelOutput { mut response } = match adapter.execute(request, timeout, &mut on_progress).await {
            Ok(output) => output,
            Err(error) => {
                self.complete_open_tool_calls(ToolCallStatus::Failed);
                tracing::warn!(elapsed_ms = start_time.elapsed().as_millis() as u64, error = %error, "agent turn failed during model execution");
                return Err(error);
            }
        };

        merge_stream_and_completed_text(&mut full_text, response.content.as_deref());
        let provider_reasoning = response.reasoning.clone();

        let finish_reason = match response.finish_reason.clone() {
            FinishReason::Stop => "stop".to_string(),
            FinishReason::ToolCalls => "tool_calls".to_string(),
            FinishReason::Length => "length".to_string(),
            FinishReason::Error(message) => message,
            _ => "unknown".to_string(),
        };
        let final_usage = response.usage.take().unwrap_or_default();
        let mut aggregated_tool_calls = response.tool_calls.take();

        self.finalize_assistant_lifecycle(&full_text);
        self.finalize_reasoning_lifecycle(&full_reasoning);
        self.finalize_tool_call_lifecycle(aggregated_tool_calls.as_deref(), &finish_reason);
        self.emit_pending_lifecycle_events();

        let mut turn_recorded = false;
        self.state.record_turn(&start_time, &mut turn_recorded);

        if final_usage.prompt_tokens > 0 || final_usage.completion_tokens > 0 {
            // `merge_usage` also feeds the session prompt-cache health
            // monitor; a sustained hit-rate collapse warns once per session
            // (SEV-style) instead of silently re-paying full input cost.
            if let Some(alert) = self.state.stats.merge_usage(&final_usage) {
                let message = alert.message();
                tracing::warn!("{message}");
                self.state.push_warning(message);
            }
        }

        aggregated_tool_calls = aggregated_tool_calls.filter(|calls| !calls.is_empty());

        let mut assistant_message = crate::llm::provider::Message::assistant(full_text.clone());
        if let Some(reasoning) = provider_reasoning
            .clone()
            .or_else(|| (!full_reasoning.is_empty()).then(|| full_reasoning.clone()))
        {
            assistant_message = assistant_message.with_reasoning(Some(reasoning));
        }
        if let Some(details) = response.reasoning_details.clone() {
            let values: Vec<serde_json::Value> = details.into_iter().map(serde_json::Value::String).collect();
            assistant_message = assistant_message.with_reasoning_details(Some(values));
        }

        match aggregated_tool_calls.as_ref() {
            Some(calls) => {
                assistant_message = assistant_message
                    .with_tool_calls(calls.clone())
                    .with_phase(Some(AssistantPhase::Commentary));
            }
            None => {
                assistant_message = assistant_message.with_phase(Some(AssistantPhase::FinalAnswer));
            }
        }

        self.state.adjust_token_count(assistant_message.estimate_tokens() as isize);
        self.state.messages_mut().push(assistant_message);

        self.state.conversation.push(Content {
            role: "model".to_string(),
            parts: vec![Part::Text { text: full_text.clone(), thought_signature: None }],
        });
        self.state.last_processed_message_idx = self.state.conversation.len();

        if response.model.is_empty() {
            response.model = request_model;
        }
        response.content = Some(full_text.clone());
        if response.reasoning.is_none() && !full_reasoning.is_empty() {
            response.reasoning = Some(full_reasoning.clone());
        }
        response.tool_calls = aggregated_tool_calls;
        response.usage = Some(final_usage);
        response.finish_reason = if finish_reason == "tool_calls" {
            FinishReason::ToolCalls
        } else if finish_reason == "Cancelled" || finish_reason == "cancelled" {
            FinishReason::Error("Cancelled".to_string())
        } else {
            response.finish_reason
        };

        tracing::debug!(
            elapsed_ms = start_time.elapsed().as_millis() as u64,
            output_bytes = full_text.len(),
            reasoning_bytes = full_reasoning.len(),
            tool_calls = response.tool_calls.as_ref().map_or(0, Vec::len),
            input_tokens = response.usage.as_ref().map_or(0, |usage| usage.prompt_tokens),
            output_tokens = response.usage.as_ref().map_or(0, |usage| usage.completion_tokens),
            finish_reason = %finish_reason,
            "agent turn completed"
        );

        Ok(TurnExecution {
            response,
            content: full_text,
            reasoning: if full_reasoning.is_empty() {
                None
            } else {
                Some(full_reasoning)
            },
        })
    }

    /// Execute a single LLM turn: stream the response, update session state, and return the result.
    pub async fn run_turn_once(
        &mut self,
        provider: &mut Box<dyn LLMProvider>,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
    ) -> Result<TurnExecution> {
        let mut steering = std::mem::take(&mut self.steering);
        let mut adapter = ProviderRuntimeModelAdapter::new(provider, &mut steering);
        let result = self.run_turn_once_with_adapter(&mut adapter, request, timeout).await;
        self.steering = steering;
        result
    }
}

#[cfg(test)]
mod tests;
