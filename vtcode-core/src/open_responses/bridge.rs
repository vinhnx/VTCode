//! Bridge layer for converting VT Code events to Open Responses format.
//!
//! This module provides adapters to convert VT Code's internal `ThreadEvent`
//! and `ThreadItem` types to Open Responses-conformant structures, enabling
//! backwards compatibility during migration.

use serde_json::json;

use super::{
    ContentPart, CustomItem, FunctionCallItem, ItemStatus, MessageItem, MessageRole,
    OpenResponseError, OpenUsage, OutputItem, ReasoningItem, Response, ResponseStatus,
    ResponseStreamEvent, StreamEventEmitter, response::generate_response_id,
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
    item_id_to_index: hashbrown::HashMap<String, usize>,
    active_items: hashbrown::HashMap<String, ActiveItemState>,
}

/// State for an active (in-progress) streaming item.
#[derive(Debug, Clone)]
struct ActiveItemState {
    output_index: usize,
    content_index: usize,
    /// Previous text content for safe delta computation (avoids UTF-8 slicing issues)
    prev_text: String,
}

impl ResponseBuilder {
    /// Creates a new response builder with the given model.
    pub fn new(model: impl Into<String>) -> Self {
        let response = Response::new(generate_response_id(), model);
        Self {
            response,
            next_output_index: 0,
            item_id_to_index: hashbrown::HashMap::new(),
            active_items: hashbrown::HashMap::new(),
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
    pub fn process_event<E: StreamEventEmitter>(&mut self, event: &ThreadEvent, emitter: &mut E) {
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
                if self.response.status.is_terminal() {
                    return;
                }
                self.response.usage = Some(OpenUsage::from_exec_usage(&evt.usage));
                self.response.status = ResponseStatus::Completed;
                self.response.complete();
                emitter.response_completed(self.response.clone());
            }

            ThreadEvent::TurnFailed(evt) => {
                if self.response.status.is_terminal() {
                    return;
                }
                self.response
                    .fail(OpenResponseError::model_error(&evt.message));
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
            ThreadEvent::PlanDelta(_) => {
                // Plan deltas are VT Code-specific extension events and are intentionally
                // ignored by the Open Responses bridge. The completed Plan item carries
                // the full final plan content.
            }

            ThreadEvent::Error(evt) => {
                if self.response.status.is_terminal() {
                    return;
                }
                self.response
                    .fail(OpenResponseError::server_error(&evt.message));
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
        // Initialize prev_text from initial content to prevent duplicate deltas
        let initial_text = match &item.details {
            ThreadItemDetails::AgentMessage(msg) => msg.text.clone(),
            ThreadItemDetails::Plan(plan) => plan.text.clone(),
            ThreadItemDetails::Reasoning(r) => r.text.clone(),
            ThreadItemDetails::ToolOutput(output) => output.output.clone(),
            _ => String::new(),
        };
        let active_state = ActiveItemState {
            output_index,
            content_index: 0,
            prev_text: initial_text,
        };
        self.active_items.insert(item.id.clone(), active_state);

        self.response.add_output(output_item.clone());
        emitter.output_item_added(&self.response.id, output_index, output_item.clone());

        // Emit ContentPartAdded for items with text content
        if let OutputItem::Message(ref msg) = output_item
            && !msg.content.is_empty()
        {
            emitter.emit(ResponseStreamEvent::ContentPartAdded {
                response_id: self.response.id.clone(),
                item_id: item.id.clone(),
                output_index,
                content_index: 0,
                part: msg.content[0].clone(),
            });
        }
    }

    fn handle_item_updated<E: StreamEventEmitter>(&mut self, item: &ThreadItem, emitter: &mut E) {
        // Handle updates for items not yet started (implicit start)
        let state = if let Some(state) = self.active_items.get_mut(&item.id) {
            state
        } else {
            // Implicit start: create item and emit Added event
            self.handle_item_started(item, emitter);
            match self.active_items.get_mut(&item.id) {
                Some(s) => s,
                None => return,
            }
        };

        match &item.details {
            ThreadItemDetails::AgentMessage(msg) => {
                // Use strip_prefix for safe UTF-8 delta computation
                let delta = if let Some(suffix) = msg.text.strip_prefix(&state.prev_text) {
                    suffix
                } else {
                    // Non-append update: emit full text as delta (fallback)
                    &msg.text
                };

                if !delta.is_empty() {
                    emitter.output_text_delta(
                        &self.response.id,
                        &item.id,
                        state.output_index,
                        state.content_index,
                        delta,
                    );
                    state.prev_text = msg.text.clone();
                }
            }

            ThreadItemDetails::Reasoning(r) => {
                // Use strip_prefix for safe UTF-8 delta computation
                let delta = if let Some(suffix) = r.text.strip_prefix(&state.prev_text) {
                    suffix
                } else {
                    // Non-append update: emit full text as delta (fallback)
                    &r.text
                };

                if !delta.is_empty() {
                    emitter.reasoning_delta(&self.response.id, &item.id, state.output_index, delta);
                    state.prev_text = r.text.clone();
                }
            }

            ThreadItemDetails::ToolOutput(output) => {
                let delta = if let Some(suffix) = output.output.strip_prefix(&state.prev_text) {
                    suffix
                } else {
                    &output.output
                };

                if !delta.is_empty() {
                    emitter.output_text_delta(
                        &self.response.id,
                        &item.id,
                        state.output_index,
                        state.content_index,
                        delta,
                    );
                    state.prev_text = output.output.clone();
                }
            }

            _ => {
                // Other item types don't have incremental updates
            }
        }
    }

    fn handle_item_completed<E: StreamEventEmitter>(&mut self, item: &ThreadItem, emitter: &mut E) {
        let was_started = self.item_id_to_index.contains_key(&item.id);

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

        // For atomic completions (never started), emit Added first, then ContentPartAdded
        if !was_started {
            emitter.output_item_added(&self.response.id, output_index, output_item.clone());

            // Emit ContentPartAdded for Message and Reasoning items
            match &output_item {
                OutputItem::Message(msg) => {
                    if !msg.content.is_empty() {
                        emitter.emit(ResponseStreamEvent::ContentPartAdded {
                            response_id: self.response.id.clone(),
                            item_id: item.id.clone(),
                            output_index,
                            content_index: 0,
                            part: msg.content[0].clone(),
                        });
                    }
                }
                OutputItem::Reasoning(r) => {
                    let text = r.content.clone().unwrap_or_default();
                    emitter.emit(ResponseStreamEvent::ContentPartAdded {
                        response_id: self.response.id.clone(),
                        item_id: item.id.clone(),
                        output_index,
                        content_index: 0,
                        part: ContentPart::output_text(text),
                    });
                }
                _ => {}
            }
        }

        // Update the response output
        if output_index < self.response.output.len() {
            self.response.output[output_index] = output_item.clone();
        } else {
            self.response.add_output(output_item.clone());
        }

        // Emit content-specific "done" events based on item type
        match &output_item {
            OutputItem::Message(msg) => {
                // Emit OutputTextDone for text content
                if let Some(ContentPart::OutputText(text_content)) = msg.content.first() {
                    emitter.emit(ResponseStreamEvent::OutputTextDone {
                        response_id: self.response.id.clone(),
                        item_id: item.id.clone(),
                        output_index,
                        content_index: 0,
                        text: text_content.text.clone(),
                    });
                    emitter.emit(ResponseStreamEvent::ContentPartDone {
                        response_id: self.response.id.clone(),
                        item_id: item.id.clone(),
                        output_index,
                        content_index: 0,
                        part: msg.content[0].clone(),
                    });
                }
            }
            OutputItem::Reasoning(r) => {
                // Emit ReasoningDone then ContentPartDone
                emitter.emit(ResponseStreamEvent::ReasoningDone {
                    response_id: self.response.id.clone(),
                    item_id: item.id.clone(),
                    output_index,
                    item: output_item.clone(),
                });
                let text = r.content.clone().unwrap_or_default();
                emitter.emit(ResponseStreamEvent::ContentPartDone {
                    response_id: self.response.id.clone(),
                    item_id: item.id.clone(),
                    output_index,
                    content_index: 0,
                    part: ContentPart::output_text(text),
                });
            }
            OutputItem::FunctionCall(fc) => {
                // Emit FunctionCallArgumentsDone
                if let Ok(args_str) = serde_json::to_string(&fc.arguments) {
                    emitter.emit(ResponseStreamEvent::FunctionCallArgumentsDone {
                        response_id: self.response.id.clone(),
                        item_id: item.id.clone(),
                        output_index,
                        arguments: args_str,
                    });
                }
            }
            OutputItem::FunctionCallOutput(fco) => {
                if !fco.output.is_empty() {
                    emitter.emit(ResponseStreamEvent::OutputTextDone {
                        response_id: self.response.id.clone(),
                        item_id: item.id.clone(),
                        output_index,
                        content_index: 0,
                        text: fco.output.clone(),
                    });
                }
            }
            _ => {}
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
            ThreadItemDetails::ToolInvocation(invocation) => match invocation.status {
                vtcode_exec_events::ToolCallStatus::Completed => ItemStatus::Completed,
                vtcode_exec_events::ToolCallStatus::Failed => ItemStatus::Failed,
                vtcode_exec_events::ToolCallStatus::InProgress => ItemStatus::InProgress,
            },
            ThreadItemDetails::ToolOutput(output) => match output.status {
                vtcode_exec_events::ToolCallStatus::Completed => ItemStatus::Completed,
                vtcode_exec_events::ToolCallStatus::Failed => ItemStatus::Failed,
                vtcode_exec_events::ToolCallStatus::InProgress => ItemStatus::InProgress,
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
            ThreadItemDetails::AgentMessage(msg) => OutputItem::Message(MessageItem {
                id: item.id.clone(),
                status,
                role: MessageRole::Assistant,
                content: vec![ContentPart::output_text(&msg.text)],
            }),

            ThreadItemDetails::Reasoning(r) => OutputItem::Reasoning(ReasoningItem {
                id: item.id.clone(),
                status,
                summary: None,
                content: Some(r.text.clone()),
                encrypted_content: None,
            }),

            ThreadItemDetails::Plan(plan) => OutputItem::Custom(CustomItem {
                id: item.id.clone(),
                status,
                custom_type: "vtcode:plan".to_string(),
                data: json!({
                    "text": plan.text,
                }),
            }),

            ThreadItemDetails::CommandExecution(cmd) => OutputItem::Custom(CustomItem {
                id: item.id.clone(),
                status,
                custom_type: "vtcode:command_execution".to_string(),
                data: json!({
                    "command": cmd.command,
                    "arguments": cmd.arguments,
                    "aggregated_output": cmd.aggregated_output,
                    "exit_code": cmd.exit_code,
                    "status": serde_json::to_value(&cmd.status).unwrap_or(serde_json::Value::Null),
                }),
            }),

            ThreadItemDetails::ToolInvocation(invocation) => {
                let tool_name = crate::tools::tool_intent::canonical_unified_exec_tool_name(
                    &invocation.tool_name,
                )
                .unwrap_or(invocation.tool_name.as_str())
                .to_string();
                OutputItem::FunctionCall(FunctionCallItem {
                    id: item.id.clone(),
                    status,
                    name: tool_name,
                    arguments: invocation.arguments.clone().unwrap_or(json!({})),
                    call_id: Some(item.id.clone()),
                })
            }

            ThreadItemDetails::ToolOutput(output) => {
                OutputItem::FunctionCallOutput(crate::open_responses::FunctionCallOutputItem {
                    id: item.id.clone(),
                    status,
                    call_id: Some(output.call_id.clone()),
                    output: output.output.clone(),
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

            ThreadItemDetails::McpToolCall(tc) => OutputItem::FunctionCall(FunctionCallItem {
                id: item.id.clone(),
                status,
                name: tc.tool_name.clone(),
                arguments: tc.arguments.clone().unwrap_or(json!({})),
                call_id: Some(item.id.clone()),
            }),

            ThreadItemDetails::WebSearch(ws) => OutputItem::Custom(CustomItem {
                id: item.id.clone(),
                status,
                custom_type: "vtcode:web_search".to_string(),
                data: json!({
                    "query": ws.query,
                    "provider": ws.provider,
                    "results": ws.results,
                }),
            }),

            ThreadItemDetails::Harness(event) => OutputItem::Custom(CustomItem {
                id: item.id.clone(),
                status,
                custom_type: "vtcode:harness_event".to_string(),
                data: json!({
                    "event": serde_json::to_value(&event.event).unwrap_or(serde_json::Value::Null),
                    "message": event.message,
                    "command": event.command,
                    "exit_code": event.exit_code,
                }),
            }),

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
    use serde_json::json;
    use vtcode_exec_events::{
        AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ItemCompletedEvent,
        ItemStartedEvent, PlanItem, ThreadStartedEvent, ToolCallStatus, ToolInvocationItem,
        ToolOutputItem, TurnCompletedEvent, Usage,
    };

    #[test]
    fn test_response_builder_thread_lifecycle() {
        let mut builder = ResponseBuilder::new("gpt-5");
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
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::ResponseCreated { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::ResponseCompleted { .. }))
        );
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
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::OutputItemAdded { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::OutputItemDone { .. }))
        );
        // Verify ContentPartAdded is emitted
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::ContentPartAdded { .. }))
        );
        // Verify OutputTextDone is emitted
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::OutputTextDone { .. }))
        );
    }

    #[test]
    fn test_atomic_completion_emits_added_and_done() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        // Complete item without prior start (atomic)
        let item = ThreadItem {
            id: "msg_atomic".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Atomic message".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemCompleted(ItemCompletedEvent { item }),
            &mut emitter,
        );

        let events = emitter.into_events();
        // Must emit Added before Done for atomic completions
        let added_pos = events
            .iter()
            .position(|e| matches!(e, ResponseStreamEvent::OutputItemAdded { .. }));
        let done_pos = events
            .iter()
            .position(|e| matches!(e, ResponseStreamEvent::OutputItemDone { .. }));

        assert!(added_pos.is_some(), "OutputItemAdded should be emitted");
        assert!(done_pos.is_some(), "OutputItemDone should be emitted");
        assert!(
            added_pos.unwrap() < done_pos.unwrap(),
            "Added must come before Done"
        );
    }

    #[test]
    fn test_update_without_start_handles_implicit_start() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        // Update without prior start
        let item = ThreadItem {
            id: "msg_implicit".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Hello".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent { item }),
            &mut emitter,
        );

        let events = emitter.into_events();
        // Should have implicitly started
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::OutputItemAdded { .. }))
        );
    }

    #[test]
    fn test_unicode_delta_safety() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        // Start with emoji
        let item1 = ThreadItem {
            id: "msg_unicode".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Hello 👋".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemStarted(ItemStartedEvent { item: item1 }),
            &mut emitter,
        );

        // Update with more content
        let item2 = ThreadItem {
            id: "msg_unicode".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Hello 👋 World 🌍".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent { item: item2 }),
            &mut emitter,
        );

        // Should not panic and should emit delta
        let events = emitter.into_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ResponseStreamEvent::OutputTextDelta { .. }))
        );
    }

    #[test]
    fn test_non_append_update_fallback() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        // Start with some text
        let item1 = ThreadItem {
            id: "msg_edit".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Original text".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemStarted(ItemStartedEvent { item: item1 }),
            &mut emitter,
        );

        // Update with completely different text (non-append)
        let item2 = ThreadItem {
            id: "msg_edit".to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "Completely different".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent { item: item2 }),
            &mut emitter,
        );

        // Should fallback to emitting full text as delta
        let events = emitter.into_events();
        let delta_event = events.iter().find(|e| {
            matches!(
                e,
                ResponseStreamEvent::OutputTextDelta { delta, .. } if delta == "Completely different"
            )
        });
        assert!(
            delta_event.is_some(),
            "Should emit full text as delta for non-append updates"
        );
    }

    #[test]
    fn test_plan_item_maps_to_custom_output() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        let item = ThreadItem {
            id: "plan_1".to_string(),
            details: ThreadItemDetails::Plan(PlanItem {
                text: "- Step 1\n- Step 2".to_string(),
            }),
        };
        builder.process_event(
            &ThreadEvent::ItemCompleted(ItemCompletedEvent { item }),
            &mut emitter,
        );

        assert_eq!(builder.response().output.len(), 1);
        match &builder.response().output[0] {
            OutputItem::Custom(custom) => {
                assert_eq!(custom.custom_type, "vtcode:plan");
                assert_eq!(custom.data["text"], "- Step 1\n- Step 2");
            }
            _ => panic!("expected custom output for plan item"),
        }
    }

    #[test]
    fn test_tool_invocation_uses_canonical_arguments() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        let item = ThreadItem {
            id: "tool_1".to_string(),
            details: ThreadItemDetails::ToolInvocation(ToolInvocationItem {
                tool_name: "exec_command".to_string(),
                arguments: Some(json!({
                    "command": ["git", "status"],
                    "yield_time_ms": 1000
                })),
                status: ToolCallStatus::Completed,
            }),
        };

        builder.process_event(
            &ThreadEvent::ItemCompleted(ItemCompletedEvent { item }),
            &mut emitter,
        );

        match &builder.response().output[0] {
            OutputItem::FunctionCall(call) => {
                assert_eq!(call.name, "unified_exec");
                assert_eq!(call.arguments["command"][0], "git");
                assert_eq!(call.arguments["yield_time_ms"], 1000);
            }
            other => panic!("expected function call, got {other:?}"),
        }
    }

    #[test]
    fn test_tool_output_updates_stream_as_function_call_output() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        builder.process_event(
            &ThreadEvent::ItemStarted(ItemStartedEvent {
                item: ThreadItem {
                    id: "tool_1:output".to_string(),
                    details: ThreadItemDetails::ToolOutput(ToolOutputItem {
                        call_id: "tool_1".to_string(),
                        output: String::new(),
                        exit_code: None,
                        status: ToolCallStatus::InProgress,
                    }),
                },
            }),
            &mut emitter,
        );
        builder.process_event(
            &ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent {
                item: ThreadItem {
                    id: "tool_1:output".to_string(),
                    details: ThreadItemDetails::ToolOutput(ToolOutputItem {
                        call_id: "tool_1".to_string(),
                        output: "On branch".to_string(),
                        exit_code: None,
                        status: ToolCallStatus::InProgress,
                    }),
                },
            }),
            &mut emitter,
        );
        builder.process_event(
            &ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: "tool_1:output".to_string(),
                    details: ThreadItemDetails::ToolOutput(ToolOutputItem {
                        call_id: "tool_1".to_string(),
                        output: "On branch main".to_string(),
                        exit_code: Some(0),
                        status: ToolCallStatus::Completed,
                    }),
                },
            }),
            &mut emitter,
        );

        match &builder.response().output[0] {
            OutputItem::FunctionCallOutput(output) => {
                assert_eq!(output.call_id.as_deref(), Some("tool_1"));
                assert_eq!(output.output, "On branch main");
            }
            other => panic!("expected function call output, got {other:?}"),
        }

        let events = emitter.into_events();
        assert!(events.iter().any(|event| matches!(
            event,
            ResponseStreamEvent::OutputItemAdded {
                item: OutputItem::FunctionCallOutput(_),
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ResponseStreamEvent::OutputTextDelta { delta, .. } if delta == "On branch"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ResponseStreamEvent::OutputTextDone { text, .. } if text == "On branch main"
        )));
    }

    #[test]
    fn test_command_execution_maps_to_custom_extension() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        builder.process_event(
            &ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: "cmd_1".to_string(),
                    details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                        command: "git status".to_string(),
                        arguments: Some(json!({ "cwd": "/repo" })),
                        aggregated_output: "On branch main".to_string(),
                        exit_code: Some(0),
                        status: CommandExecutionStatus::Completed,
                    })),
                },
            }),
            &mut emitter,
        );

        match &builder.response().output[0] {
            OutputItem::Custom(custom) => {
                assert_eq!(custom.custom_type, "vtcode:command_execution");
                assert_eq!(custom.data["command"], "git status");
                assert_eq!(custom.data["exit_code"], 0);
                assert_eq!(custom.data["status"], "completed");
            }
            other => panic!("expected custom output, got {other:?}"),
        }
    }

    #[test]
    fn test_failed_response_ignores_late_completion() {
        let mut builder = ResponseBuilder::new("gpt-5");
        let mut emitter = VecStreamEmitter::new();

        builder.process_event(
            &ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: "thread_1".to_string(),
            }),
            &mut emitter,
        );
        builder.process_event(
            &ThreadEvent::TurnFailed(vtcode_exec_events::TurnFailedEvent {
                message: "boom".to_string(),
                usage: None,
            }),
            &mut emitter,
        );
        builder.process_event(
            &ThreadEvent::TurnCompleted(TurnCompletedEvent {
                usage: Usage::default(),
            }),
            &mut emitter,
        );

        assert_eq!(builder.response().status, ResponseStatus::Failed);
        let events = emitter.into_events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ResponseStreamEvent::ResponseFailed { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ResponseStreamEvent::ResponseCompleted { .. }))
        );
    }
}
