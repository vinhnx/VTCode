//! Bridge layer for converting VT Code events to Open Responses format.
//!
//! This module provides adapters to convert VT Code's internal `ThreadEvent`
//! and `ThreadItem` types to Open Responses-compliant structures, enabling
//! backwards compatibility during migration.

use serde_json::json;

use super::{
    ContentPart, CustomItem, FunctionCallItem, ItemStatus, MessageItem, MessageRole,
    OpenResponseError, OpenUsage, OutputItem, ReasoningItem, Response, ResponseStatus,
    StreamEventEmitter, response::generate_response_id,
};
use vtcode_exec_events::{
    CommandExecutionStatus, McpToolCallStatus, PatchApplyStatus, ThreadEvent, ThreadItem,
    ThreadItemDetails,
};

/// Builder for constructing Open Responses `Response` objects from VT Code events.
///
/// Tracks streaming state and maintains the mapping between VT Code item IDs
/// and Open Responses output item indices.
#[derive(Debug)]
pub struct ResponseBuilder {
    response: Response,
    next_output_index: usize,
    item_id_to_index: std::collections::HashMap<String, usize>,
    active_items: std::collections::HashMap<String, ActiveItemState>,
}

/// State for an active (in-progress) streaming item.
#[derive(Debug, Clone)]
struct ActiveItemState {
    output_index: usize,
    content_index: usize,
    last_emitted_len: usize,
}

impl ResponseBuilder {
    /// Creates a new response builder with the given model.
    pub fn new(model: impl Into<String>) -> Self {
        let response = Response::new(generate_response_id(), model);
        Self {
            response,
            next_output_index: 0,
            item_id_to_index: std::collections::HashMap::new(),
            active_items: std::collections::HashMap::new(),
        }
    }

    /// Returns a reference to the current response.
    pub fn response(&self) -> &Response {
        &self.response
    }

    /// Returns a mutable reference to the current response.
    pub fn response_mut(&mut self) -> &mut Response {
        &mut self.response
    }

    /// Returns the response ID.
    pub fn response_id(&self) -> &str {
        &self.response.id
    }

    /// Consumes the builder and returns the final response.
    pub fn build(self) -> Response {
        self.response
    }

    /// Processes a VT Code `ThreadEvent` and emits corresponding Open Responses events.
    pub fn process_event<E: StreamEventEmitter>(
        &mut self,
        event: &ThreadEvent,
        emitter: &mut E,
    ) {
        match event {
            ThreadEvent::ThreadStarted(_) => {
                emitter.response_created(self.response.clone());
                self.response.status = ResponseStatus::InProgress;
                emitter.response_in_progress(self.response.clone());
            }

            ThreadEvent::TurnStarted(_) => {
                // Turn started is internal to VT Code; no direct Open Responses equivalent
                // The response is already in progress from ThreadStarted
            }

            ThreadEvent::TurnCompleted(evt) => {
                self.response.usage = Some(OpenUsage::from_exec_usage(&evt.usage));
                self.response.status = ResponseStatus::Completed;
                self.response.complete();
                emitter.response_completed(self.response.clone());
            }

            ThreadEvent::TurnFailed(evt) => {
                self.response.fail(OpenResponseError::model_error(&evt.message));
                emitter.response_failed(self.response.clone());
            }

            ThreadEvent::ItemStarted(evt) => {
                self.handle_item_started(&evt.item, emitter);
            }

            ThreadEvent::ItemUpdated(evt) => {
                self.handle_item_updated(&evt.item, emitter);
            }

            ThreadEvent::ItemCompleted(evt) => {
                self.handle_item_completed(&evt.item, emitter);
            }

            ThreadEvent::Error(evt) => {
                self.response.fail(OpenResponseError::server_error(&evt.message));
                emitter.response_failed(self.response.clone());
            }
        }
    }

    fn handle_item_started<E: StreamEventEmitter>(&mut self, item: &ThreadItem, emitter: &mut E) {
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        self.item_id_to_index.insert(item.id.clone(), output_index);

        let output_item = self.convert_thread_item(item, ItemStatus::InProgress);

        // Track active item state for streaming
        let active_state = ActiveItemState {
            output_index,
            content_index: 0,
            last_emitted_len: 0,
        };
        self.active_items.insert(item.id.clone(), active_state);

        self.response.add_output(output_item.clone());
        emitter.output_item_added(&self.response.id, output_index, output_item);
    }

