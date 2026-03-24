use crate::core::agent::events::{EventSink, SharedLifecycleEmitter};
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::steering::SteeringMessage;
use crate::exec::events::{ThreadEvent, ToolCallStatus};
use crate::llm::provider::{
    AssistantPhase, FinishReason, LLMProvider, LLMRequest, LLMResponse, NormalizedStreamEvent,
    ToolCall, Usage as ProviderUsage,
};
use crate::llm::providers::gemini::wire::{Content, Part};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use std::collections::VecDeque;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeControl {
    Continue,
    Resumed,
    StopRequested,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeModelProgress {
    OutputDelta(String),
    ReasoningDelta(String),
    ReasoningStage(String),
    ToolCallStarted {
        call_id: String,
        name: Option<String>,
    },
    ToolCallDelta {
        call_id: String,
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
        let request_model = request.model.clone();
        let mut stream = if let Some(duration) = timeout {
            match tokio::time::timeout(duration, self.provider.stream_normalized(request)).await {
                Ok(result) => result?,
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "Stream request timed out after {:?}",
                        duration
                    ));
                }
            }
        } else {
            self.provider.stream_normalized(request).await?
        };

        let mut final_usage = ProviderUsage::default();
        let mut completed_response: Option<LLMResponse> = None;
        while let Some(event_result) = stream.next().await {
            if matches!(
                self.steering.poll_turn_control().await,
                RuntimeControl::StopRequested
            ) {
                let mut response = LLMResponse {
                    model: request_model.clone(),
                    finish_reason: FinishReason::Error("Cancelled".to_string()),
                    usage: Some(final_usage.clone()),
                    ..Default::default()
                };
                if response.usage.as_ref().is_some_and(|usage| {
                    usage.prompt_tokens == 0
                        && usage.completion_tokens == 0
                        && usage.total_tokens == 0
                }) {
                    response.usage = None;
                }
                return Ok(RuntimeModelOutput { response });
            }

            match event_result? {
                NormalizedStreamEvent::TextDelta { delta } => {
                    on_progress(RuntimeModelProgress::OutputDelta(delta));
                }
                NormalizedStreamEvent::ReasoningDelta { delta } => {
                    on_progress(RuntimeModelProgress::ReasoningDelta(delta));
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
            && (final_usage.prompt_tokens > 0
                || final_usage.completion_tokens > 0
                || final_usage.total_tokens > 0)
        {
            response.usage = Some(final_usage);
        }

        Ok(RuntimeModelOutput { response })
    }
}

pub struct RuntimeSteering {
    steering_receiver: Option<UnboundedReceiver<SteeringMessage>>,
    queued_follow_up_inputs: VecDeque<String>,
}

impl Default for RuntimeSteering {
    fn default() -> Self {
        Self::new(None)
    }
}

impl RuntimeSteering {
    fn new(steering_receiver: Option<UnboundedReceiver<SteeringMessage>>) -> Self {
        Self {
            steering_receiver,
            queued_follow_up_inputs: VecDeque::new(),
        }
    }

    pub fn set_receiver(&mut self, receiver: Option<UnboundedReceiver<SteeringMessage>>) {
        self.steering_receiver = receiver;
    }

    pub fn take_receiver(&mut self) -> Option<UnboundedReceiver<SteeringMessage>> {
        self.steering_receiver.take()
    }

    #[must_use]
    pub fn has_pending_follow_up_inputs(&self) -> bool {
        !self.queued_follow_up_inputs.is_empty()
    }

    pub fn pop_follow_up_input(&mut self) -> Option<String> {
        self.queued_follow_up_inputs.pop_front()
    }

    pub fn queue_follow_up_input(&mut self, input: String) {
        self.queued_follow_up_inputs.push_back(input);
    }

    pub async fn poll_turn_control(&mut self) -> RuntimeControl {
        self.poll_control().await
    }

    pub async fn poll_tool_control(&mut self) -> RuntimeControl {
        self.poll_control().await
    }

    async fn poll_control(&mut self) -> RuntimeControl {
        let mut paused = false;

        loop {
            let Some(receiver) = self.steering_receiver.as_mut() else {
                return if paused {
                    RuntimeControl::Resumed
                } else {
                    RuntimeControl::Continue
                };
            };

            match receiver.try_recv() {
                Ok(SteeringMessage::SteerStop) => return RuntimeControl::StopRequested,
                Ok(SteeringMessage::Pause) => {
                    paused = true;
                    if matches!(self.wait_for_resume().await, RuntimeControl::StopRequested) {
                        return RuntimeControl::StopRequested;
                    }
                }
                Ok(SteeringMessage::Resume) => {
                    paused = true;
                }
                Ok(SteeringMessage::FollowUpInput(input)) => {
                    self.queued_follow_up_inputs.push_back(input);
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                    return if paused {
                        RuntimeControl::Resumed
                    } else {
                        RuntimeControl::Continue
                    };
                }
            }
        }
    }

    async fn wait_for_resume(&mut self) -> RuntimeControl {
        loop {
            let Some(receiver) = self.steering_receiver.as_mut() else {
                return RuntimeControl::Continue;
            };

            match receiver.recv().await {
                Some(SteeringMessage::Resume) => return RuntimeControl::Continue,
                Some(SteeringMessage::SteerStop) => return RuntimeControl::StopRequested,
                Some(SteeringMessage::FollowUpInput(input)) => {
                    self.queued_follow_up_inputs.push_back(input);
                }
                Some(SteeringMessage::Pause) => {}
                None => return RuntimeControl::Continue,
            }
        }
    }
}

pub struct TurnExecution {
    pub response: LLMResponse,
    pub content: String,
    pub reasoning: Option<String>,
}

const MIN_REASONING_UPDATE_BYTES: usize = 256;
const MAX_REASONING_UPDATE_EVENTS: usize = 2;

#[doc(hidden)]
pub struct StreamingLifecycleBridge {
    event_sink: Option<EventSink>,
    assistant_item_id: String,
    reasoning_item_id: String,
    lifecycle: SharedLifecycleEmitter,
    tool_call_item_ids: hashbrown::HashMap<String, String>,
    reasoning_stage: Option<String>,
    reasoning_update_events: usize,
    last_reasoning_emit_len: usize,
}

impl StreamingLifecycleBridge {
    #[must_use]
    pub fn new(event_sink: Option<EventSink>, turn_id: &str, step: usize, attempt: usize) -> Self {
        Self {
            event_sink,
            assistant_item_id: format!("{turn_id}-step-{step}-assistant-stream-{attempt}"),
            reasoning_item_id: format!("{turn_id}-step-{step}-reasoning-stream-{attempt}"),
            lifecycle: SharedLifecycleEmitter::default(),
            tool_call_item_ids: hashbrown::HashMap::new(),
            reasoning_stage: None,
            reasoning_update_events: 0,
            last_reasoning_emit_len: 0,
        }
    }

    pub fn on_progress(&mut self, event: RuntimeModelProgress) {
        match event {
            RuntimeModelProgress::OutputDelta(delta) => self.push_assistant_delta(&delta),
            RuntimeModelProgress::ReasoningDelta(delta) => self.push_reasoning_delta(&delta),
            RuntimeModelProgress::ReasoningStage(stage) => self.update_reasoning_stage(stage),
            RuntimeModelProgress::ToolCallStarted { call_id, name } => {
                self.start_tool_call(call_id, name);
            }
            RuntimeModelProgress::ToolCallDelta { call_id, delta } => {
                self.push_tool_call_delta(call_id, &delta);
            }
        }
    }

    pub fn abort(&mut self) {
        self.lifecycle.complete_open_text_items();
        self.lifecycle
            .complete_open_tool_calls_with_status(ToolCallStatus::Failed);
        self.emit_pending_events();
    }

    pub fn complete_open_items(&mut self) {
        self.lifecycle.complete_open_text_items();
        self.emit_pending_events();
    }

    #[must_use]
    pub fn take_streamed_tool_call_items(&mut self) -> hashbrown::HashMap<String, String> {
        std::mem::take(&mut self.tool_call_item_ids)
    }

    fn push_assistant_delta(&mut self, delta: &str) {
        if !self.lifecycle.append_assistant_delta(delta) {
            return;
        }

        let _ = self
            .lifecycle
            .emit_assistant_snapshot(Some(self.assistant_item_id.clone()));
        self.emit_pending_events();
    }

    fn push_reasoning_delta(&mut self, delta: &str) {
        if !self.lifecycle.append_reasoning_delta(delta) {
            return;
        }

        if !self.lifecycle.reasoning_started() {
            if self
                .lifecycle
                .emit_reasoning_snapshot(Some(self.reasoning_item_id.clone()))
            {
                self.last_reasoning_emit_len = self.lifecycle.reasoning_len();
                self.emit_pending_events();
            }
            return;
        }

        if !self.should_emit_reasoning_update(false) {
            return;
        }

        if self
            .lifecycle
            .emit_reasoning_snapshot(Some(self.reasoning_item_id.clone()))
        {
            self.record_reasoning_update();
            self.emit_pending_events();
        }
    }

    fn update_reasoning_stage(&mut self, stage: String) {
        let stage_changed = self.reasoning_stage.as_deref() != Some(stage.as_str());
        self.reasoning_stage = Some(stage);
        if !stage_changed
            || !self
                .lifecycle
                .set_reasoning_stage(self.reasoning_stage.clone())
        {
            return;
        }

        if !self.lifecycle.reasoning_started() || !self.should_emit_reasoning_update(true) {
            return;
        }

        if self.lifecycle.emit_reasoning_stage_update() {
            self.record_reasoning_update();
            self.emit_pending_events();
        }
    }

    fn should_emit_reasoning_update(&self, stage_changed: bool) -> bool {
        if self.reasoning_update_events >= MAX_REASONING_UPDATE_EVENTS {
            return false;
        }

        stage_changed
            || self
                .lifecycle
                .reasoning_len()
                .saturating_sub(self.last_reasoning_emit_len)
                >= MIN_REASONING_UPDATE_BYTES
    }

    fn record_reasoning_update(&mut self) {
        self.reasoning_update_events += 1;
        self.last_reasoning_emit_len = self.lifecycle.reasoning_len();
    }

    fn start_tool_call(&mut self, call_id: String, name: Option<String>) {
        let item_id = format!("{}-tool-call-{call_id}", self.assistant_item_id);
        self.tool_call_item_ids
            .insert(call_id.clone(), item_id.clone());
        let _ = self
            .lifecycle
            .start_tool_call(&call_id, name, Some(item_id));
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

pub struct AgentRuntime {
    pub state: AgentSessionState,
    steering: RuntimeSteering,
    event_sink: Option<EventSink>,
    lifecycle: SharedLifecycleEmitter,
    emitted_events: Vec<ThreadEvent>,
}

impl AgentRuntime {
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
        }
    }

    pub fn set_event_handler(&mut self, sink: Option<EventSink>) {
        self.event_sink = sink;
    }

    pub fn set_steering_receiver(&mut self, receiver: Option<UnboundedReceiver<SteeringMessage>>) {
        self.steering.set_receiver(receiver);
    }

    pub fn take_steering_receiver(&mut self) -> Option<UnboundedReceiver<SteeringMessage>> {
        self.steering.take_receiver()
    }

    pub fn split_mut(&mut self) -> (&mut AgentSessionState, &mut RuntimeSteering) {
        (&mut self.state, &mut self.steering)
    }

    #[must_use]
    pub fn has_pending_follow_up_inputs(&self) -> bool {
        self.steering.has_pending_follow_up_inputs()
    }

    pub fn run_until_idle(&mut self) -> Option<String> {
        let input = self.steering.pop_follow_up_input()?;
        self.state.add_user_message(input.clone());
        Some(input)
    }

    pub async fn poll_turn_control(&mut self) -> RuntimeControl {
        self.steering.poll_turn_control().await
    }

    pub async fn poll_tool_control(&mut self) -> RuntimeControl {
        self.steering.poll_tool_control().await
    }

    pub fn take_emitted_events(&mut self) -> Vec<ThreadEvent> {
        std::mem::take(&mut self.emitted_events)
    }

    #[must_use]
    pub fn tool_call_item_id(&self, call_id: &str) -> Option<String> {
        self.lifecycle
            .tool_call_item_id(call_id)
            .map(str::to_string)
    }

    pub fn complete_tool_call(&mut self, call_id: &str, status: ToolCallStatus) {
        let _ = self.lifecycle.complete_tool_call(call_id, status);
        self.emit_pending_lifecycle_events();
    }

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

        let should_emit_snapshot =
            !self.lifecycle.assistant_started() || self.lifecycle.replace_assistant_text(text);
        if should_emit_snapshot {
            let _ = self.lifecycle.emit_assistant_snapshot(None);
        }
        let _ = self.lifecycle.complete_assistant_stream();
    }

    fn finalize_reasoning_lifecycle(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let should_emit_snapshot =
            !self.lifecycle.reasoning_started() || self.lifecycle.replace_reasoning_text(text);
        if should_emit_snapshot {
            let _ = self.lifecycle.emit_reasoning_snapshot(None);
        }
        let _ = self.lifecycle.complete_reasoning_stream();
    }

    fn finalize_tool_call_lifecycle(
        &mut self,
        tool_calls: Option<&[ToolCall]>,
        _finish_reason: &str,
    ) {
        if let Some(tool_calls) = tool_calls {
            for call in tool_calls {
                let tool_name = call.function.as_ref().map(|function| function.name.clone());
                let _ = self
                    .lifecycle
                    .start_tool_call(&call.id, tool_name.clone(), None);
                if let Some(function) = call.function.as_ref() {
                    let _ = self.lifecycle.sync_tool_call_arguments(
                        &call.id,
                        &function.arguments,
                        tool_name,
                        None,
                    );
                }
            }
            return;
        }

        self.lifecycle
            .complete_open_tool_calls_with_status(ToolCallStatus::Failed);
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
                    let _ = self.lifecycle.emit_assistant_snapshot(None);
                    self.emit_pending_lifecycle_events();
                }
            }
            RuntimeModelProgress::ReasoningDelta(delta) => {
                full_reasoning.push_str(&delta);
                if self.lifecycle.append_reasoning_delta(&delta) {
                    let _ = self.lifecycle.emit_reasoning_snapshot(None);
                    self.emit_pending_lifecycle_events();
                }
            }
            RuntimeModelProgress::ReasoningStage(stage) => {
                if self.lifecycle.set_reasoning_stage(Some(stage)) {
                    let _ = self.lifecycle.emit_reasoning_stage_update();
                    self.emit_pending_lifecycle_events();
                }
            }
            RuntimeModelProgress::ToolCallStarted { call_id, name } => {
                let _ = self.lifecycle.start_tool_call(&call_id, name, None);
                self.emit_pending_lifecycle_events();
            }
            RuntimeModelProgress::ToolCallDelta { call_id, delta } => {
                if self
                    .lifecycle
                    .append_tool_call_delta(&call_id, &delta, None, None)
                {
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
        let start_time = std::time::Instant::now();
        let mut full_text = String::new();
        let mut full_reasoning = String::new();
        let mut on_progress =
            |event| self.record_model_progress(event, &mut full_text, &mut full_reasoning);
        let RuntimeModelOutput { mut response } =
            adapter.execute(request, timeout, &mut on_progress).await?;

        merge_stream_and_completed_text(&mut full_text, response.content.as_deref());
        merge_stream_and_completed_text(&mut full_reasoning, response.reasoning.as_deref());

        let finish_reason = match response.finish_reason.clone() {
            FinishReason::Stop => "stop".to_string(),
            FinishReason::ToolCalls => "tool_calls".to_string(),
            FinishReason::Length => "length".to_string(),
            FinishReason::Error(message) => message,
            _ => "unknown".to_string(),
        };
        let final_usage = response.usage.clone().unwrap_or_default();
        let mut aggregated_tool_calls = response.tool_calls.clone();

        self.finalize_assistant_lifecycle(&full_text);
        self.finalize_reasoning_lifecycle(&full_reasoning);
        self.finalize_tool_call_lifecycle(aggregated_tool_calls.as_deref(), &finish_reason);
        self.emit_pending_lifecycle_events();

        let mut turn_recorded = false;
        self.state.record_turn(&start_time, &mut turn_recorded);

        if final_usage.prompt_tokens > 0 || final_usage.completion_tokens > 0 {
            self.state.stats.merge_usage(final_usage.clone());
        }

        aggregated_tool_calls = aggregated_tool_calls.filter(|calls| !calls.is_empty());

        let mut assistant_message = crate::llm::provider::Message::assistant(full_text.clone());
        if !full_reasoning.is_empty() {
            assistant_message = assistant_message.with_reasoning(Some(full_reasoning.clone()));
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

        self.state.messages.push(assistant_message);

        self.state.conversation.push(Content {
            role: "model".to_string(),
            parts: vec![Part::Text {
                text: full_text.clone(),
                thought_signature: None,
            }],
        });
        self.state.last_processed_message_idx = self.state.conversation.len();

        if response.model.is_empty() {
            response.model = request_model;
        }
        response.content = Some(full_text.clone());
        response.reasoning = if full_reasoning.is_empty() {
            None
        } else {
            Some(full_reasoning.clone())
        };
        response.tool_calls = aggregated_tool_calls.clone();
        response.usage = Some(final_usage.clone());
        response.finish_reason = if finish_reason == "tool_calls" {
            FinishReason::ToolCalls
        } else if finish_reason == "Cancelled" || finish_reason == "cancelled" {
            FinishReason::Error("Cancelled".to_string())
        } else {
            response.finish_reason
        };

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

    pub async fn run_turn_once(
        &mut self,
        provider: &mut Box<dyn LLMProvider>,
        request: LLMRequest,
        timeout: Option<std::time::Duration>,
    ) -> Result<TurnExecution> {
        let mut steering = std::mem::take(&mut self.steering);
        let mut adapter = ProviderRuntimeModelAdapter::new(provider, &mut steering);
        let result = self
            .run_turn_once_with_adapter(&mut adapter, request, timeout)
            .await;
        self.steering = steering;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;

    use crate::llm::provider::{
        LLMError, LLMNormalizedStream, LLMStream, LLMStreamEvent, NormalizedStreamEvent,
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
            Ok(Box::pin(stream::iter(vec![
                Ok(NormalizedStreamEvent::ReasoningDelta {
                    delta: self.reasoning_delta.clone(),
                }),
                Ok(NormalizedStreamEvent::TextDelta {
                    delta: self.text_delta.clone(),
                }),
                Ok(NormalizedStreamEvent::Done {
                    response: Box::new(self.response.clone()),
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

    #[tokio::test]
    async fn paused_runtime_resumes_and_preserves_follow_up_inputs() {
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = AgentRuntime::new(state, None, Some(receiver));

        sender
            .send(SteeringMessage::Pause)
            .expect("pause should send");
        sender
            .send(SteeringMessage::FollowUpInput(
                "queued while paused".to_string(),
            ))
            .expect("follow-up should send");
        sender
            .send(SteeringMessage::Resume)
            .expect("resume should send");

        assert_eq!(runtime.poll_turn_control().await, RuntimeControl::Resumed);
        assert!(runtime.has_pending_follow_up_inputs());
        assert_eq!(
            runtime.run_until_idle().as_deref(),
            Some("queued while paused")
        );
    }

    #[tokio::test]
    async fn paused_runtime_stop_request_wins() {
        let state = AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = AgentRuntime::new(state, None, Some(receiver));

        sender
            .send(SteeringMessage::Pause)
            .expect("pause should send");
        sender
            .send(SteeringMessage::SteerStop)
            .expect("stop should send");

        assert_eq!(
            runtime.poll_turn_control().await,
            RuntimeControl::StopRequested
        );
        assert!(!runtime.has_pending_follow_up_inputs());
    }

    #[tokio::test]
    async fn run_turn_once_uses_completed_payload_when_no_deltas_exist() {
        let response = LLMResponse {
            content: Some("### Header\n- item".to_string()),
            model: "test-model".to_string(),
            finish_reason: FinishReason::Stop,
            reasoning: Some("**why** this works".to_string()),
            ..Default::default()
        };
        let provider = CompletedOnlyStreamProvider {
            response: response.clone(),
        };
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
        assert_eq!(turn.reasoning.as_deref(), Some("**why** this works"));
        assert_eq!(turn.response.content.as_deref(), Some("### Header\n- item"));
        assert_eq!(
            turn.response.reasoning.as_deref(),
            Some("**why** this works")
        );
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

    #[test]
    fn streaming_lifecycle_bridge_tracks_tool_call_item_ids() {
        let mut bridge = StreamingLifecycleBridge::new(None, "turn_tool_map", 5, 2);
        bridge.on_progress(RuntimeModelProgress::ToolCallStarted {
            call_id: "call_42".to_string(),
            name: Some("shell".to_string()),
        });

        let item_ids = bridge.take_streamed_tool_call_items();
        assert_eq!(
            item_ids.get("call_42").map(String::as_str),
            Some("turn_tool_map-step-5-assistant-stream-2-tool-call-call_42")
        );
    }
}
