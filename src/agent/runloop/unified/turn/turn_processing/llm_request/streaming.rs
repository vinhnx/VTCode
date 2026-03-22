use vtcode_core::core::agent::events::SharedLifecycleEmitter;

use crate::agent::runloop::unified::ui_interaction::StreamProgressEvent;

const MIN_REASONING_UPDATE_BYTES: usize = 256;
const MAX_REASONING_UPDATE_EVENTS: usize = 2;

pub(super) struct HarnessStreamingBridge<'a> {
    emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    assistant_item_id: String,
    reasoning_item_id: String,
    lifecycle: SharedLifecycleEmitter,
    reasoning_stage: Option<String>,
    reasoning_update_events: usize,
    last_reasoning_emit_len: usize,
}

impl<'a> HarnessStreamingBridge<'a> {
    pub(super) fn new(
        emitter: Option<
            &'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter,
        >,
        turn_id: &str,
        step: usize,
        attempt: usize,
    ) -> Self {
        Self {
            emitter,
            assistant_item_id: format!("{turn_id}-step-{step}-assistant-stream-{attempt}"),
            reasoning_item_id: format!("{turn_id}-step-{step}-reasoning-stream-{attempt}"),
            lifecycle: SharedLifecycleEmitter::default(),
            reasoning_stage: None,
            reasoning_update_events: 0,
            last_reasoning_emit_len: 0,
        }
    }

    pub(super) fn on_progress(&mut self, event: StreamProgressEvent) {
        match event {
            StreamProgressEvent::OutputDelta(delta) => self.push_assistant_delta(&delta),
            StreamProgressEvent::ReasoningDelta(delta) => self.push_reasoning_delta(&delta),
            StreamProgressEvent::ReasoningStage(stage) => self.update_reasoning_stage(stage),
            StreamProgressEvent::ToolCallStarted { call_id, name } => {
                self.start_tool_call(call_id, name)
            }
            StreamProgressEvent::ToolCallDelta { call_id, delta } => {
                self.push_tool_call_delta(call_id, &delta)
            }
        }
    }

    pub(super) fn abort(&mut self) {
        self.lifecycle.complete_open_text_items();
        self.lifecycle.complete_open_tool_calls_with_status(
            vtcode_core::exec::events::ToolCallStatus::Failed,
        );
        self.emit_pending_events();
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

    pub(super) fn complete_open_items(&mut self) {
        self.lifecycle.complete_open_text_items();
        self.emit_pending_events();
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
        let _ = self.lifecycle.start_tool_call(
            &call_id,
            name,
            Some(format!("{}-tool-call-{call_id}", self.assistant_item_id)),
        );
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
        let Some(emitter) = self.emitter else {
            let _ = self.lifecycle.drain_events();
            return;
        };

        for event in self.lifecycle.drain_events() {
            let _ = emitter.emit(event);
        }
    }
}
