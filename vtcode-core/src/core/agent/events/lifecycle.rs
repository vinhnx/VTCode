use crate::exec::events::{
    AgentMessageItem, ErrorItem, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent,
    ReasoningItem, ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem,
    ToolOutputItem,
};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
struct StreamingTextState {
    item_id: Option<String>,
    text: String,
    started: bool,
}

#[derive(Debug, Clone)]
struct ToolCallStreamState {
    item_id: String,
    name: Option<String>,
    arguments: String,
    started: bool,
}

/// Shared lifecycle state for assistant text, reasoning, and model-emitted tool calls.
#[derive(Debug, Default)]
pub struct SharedLifecycleEmitter {
    next_item_index: u64,
    assistant: StreamingTextState,
    reasoning: StreamingTextState,
    reasoning_stage: Option<String>,
    tool_calls: HashMap<String, ToolCallStreamState>,
    pending_events: Vec<ThreadEvent>,
}

impl SharedLifecycleEmitter {
    #[must_use]
    pub fn next_item_id(&mut self) -> String {
        let id = self.next_item_index;
        self.next_item_index += 1;
        format!("item_{id}")
    }

    pub fn emit_completed_agent_message(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item_id = self.next_item_id();
        self.pending_events
            .push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: item_id,
                    details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                        text: text.to_string(),
                    }),
                },
            }));
    }

    pub fn replace_assistant_text(&mut self, text: &str) -> bool {
        replace_stream_text(&mut self.assistant, text)
    }

    #[must_use]
    pub fn assistant_started(&self) -> bool {
        self.assistant.started
    }

    pub fn append_assistant_delta(&mut self, delta: &str) -> bool {
        append_stream_delta(&mut self.assistant, delta)
    }

    pub fn emit_assistant_snapshot(&mut self, item_id: Option<String>) -> bool {
        let item_id = item_id.unwrap_or_else(|| self.next_item_id());
        emit_text_snapshot(
            &mut self.pending_events,
            &mut self.assistant,
            item_id,
            |text| ThreadItemDetails::AgentMessage(AgentMessageItem { text }),
        )
    }

    pub fn complete_assistant_stream(&mut self) -> bool {
        complete_text_stream(&mut self.pending_events, &mut self.assistant, |text| {
            ThreadItemDetails::AgentMessage(AgentMessageItem { text })
        })
    }

    pub fn emit_completed_reasoning(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item_id = self.next_item_id();
        self.pending_events
            .push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: item_id,
                    details: ThreadItemDetails::Reasoning(ReasoningItem {
                        text: text.to_string(),
                        stage: self.reasoning_stage.clone(),
                    }),
                },
            }));
    }

    pub fn replace_reasoning_text(&mut self, text: &str) -> bool {
        replace_stream_text(&mut self.reasoning, text)
    }

    pub fn append_reasoning_delta(&mut self, delta: &str) -> bool {
        append_stream_delta(&mut self.reasoning, delta)
    }

    pub fn set_reasoning_stage(&mut self, stage: Option<String>) -> bool {
        if self.reasoning_stage == stage {
            return false;
        }
        self.reasoning_stage = stage;
        true
    }

    #[must_use]
    pub fn reasoning_len(&self) -> usize {
        self.reasoning.text.len()
    }

    #[must_use]
    pub fn reasoning_started(&self) -> bool {
        self.reasoning.started
    }

    pub fn emit_reasoning_snapshot(&mut self, item_id: Option<String>) -> bool {
        let item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let stage = self.reasoning_stage.clone();
        emit_text_snapshot(
            &mut self.pending_events,
            &mut self.reasoning,
            item_id,
            move |text| {
                ThreadItemDetails::Reasoning(ReasoningItem {
                    text,
                    stage: stage.clone(),
                })
            },
        )
    }

    pub fn emit_reasoning_stage_update(&mut self) -> bool {
        if !self.reasoning.started {
            return false;
        }
        let Some(item_id) = self.reasoning.item_id.clone() else {
            return false;
        };
        self.pending_events
            .push(ThreadEvent::ItemUpdated(ItemUpdatedEvent {
                item: ThreadItem {
                    id: item_id,
                    details: ThreadItemDetails::Reasoning(ReasoningItem {
                        text: self.reasoning.text.clone(),
                        stage: self.reasoning_stage.clone(),
                    }),
                },
            }));
        true
    }

    pub fn complete_reasoning_stream(&mut self) -> bool {
        let stage = self.reasoning_stage.clone();
        complete_text_stream(&mut self.pending_events, &mut self.reasoning, move |text| {
            ThreadItemDetails::Reasoning(ReasoningItem {
                text,
                stage: stage.clone(),
            })
        })
    }

    pub fn start_tool_call(
        &mut self,
        call_id: &str,
        tool_name: Option<String>,
        item_id: Option<String>,
    ) -> bool {
        let generated_item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let buffer = self
            .tool_calls
            .entry(call_id.to_string())
            .or_insert_with(|| ToolCallStreamState {
                item_id: generated_item_id,
                name: None,
                arguments: String::new(),
                started: false,
            });

        if buffer.name.is_none() {
            buffer.name = tool_name;
        }
        if buffer.started {
            return false;
        }

        buffer.started = true;
        self.pending_events.push(tool_started_event(
            buffer.item_id.clone(),
            buffer.name.as_deref().unwrap_or_default(),
            None,
            Some(call_id),
        ));
        true
    }

    pub fn append_tool_call_delta(
        &mut self,
        call_id: &str,
        delta: &str,
        tool_name: Option<String>,
        item_id: Option<String>,
    ) -> bool {
        if delta.is_empty() {
            return false;
        }

        if !self.tool_calls.contains_key(call_id) {
            let _ = self.start_tool_call(call_id, tool_name.clone(), item_id);
        }

        let Some(buffer) = self.tool_calls.get_mut(call_id) else {
            return false;
        };

        if buffer.name.is_none() {
            buffer.name = tool_name;
        }

        buffer.arguments.push_str(delta);
        let arguments = progress_tool_arguments(&buffer.arguments);
        self.pending_events.push(tool_invocation_updated_event(
            buffer.item_id.clone(),
            buffer.name.as_deref().unwrap_or_default(),
            Some(&arguments),
            Some(call_id),
            ToolCallStatus::InProgress,
        ));
        true
    }

    pub fn complete_tool_call(&mut self, call_id: &str, status: ToolCallStatus) -> bool {
        let Some(buffer) = self.tool_calls.remove(call_id) else {
            return false;
        };
        if !buffer.started {
            return false;
        }

        let arguments = if buffer.arguments.is_empty() {
            None
        } else {
            Some(progress_tool_arguments(&buffer.arguments))
        };
        self.pending_events.push(tool_invocation_completed_event(
            buffer.item_id,
            buffer.name.as_deref().unwrap_or_default(),
            arguments.as_ref(),
            Some(call_id),
            status,
        ));
        true
    }

    #[must_use]
    pub fn tool_call_item_id(&self, call_id: &str) -> Option<&str> {
        self.tool_calls
            .get(call_id)
            .map(|buffer| buffer.item_id.as_str())
    }

    pub fn sync_tool_call_arguments(
        &mut self,
        call_id: &str,
        arguments: &str,
        tool_name: Option<String>,
        item_id: Option<String>,
    ) -> bool {
        if !self.tool_calls.contains_key(call_id) {
            let _ = self.start_tool_call(call_id, tool_name.clone(), item_id);
        }

        let Some(buffer) = self.tool_calls.get_mut(call_id) else {
            return false;
        };

        if buffer.name.is_none() {
            buffer.name = tool_name;
        }

        if buffer.arguments == arguments {
            return false;
        }

        buffer.arguments.clear();
        buffer.arguments.push_str(arguments);
        let arguments = progress_tool_arguments(&buffer.arguments);
        self.pending_events.push(tool_invocation_updated_event(
            buffer.item_id.clone(),
            buffer.name.as_deref().unwrap_or_default(),
            Some(&arguments),
            Some(call_id),
            ToolCallStatus::InProgress,
        ));
        true
    }

    pub fn complete_open_items(&mut self) {
        self.complete_open_text_items();
        self.complete_open_tool_calls_with_status(ToolCallStatus::Completed);
    }

    pub fn complete_open_text_items(&mut self) {
        let _ = self.complete_assistant_stream();
        let _ = self.complete_reasoning_stream();
    }

    pub fn complete_open_items_with_tool_status(&mut self, status: ToolCallStatus) {
        self.complete_open_text_items();
        self.complete_open_tool_calls_with_status(status);
    }

    pub fn complete_open_tool_calls_with_status(&mut self, status: ToolCallStatus) {
        let call_ids = self.tool_calls.keys().cloned().collect::<Vec<_>>();
        for call_id in call_ids {
            let _ = self.complete_tool_call(&call_id, status.clone());
        }
    }

    #[must_use]
    pub fn drain_events(&mut self) -> Vec<ThreadEvent> {
        std::mem::take(&mut self.pending_events)
    }
}

