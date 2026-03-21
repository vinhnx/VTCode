use std::collections::HashMap;
use vtcode_core::exec::events::{
    AgentMessageItem, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent, ReasoningItem,
    ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem,
};

use crate::agent::runloop::unified::ui_interaction::StreamProgressEvent;

const MIN_REASONING_UPDATE_BYTES: usize = 256;
const MAX_REASONING_UPDATE_EVENTS: usize = 2;

#[derive(Default)]
struct StreamItemBuffer {
    started: bool,
    text: String,
}

#[derive(Default)]
struct ToolCallBuffer {
    item_id: String,
    started: bool,
    name: Option<String>,
    arguments: String,
}

pub(super) struct HarnessStreamingBridge<'a> {
    emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    assistant_item_id: String,
    reasoning_item_id: String,
    assistant: StreamItemBuffer,
    reasoning: StreamItemBuffer,
    tool_calls: HashMap<String, ToolCallBuffer>,
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
            tool_calls: HashMap::new(),
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
        let tool_call_ids = self.tool_calls.keys().cloned().collect::<Vec<_>>();
        for call_id in tool_call_ids {
            self.complete_tool_call(&call_id);
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

    fn start_tool_call(&mut self, call_id: String, name: Option<String>) {
        let item_id = format!("{}-tool-call-{call_id}", self.assistant_item_id);
        let (item_id, tool_name, started) = {
            let buffer = self
                .tool_calls
                .entry(call_id.clone())
                .or_insert(ToolCallBuffer {
                    item_id,
                    ..Default::default()
                });
            if buffer.name.is_none() {
                buffer.name = name;
            }
            if buffer.started {
                (
                    buffer.item_id.clone(),
                    buffer.name.clone().unwrap_or_default(),
                    true,
                )
            } else {
                buffer.started = true;
                (
                    buffer.item_id.clone(),
                    buffer.name.clone().unwrap_or_default(),
                    false,
                )
            }
        };
        if started {
            return;
        }

        self.emit_item_started(ThreadItem {
            id: item_id,
            details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                tool_name,
                arguments: None,
                tool_call_id: Some(call_id),
                status: ToolCallStatus::InProgress,
            }),
        });
    }

    fn push_tool_call_delta(&mut self, call_id: String, delta: &str) {
        if delta.is_empty() {
            return;
        }

        if !self.tool_calls.contains_key(&call_id) {
            self.start_tool_call(call_id.clone(), None);
        }

        let Some(buffer) = self.tool_calls.get_mut(&call_id) else {
            return;
        };
        buffer.arguments.push_str(delta);
        let item_id = buffer.item_id.clone();
        let tool_name = buffer.name.clone().unwrap_or_default();
        let arguments = progress_tool_arguments(&buffer.arguments);
        let tool_call_id = call_id.clone();

        self.emit_item_updated(ThreadItem {
            id: item_id,
            details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                tool_name,
                arguments: Some(arguments),
                tool_call_id: Some(tool_call_id),
                status: ToolCallStatus::InProgress,
            }),
        });
    }

    fn complete_tool_call(&mut self, call_id: &str) {
        let Some(buffer) = self.tool_calls.get_mut(call_id) else {
            return;
        };
        if !buffer.started {
            return;
        }

        buffer.started = false;
        let item_id = buffer.item_id.clone();
        let tool_name = buffer.name.clone().unwrap_or_default();
        let arguments = if buffer.arguments.is_empty() {
            None
        } else {
            Some(progress_tool_arguments(&buffer.arguments))
        };
        self.emit_item_completed(ThreadItem {
            id: item_id,
            details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                tool_name,
                arguments,
                tool_call_id: Some(call_id.to_string()),
                status: ToolCallStatus::Completed,
            }),
        });
    }
}

fn progress_tool_arguments(arguments: &str) -> serde_json::Value {
    serde_json::from_str(arguments)
        .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()))
}