    fn handle_item_updated<E: StreamEventEmitter>(&mut self, item: &ThreadItem, emitter: &mut E) {
        let Some(state) = self.active_items.get_mut(&item.id) else {
            return;
        };

        match &item.details {
            ThreadItemDetails::AgentMessage(msg) => {
                // Emit text delta
                let delta = if msg.text.len() > state.last_emitted_len {
                    &msg.text[state.last_emitted_len..]
                } else {
                    ""
                };

                if !delta.is_empty() {
                    emitter.output_text_delta(
                        &self.response.id,
                        &item.id,
                        state.output_index,
                        state.content_index,
                        delta,
                    );
                    state.last_emitted_len = msg.text.len();
                }
            }

            ThreadItemDetails::Reasoning(r) => {
                // Emit reasoning delta
                let delta = if r.text.len() > state.last_emitted_len {
                    &r.text[state.last_emitted_len..]
                } else {
                    ""
                };

                if !delta.is_empty() {
                    emitter.reasoning_delta(
                        &self.response.id,
                        &item.id,
                        state.output_index,
                        delta,
                    );
                    state.last_emitted_len = r.text.len();
                }
            }

            _ => {
                // Other item types don't have incremental updates
            }
        }
    }

    fn handle_item_completed<E: StreamEventEmitter>(&mut self, item: &ThreadItem, emitter: &mut E) {
        let output_index = self
            .item_id_to_index
            .get(&item.id)
            .copied()
            .unwrap_or_else(|| {
                // Item was completed without being started (atomic item)
                let idx = self.next_output_index;
                self.next_output_index += 1;
                self.item_id_to_index.insert(item.id.clone(), idx);
                idx
            });

        // Determine final status
        let status = self.determine_item_status(&item.details);
        let output_item = self.convert_thread_item(item, status);

        // Update the response output
        if output_index < self.response.output.len() {
            self.response.output[output_index] = output_item.clone();
        } else {
            self.response.add_output(output_item.clone());
        }

        // Clean up active state
        self.active_items.remove(&item.id);

        emitter.output_item_done(&self.response.id, output_index, output_item);
    }

    fn determine_item_status(&self, details: &ThreadItemDetails) -> ItemStatus {
        match details {
            ThreadItemDetails::CommandExecution(cmd) => match cmd.status {
                CommandExecutionStatus::Completed => ItemStatus::Completed,
                CommandExecutionStatus::Failed => ItemStatus::Failed,
                CommandExecutionStatus::InProgress => ItemStatus::InProgress,
            },
            ThreadItemDetails::FileChange(fc) => match fc.status {
                PatchApplyStatus::Completed => ItemStatus::Completed,
                PatchApplyStatus::Failed => ItemStatus::Failed,
            },
            ThreadItemDetails::McpToolCall(tc) => match tc.status {
                Some(McpToolCallStatus::Completed) => ItemStatus::Completed,
                Some(McpToolCallStatus::Failed) => ItemStatus::Failed,
                Some(McpToolCallStatus::Started) | None => ItemStatus::InProgress,
            },
            ThreadItemDetails::Error(_) => ItemStatus::Failed,
            _ => ItemStatus::Completed,
        }
    }