fn replace_stream_text(state: &mut StreamingTextState, text: &str) -> bool {
    if state.text == text {
        return false;
    }
    state.text.clear();
    state.text.push_str(text);
    true
}

fn append_stream_delta(state: &mut StreamingTextState, delta: &str) -> bool {
    if delta.is_empty() {
        return false;
    }
    state.text.push_str(delta);
    true
}

fn emit_text_snapshot(
    pending_events: &mut Vec<ThreadEvent>,
    state: &mut StreamingTextState,
    item_id: String,
    build_details: impl FnOnce(String) -> ThreadItemDetails,
) -> bool {
    if state.text.trim().is_empty() {
        return false;
    }

    let item_id = state.item_id.get_or_insert(item_id).clone();
    let item = ThreadItem {
        id: item_id,
        details: build_details(state.text.clone()),
    };

    if state.started {
        pending_events.push(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
    } else {
        state.started = true;
        pending_events.push(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
    }
    true
}

fn complete_text_stream(
    pending_events: &mut Vec<ThreadEvent>,
    state: &mut StreamingTextState,
    build_details: impl FnOnce(String) -> ThreadItemDetails,
) -> bool {
    if !state.started {
        return false;
    }

    let Some(item_id) = state.item_id.take() else {
        state.started = false;
        state.text.clear();
        return false;
    };

    state.started = false;
    let text = std::mem::take(&mut state.text);
    pending_events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: item_id,
            details: build_details(text),
        },
    }));
    true
}

