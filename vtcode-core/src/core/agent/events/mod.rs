//! Event recording utilities for the agent runner.

pub mod unified;
pub use unified::AgentEvent;

use crate::core::threads::{SubmissionId, ThreadRuntimeHandle};
use crate::exec::events::{
    AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ErrorItem, FileChangeItem,
    FileUpdateChange, HarnessEventItem, HarnessEventKind, ItemCompletedEvent, ItemStartedEvent,
    ItemUpdatedEvent, PatchApplyStatus, PatchChangeKind, ReasoningItem, ThreadEvent, ThreadItem,
    ThreadItemDetails, ThreadStartedEvent, TurnCompletedEvent, TurnFailedEvent, TurnStartedEvent,
    Usage,
};
use parking_lot::Mutex;
use std::sync::Arc;
use uuid::Uuid;

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
    thread_handle: Option<ThreadRuntimeHandle>,
    active_submission_id: Option<SubmissionId>,
    active_turn_id: Option<String>,
    active_assistant_message: Option<StreamingAgentMessage>,
    active_reasoning: Option<StreamingAgentMessage>,
    current_reasoning_stage: Option<String>,
}

impl ExecEventRecorder {
    pub fn new(
        thread_id: impl Into<String>,
        event_sink: Option<EventSink>,
        thread_handle: Option<ThreadRuntimeHandle>,
    ) -> Self {
        let thread_id = thread_id.into();
        let mut recorder = Self {
            events: Vec::new(),
            next_item_index: 0,
            event_sink,
            thread_handle,
            active_submission_id: None,
            active_turn_id: None,
            active_assistant_message: None,
            active_reasoning: None,
            current_reasoning_stage: None,
        };
        recorder.record_with_context(
            None,
            None,
            ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id }),
        );
        recorder
    }

    fn record(&mut self, event: ThreadEvent) {
        self.record_with_context(
            self.active_submission_id.clone(),
            self.active_turn_id.clone(),
            event,
        );
    }

    fn record_with_context(
        &mut self,
        submission_id: Option<SubmissionId>,
        turn_id: Option<String>,
        event: ThreadEvent,
    ) {
        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
            callback(&event);
        }
        if let Some(handle) = &self.thread_handle {
            handle.record_event(submission_id, turn_id, event.clone());
        }
        self.events.push(event);
    }

    fn next_item_id(&mut self) -> String {
        let id = self.next_item_index;
        self.next_item_index += 1;
        format!("item_{id}")
    }

    pub fn turn_started(&mut self) {
        if let Some(handle) = &self.thread_handle {
            match handle.begin_turn() {
                Ok(submission_id) => self.active_submission_id = Some(submission_id),
                Err(_) => self.active_submission_id = None,
            }
            self.active_turn_id = Some(format!("turn-{}", Uuid::new_v4()));
        }
        self.record(ThreadEvent::TurnStarted(TurnStartedEvent::default()));
    }

    pub fn turn_completed(&mut self) {
        self.record(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            usage: Usage::default(),
        }));
        self.finish_turn();
    }

    pub fn turn_failed(&mut self, message: &str) {
        self.record(ThreadEvent::TurnFailed(TurnFailedEvent {
            message: message.to_string(),
            usage: None,
        }));
        self.finish_turn();
    }

    fn finish_turn(&mut self) {
        if let Some(handle) = &self.thread_handle {
            handle.finish_turn();
        }
        self.active_submission_id = None;
        self.active_turn_id = None;
    }

    pub fn agent_message(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.into() }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn agent_message_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }

        if let Some(active) = self.active_assistant_message.as_mut() {
            active.buffer = text.into();
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
            let text_owned = text.to_string();
            let item = ThreadItem {
                id: id.clone(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: text_owned.clone(),
                }),
            };
            self.record(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
            self.active_assistant_message = Some(StreamingAgentMessage {
                id,
                buffer: text_owned,
            });
            true
        }
    }

    pub fn agent_message_stream_complete(&mut self) {
        if let Some(active) = self.active_assistant_message.take() {
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
                stage: self.current_reasoning_stage.clone(),
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn set_reasoning_stage(&mut self, stage: &str) {
        let stage_owned = Some(stage.to_string());
        if self.current_reasoning_stage == stage_owned {
            return;
        }
        self.current_reasoning_stage = stage_owned;
        // If we have an active reasoning stream, update it with the new stage
        if let Some(active) = &self.active_reasoning {
            let item = ThreadItem {
                id: active.id.clone(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: active.buffer.clone(),
                    stage: self.current_reasoning_stage.clone(),
                }),
            };
            self.record(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
        }
    }

    pub fn reasoning_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }

        if let Some(active) = self.active_reasoning.as_mut() {
            active.buffer = text.into();
            let item = ThreadItem {
                id: active.id.clone(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: active.buffer.clone(),
                    stage: self.current_reasoning_stage.clone(),
                }),
            };
            self.record(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
            true
        } else {
            let id = self.next_item_id();
            let text_owned = text.to_string();
            let item = ThreadItem {
                id: id.clone(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: text_owned.clone(),
                    stage: self.current_reasoning_stage.clone(),
                }),
            };
            self.record(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
            self.active_reasoning = Some(StreamingAgentMessage {
                id,
                buffer: text_owned,
            });
            true
        }
    }

    pub fn reasoning_stream_complete(&mut self) {
        if let Some(active) = self.active_reasoning.take() {
            let item = ThreadItem {
                id: active.id,
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: active.buffer,
                    stage: self.current_reasoning_stage.clone(),
                }),
            };
            self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
        }
    }

    pub fn command_started(&mut self, command: &str) -> ActiveCommandHandle {
        let id = self.next_item_id();
        let item = ThreadItem {
            id: id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: command.to_string(),
                arguments: None,
                aggregated_output: String::new(),
                exit_code: None,
                status: CommandExecutionStatus::InProgress,
            })),
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
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: handle.command.clone(),
                arguments: None,
                aggregated_output: aggregated_output.to_string(),
                exit_code,
                status,
            })),
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
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                changes: vec![change],
                status: PatchApplyStatus::Completed,
            })),
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

    pub fn harness_event(
        &mut self,
        event: HarnessEventKind,
        message: Option<String>,
        command: Option<String>,
        exit_code: Option<i32>,
    ) {
        let item = ThreadItem {
            id: self.next_item_id(),
            details: ThreadItemDetails::Harness(HarnessEventItem {
                event,
                message,
                command,
                exit_code,
            }),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    pub fn into_events(mut self) -> Vec<ThreadEvent> {
        if let Some(active) = self.active_assistant_message.take() {
            let item = ThreadItem {
                id: active.id,
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: active.buffer,
                }),
            };
            self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
        }
        if let Some(active) = self.active_reasoning.take() {
            let item = ThreadItem {
                id: active.id,
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: active.buffer,
                    stage: self.current_reasoning_stage.clone(),
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
    use crate::core::threads::{ThreadBootstrap, ThreadManager};

    fn make_recorder() -> ExecEventRecorder {
        ExecEventRecorder::new("thread", None, None)
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

    #[test]
    fn thread_backed_recorder_reuses_submission_id_within_turn() {
        let handle =
            ThreadManager::new().start_thread_with_identifier("thread", ThreadBootstrap::new(None));
        let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

        recorder.turn_started();
        recorder.agent_message("hello");
        recorder.turn_completed();

        let records = handle.replay_recent();
        let submission_ids: std::collections::BTreeSet<String> = records
            .iter()
            .filter_map(|record| {
                record
                    .submission_id
                    .as_ref()
                    .map(|id| id.as_str().to_string())
            })
            .collect();

        assert_eq!(submission_ids.len(), 1);
        assert!(
            records
                .iter()
                .any(|record| matches!(record.event, ThreadEvent::TurnStarted(_))
                    && record.submission_id.is_some())
        );
        assert!(records.iter().any(
            |record| matches!(record.event, ThreadEvent::TurnCompleted(_))
                && record.submission_id.is_some()
        ));
    }

    #[test]
    fn thread_backed_recorder_keeps_full_event_history_beyond_thread_buffer() {
        let handle = ThreadManager::with_event_buffer_capacity(2)
            .start_thread_with_identifier("thread", ThreadBootstrap::new(None));
        let mut recorder = ExecEventRecorder::new("thread", None, Some(handle.clone()));

        recorder.turn_started();
        recorder.agent_message("first");
        recorder.agent_message("second");
        recorder.turn_completed();

        let full_events = recorder.into_events();
        let buffered_events = handle.recent_events();

        assert_eq!(buffered_events.len(), 2);
        assert!(full_events.len() > buffered_events.len());
        assert_eq!(
            full_events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::ItemCompleted(_)))
                .count(),
            2
        );
    }
}