    fn convert_thread_item(&self, item: &ThreadItem, status: ItemStatus) -> OutputItem {
        match &item.details {
            ThreadItemDetails::AgentMessage(msg) => {
                OutputItem::Message(MessageItem {
                    id: item.id.clone(),
                    status,
                    role: MessageRole::Assistant,
                    content: vec![ContentPart::output_text(&msg.text)],
                })
            }

            ThreadItemDetails::Reasoning(r) => {
                OutputItem::Reasoning(ReasoningItem {
                    id: item.id.clone(),
                    status,
                    summary: None,
                    content: Some(r.text.clone()),
                    encrypted_content: None,
                })
            }

            ThreadItemDetails::CommandExecution(cmd) => {
                OutputItem::FunctionCall(FunctionCallItem {
                    id: item.id.clone(),
                    status,
                    name: "vtcode.run_command".to_string(),
                    arguments: json!({
                        "command": cmd.command,
                    }),
                    call_id: Some(item.id.clone()),
                })
            }

            ThreadItemDetails::FileChange(fc) => {
                let changes: Vec<_> = fc
                    .changes
                    .iter()
                    .map(|c| {
                        json!({
                            "path": c.path,
                            "kind": format!("{:?}", c.kind).to_lowercase(),
                        })
                    })
                    .collect();

                OutputItem::Custom(CustomItem {
                    id: item.id.clone(),
                    status,
                    custom_type: "vtcode:file_change".to_string(),
                    data: json!({
                        "changes": changes,
                        "status": format!("{:?}", fc.status).to_lowercase(),
                    }),
                })
            }

            ThreadItemDetails::McpToolCall(tc) => {
                OutputItem::FunctionCall(FunctionCallItem {
                    id: item.id.clone(),
                    status,
                    name: tc.tool_name.clone(),
                    arguments: tc.arguments.clone().unwrap_or(json!({})),
                    call_id: Some(item.id.clone()),
                })
            }

            ThreadItemDetails::WebSearch(ws) => {
                OutputItem::Custom(CustomItem {
                    id: item.id.clone(),
                    status,
                    custom_type: "vtcode:web_search".to_string(),
                    data: json!({
                        "query": ws.query,
                        "provider": ws.provider,
                        "results": ws.results,
                    }),
                })
            }

            ThreadItemDetails::Error(err) => {
                // Errors are represented as custom items
                OutputItem::Custom(CustomItem {
                    id: item.id.clone(),
                    status: ItemStatus::Failed,
                    custom_type: "vtcode:error".to_string(),
                    data: json!({
                        "message": err.message,
                    }),
                })
            }
        }
    }
}

/// Adapter that wraps a VT Code event sink and also emits Open Responses events.
pub struct DualEventEmitter<E: StreamEventEmitter> {
    open_responses_emitter: E,
    builder: ResponseBuilder,
}

impl<E: StreamEventEmitter> DualEventEmitter<E> {
    /// Creates a new dual emitter with the given Open Responses emitter and model.
    pub fn new(emitter: E, model: impl Into<String>) -> Self {
        Self {
            open_responses_emitter: emitter,
            builder: ResponseBuilder::new(model),
        }
    }

    /// Processes a VT Code event and emits corresponding Open Responses events.
    pub fn process(&mut self, event: &ThreadEvent) {
        self.builder
            .process_event(event, &mut self.open_responses_emitter);
    }

    /// Returns a reference to the current response.
    pub fn response(&self) -> &Response {
        self.builder.response()
    }

    /// Returns the underlying Open Responses emitter.
    pub fn into_emitter(self) -> E {
        self.open_responses_emitter
    }

    /// Consumes the adapter and returns the final response.
    pub fn into_response(self) -> Response {
        self.builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_responses::{ResponseStreamEvent, events::VecStreamEmitter};
    use vtcode_exec_events::{
        AgentMessageItem, ItemCompletedEvent, ItemStartedEvent, ThreadStartedEvent,
        TurnCompletedEvent, Usage,
    };

    #[test]
    fn test_response_builder_thread_lifecycle() {
        let mut builder = ResponseBuilder::new("gpt-4");
        let mut emitter = VecStreamEmitter::new();

        // Thread started
        builder.process_event(
            &ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: "thread_1".to_string(),
            }),
            &mut emitter,
        );

        assert_eq!(builder.response().status, ResponseStatus::InProgress);

        // Turn completed
        builder.process_event(
            &ThreadEvent::TurnCompleted(TurnCompletedEvent {
                usage: Usage {
                    input_tokens: 100,
                    cached_input_tokens: 50,
                    output_tokens: 25,
                },
            }),
            &mut emitter,
        );

        assert_eq!(builder.response().status, ResponseStatus::Completed);
        assert!(builder.response().usage.is_some());

        let events = emitter.into_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, ResponseStreamEvent::ResponseCreated { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, ResponseStreamEvent::ResponseCompleted { .. })));
    }

    #[test]
    fn test_response_builder_message_item() {
        let mut builder = ResponseBuilder::new("claude-3");
        let mut emitter = VecStreamEmitter::new();

        // Item started
        let item = ThreadItem {
            id: "msg_1".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Hello".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemStarted(ItemStartedEvent { item: item.clone() }),
            &mut emitter,
        );

        // Item completed
        let completed_item = ThreadItem {
            id: "msg_1".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Hello, world!".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: completed_item,
            }),
            &mut emitter,
        );

        assert_eq!(builder.response().output.len(), 1);
        assert!(matches!(
            &builder.response().output[0],
            OutputItem::Message(_)
        ));

        let events = emitter.into_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, ResponseStreamEvent::OutputItemAdded { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, ResponseStreamEvent::OutputItemDone { .. })));
    }
}
