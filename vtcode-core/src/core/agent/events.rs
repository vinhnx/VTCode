//! Event recording utilities for the agent runner.

use crate::exec::events::{
    AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ErrorItem, FileChangeItem,
    FileUpdateChange, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent, PatchApplyStatus,
    PatchChangeKind, ReasoningItem, ThreadEvent, ThreadItem, ThreadItemDetails, ThreadStartedEvent,
    TurnCompletedEvent, TurnFailedEvent, TurnStartedEvent, Usage,
};
use std::sync::{Arc, Mutex};
use tracing::warn;

/// Callback type alias for streaming structured events.
pub type EventSink = Arc<Mutex<Box<dyn FnMut(&ThreadEvent) + Send>>>;

#[derive(Debug, Clone)]
pub struct ActiveCommandHandle {
    id: String,
    command: String,
}

#[derive(Debug, Clone)]
struct StreamingAgentMessage {
    id: String,
    buffer: String,
}

/// Helper responsible for recording execution events and relaying them to optional sinks.
#[derive(Default)]
pub struct ExecEventRecorder {
    events: Vec<ThreadEvent>,
    next_item_index: u64,
    event_sink: Option<EventSink>,
    active_agent_message: Option<StreamingAgentMessage>,
}

impl ExecEventRecorder {
    pub fn new(thread_id: impl Into<String>, event_sink: Option<EventSink>) -> Self {
        let mut recorder = Self {
            events: Vec::new(),
            next_item_index: 0,
            event_sink,
            active_agent_message: None,
        };
        recorder.record(ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: thread_id.into(),
        }));
        recorder
    }

    fn record(&mut self, event: ThreadEvent) {
        if let Some(sink) = &self.event_sink {
            match sink.lock() {
                Ok(mut callback) => {
                    callback(&event);
                }
                Err(err) => {
                    warn!("Failed to acquire event sink lock: {}", err);
                }
            }
        }
        self.events.push(event);
    }

    fn next_item_id(&mut self) -> String {
        let id = self.next_item_index;
        self.next_item_index += 1;
        format!("item_{id}")
    }

    pub fn turn_started(&mut self) {
        self.record(ThreadEvent::TurnStarted(TurnStartedEvent::default()));
    }

    pub fn turn_completed(&mut self) {
        self.record(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            usage: Usage::default(),
        }));
    }

    pub fn turn_failed(&mut self, message: &str) {
        self.record(ThreadEvent::TurnFailed(TurnFailedEvent {
            message: message.to_string(),
            usage: None,
        }));
    }

    pub fn agent_message(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: text.to_string(),
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn agent_message_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }

        if let Some(active) = self.active_agent_message.as_mut() {
            active.buffer = text.to_string();
            let item = ThreadItem {
                id: active.id.clone(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: active.buffer.clone(),
                }),
            };
            self.record(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
            true
        } else {
            let id = self.next_item_id();
            let item = ThreadItem {
                id: id.clone(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: text.to_string(),
                }),
            };
            self.record(ThreadEvent::ItemStarted(ItemStartedEvent {
                item: item.clone(),
            }));
            self.active_agent_message = Some(StreamingAgentMessage {
                id,
                buffer: text.to_string(),
            });
            true
        }
    }

    pub fn agent_message_stream_complete(&mut self) {
        if let Some(active) = self.active_agent_message.take() {
            let item = ThreadItem {
                id: active.id,
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: active.buffer,
                }),
            };
            self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
        }
    }

    pub fn reasoning(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::Reasoning(ReasoningItem {
                text: text.to_string(),
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn command_started(&mut self, command: &str) -> ActiveCommandHandle {
        let id = self.next_item_id();
        let item = ThreadItem {
            id: id.clone(),
            details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
                command: command.to_string(),
                aggregated_output: String::new(),
                exit_code: None,
                status: CommandExecutionStatus::InProgress,
            }),
        };
        self.record(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
        ActiveCommandHandle {
            id,
            command: command.to_string(),
        }
    }

    pub fn command_finished(
        &mut self,
        handle: &ActiveCommandHandle,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
    ) {
        let item = ThreadItem {
            id: handle.id.clone(),
            details: ThreadItemDetails::CommandExecution(CommandExecutionItem {
                command: handle.command.clone(),
                aggregated_output: aggregated_output.to_string(),
                exit_code,
                status,
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn file_change_completed(&mut self, path: &str) {
        let change = FileUpdateChange {
            path: path.to_string(),
            kind: PatchChangeKind::Update,
        };
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::FileChange(FileChangeItem {
                changes: vec![change],
                status: PatchApplyStatus::Completed,
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn warning(&mut self, message: &str) {
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::Error(ErrorItem {
                message: message.to_string(),
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn into_events(mut self) -> Vec<ThreadEvent> {
        if let Some(active) = self.active_agent_message.take() {
            let item = ThreadItem {
                id: active.id,
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: active.buffer,
                }),
            };
            self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
        }
        self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_recorder() -> ExecEventRecorder {
        ExecEventRecorder::new("thread", None)
    }

    #[test]
    fn streaming_events_flush_on_completion() {
        let mut recorder = make_recorder();
        recorder.turn_started();
        assert!(recorder.agent_message_stream_update("partial"));
        recorder.agent_message_stream_complete();
        let events = recorder.into_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ThreadEvent::ItemCompleted(_)))
        );
    }

    #[test]
    fn command_events_capture_status() {
        let mut recorder = make_recorder();
        let handle = recorder.command_started("git status");
        recorder.command_finished(&handle, CommandExecutionStatus::Completed, Some(0), "");
        let events = recorder.into_events();
        let command = events
            .into_iter()
            .filter_map(|event| match event {
                ThreadEvent::ItemCompleted(event) => Some(event.item),
                _ => None,
            })
            .find(|item| matches!(item.details, ThreadItemDetails::CommandExecution(_)))
            .expect("command event should be emitted");

        match command.details {
            ThreadItemDetails::CommandExecution(details) => {
                assert_eq!(details.command, "git status");
                assert_eq!(details.status, CommandExecutionStatus::Completed);
            }
            _ => panic!("unexpected event variant"),
        }
    }
}
