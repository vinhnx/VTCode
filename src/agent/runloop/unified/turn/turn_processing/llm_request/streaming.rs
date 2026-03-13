use vtcode_core::exec::events::{
    AgentMessageItem, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent, ReasoningItem,
    ThreadEvent, ThreadItem, ThreadItemDetails,
};

use crate::agent::runloop::unified::ui_interaction::StreamProgressEvent;

const MIN_REASONING_UPDATE_BYTES: usize = 256;
const MAX_REASONING_UPDATE_EVENTS: usize = 2;

#[derive(Default)]
struct StreamItemBuffer {
    started: bool,
    text: String,
}

pub(super) struct HarnessStreamingBridge<'a> {
    emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    assistant_item_id: String,
    reasoning_item_id: String,
    assistant: StreamItemBuffer,
    reasoning: StreamItemBuffer,
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
            assistant: StreamItemBuffer::default(),
            reasoning: StreamItemBuffer::default(),
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
        }
    }

    pub(super) fn abort(&mut self) {
        self.complete_open_items();
    }

    fn push_assistant_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.assistant.text.push_str(delta);
        if !self.assistant.started {
            self.assistant.started = true;
            self.emit_item_started(ThreadItem {
                id: self.assistant_item_id.clone(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: self.assistant.text.clone(),
                }),
            });
            return;
        }

        self.emit_item_updated(ThreadItem {
            id: self.assistant_item_id.clone(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: self.assistant.text.clone(),
            }),
        });
    }

    fn push_reasoning_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.reasoning.text.push_str(delta);
        if !self.reasoning.started {
            self.reasoning.started = true;
            self.last_reasoning_emit_len = self.reasoning.text.len();
            self.emit_item_started(ThreadItem {
                id: self.reasoning_item_id.clone(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: self.reasoning.text.clone(),
                    stage: self.reasoning_stage.clone(),
                }),
            });
            return;
        }

        if !self.should_emit_reasoning_update(false) {
            return;
        }

        self.emit_item_updated(ThreadItem {
            id: self.reasoning_item_id.clone(),
            details: ThreadItemDetails::Reasoning(ReasoningItem {
                text: self.reasoning.text.clone(),
                stage: self.reasoning_stage.clone(),
            }),
        });
        self.record_reasoning_update();
    }

    fn update_reasoning_stage(&mut self, stage: String) {
        let stage_changed = self.reasoning_stage.as_deref() != Some(stage.as_str());
        self.reasoning_stage = Some(stage);
        if !self.reasoning.started || !stage_changed || !self.should_emit_reasoning_update(true) {
            return;
        }
        self.emit_item_updated(ThreadItem {
            id: self.reasoning_item_id.clone(),
            details: ThreadItemDetails::Reasoning(ReasoningItem {
                text: self.reasoning.text.clone(),
                stage: self.reasoning_stage.clone(),
            }),
        });
        self.record_reasoning_update();
    }

    pub(super) fn complete_open_items(&mut self) {
        if self.assistant.started {
            self.assistant.started = false;
            self.emit_item_completed(ThreadItem {
                id: self.assistant_item_id.clone(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: self.assistant.text.clone(),
                }),
            });
        }
        if self.reasoning.started {
            self.reasoning.started = false;
            self.emit_item_completed(ThreadItem {
                id: self.reasoning_item_id.clone(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: self.reasoning.text.clone(),
                    stage: self.reasoning_stage.clone(),
                }),
            });
        }
    }

    fn should_emit_reasoning_update(&self, stage_changed: bool) -> bool {
        if self.reasoning_update_events >= MAX_REASONING_UPDATE_EVENTS {
            return false;
        }

        stage_changed
            || self
                .reasoning
                .text
                .len()
                .saturating_sub(self.last_reasoning_emit_len)
                >= MIN_REASONING_UPDATE_BYTES
    }

    fn record_reasoning_update(&mut self) {
        self.reasoning_update_events += 1;
        self.last_reasoning_emit_len = self.reasoning.text.len();
    }

    fn emit_item_started(&self, item: ThreadItem) {
        if let Some(emitter) = self.emitter {
            let _ = emitter.emit(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
        }
    }

    fn emit_item_updated(&self, item: ThreadItem) {
        if let Some(emitter) = self.emitter {
            let _ = emitter.emit(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
        }
    }

    fn emit_item_completed(&self, item: ThreadItem) {
        if let Some(emitter) = self.emitter {
            let _ = emitter.emit(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
        }
    }
}