#[must_use]
pub fn tool_output_item_id(call_item_id: &str) -> String {
    format!("{call_item_id}:output")
}

fn tool_invocation_item(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
) -> ThreadItem {
    ThreadItem {
        id: item_id,
        details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
            tool_name: tool_name.to_string(),
            arguments: arguments.cloned(),
            tool_call_id: tool_call_id.map(str::to_string),
            status,
        }),
    }
}

fn tool_output_item(
    call_item_id: &str,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    output: impl Into<String>,
) -> ThreadItem {
    ThreadItem {
        id: tool_output_item_id(call_item_id),
        details: ThreadItemDetails::ToolOutput(ToolOutputItem {
            call_id: call_item_id.to_string(),
            tool_call_id: tool_call_id.map(str::to_string),
            spool_path: spool_path.map(str::to_string),
            output: output.into(),
            exit_code,
            status,
        }),
    }
}

#[must_use]
pub fn tool_started_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
) -> ThreadEvent {
    ThreadEvent::ItemStarted(ItemStartedEvent {
        item: tool_invocation_item(
            item_id,
            tool_name,
            arguments,
            tool_call_id,
            ToolCallStatus::InProgress,
        ),
    })
}

#[must_use]
pub fn tool_invocation_updated_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
) -> ThreadEvent {
    ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, status),
    })
}

#[must_use]
pub fn tool_invocation_completed_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, status),
    })
}

#[must_use]
pub fn tool_output_started_event(call_item_id: String, tool_call_id: Option<&str>) -> ThreadEvent {
    ThreadEvent::ItemStarted(ItemStartedEvent {
        item: tool_output_item(
            &call_item_id,
            tool_call_id,
            ToolCallStatus::InProgress,
            None,
            None,
            String::new(),
        ),
    })
}

#[must_use]
pub fn tool_output_updated_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    output: impl Into<String>,
) -> ThreadEvent {
    ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: tool_output_item(
            &call_item_id,
            tool_call_id,
            ToolCallStatus::InProgress,
            None,
            None,
            output,
        ),
    })
}

#[must_use]
pub fn tool_output_completed_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    output: impl Into<String>,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item(
            &call_item_id,
            tool_call_id,
            status,
            exit_code,
            spool_path,
            output,
        ),
    })
}

#[must_use]
pub fn error_item_completed_event(item_id: String, message: impl Into<String>) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: item_id,
            details: ThreadItemDetails::Error(ErrorItem {
                message: message.into(),
            }),
        },
    })
}

fn progress_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_started_event_omits_arguments_when_absent() {
        let event = tool_started_event("item".to_string(), "shell", None, Some("call_1"));
        let ThreadEvent::ItemStarted(ItemStartedEvent { item }) = event else {
            panic!("expected started item");
        };
        let ThreadItemDetails::ToolInvocation(details) = item.details else {
            panic!("expected tool invocation");
        };
        assert!(details.arguments.is_none());
        assert_eq!(details.tool_name, "shell");
    }

    #[test]
    fn tool_output_updated_event_streams_in_progress_output() {
        let event = tool_output_updated_event("item".to_string(), Some("call_1"), "abc");
        let ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }) = event else {
            panic!("expected updated item");
        };
        let ThreadItemDetails::ToolOutput(details) = item.details else {
            panic!("expected tool output");
        };
        assert_eq!(details.call_id, "item");
        assert_eq!(details.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(details.output, "abc");
        assert_eq!(details.status, ToolCallStatus::InProgress);
    }
}
