//! ThreadEvent item handling: started/updated/completed item conversion.

use super::*;

fn tool_output_text(output: &ToolOutputItem) -> String {
    if !output.output.is_empty() {
        return output.output.clone();
    }

    output
        .spool_path
        .as_deref()
        .map(|path| format!("Output saved to {path}"))
        .unwrap_or_default()
}

impl ResponseBuilder {
    pub fn process_event<E: StreamEventEmitter>(&mut self, event: &ThreadEvent, emitter: &mut E) {
        match event {
            ThreadEvent::ThreadStarted(_) => {
                emitter.response_created(self.response.clone());
                self.response.status = ResponseStatus::InProgress;
                emitter.response_in_progress(self.response.clone());
                self.normalized.response_started = true;
            }

            ThreadEvent::TurnStarted(_) => {
                // Turn started is internal to VT Code; no direct Open Responses equivalent
                // The response is already in progress from ThreadStarted
            }

            ThreadEvent::TurnCompleted(evt) => {
                if self.response.status.is_terminal() {
                    return;
                }
                self.response.usage = Some(OpenUsage::from_exec_usage(&evt.usage).into());
                self.response.status = ResponseStatus::Completed;
                self.response.complete();
                emitter.response_completed(self.response.clone());
            }

            ThreadEvent::TurnFailed(evt) => {
                if self.response.status.is_terminal() {
                    return;
                }
                self.response.fail(OpenResponseError::model_error(&evt.message));
                emitter.response_failed(self.response.clone());
            }

            ThreadEvent::TurnBlocked(evt) => {
                self.emit_custom_event(
                    emitter,
                    "vtcode.turn_blocked",
                    json!({
                        "completed_at": evt.completed_at,
                        "message": evt.message,
                        "last_tool": evt.last_tool,
                        "blocked_streak": evt.blocked_streak,
                        "blocked_total": evt.blocked_total,
                        "consecutive_cap": evt.consecutive_cap,
                        "total_cap": evt.total_cap,
                        "recovery_active": evt.recovery_active,
                    }),
                );
            }

            ThreadEvent::ThreadCompleted(evt) => {
                self.emit_custom_event(
                    emitter,
                    "vtcode.thread_completed",
                    json!({
                        "completed_at": evt.completed_at,
                        "thread_id": evt.thread_id,
                        "session_id": evt.session_id,
                        "subtype": evt.subtype.as_str(),
                        "outcome_code": evt.outcome_code,
                        "result": evt.result,
                        "stop_reason": evt.stop_reason,
                        "usage": evt.usage,
                        "total_cost_usd": evt.total_cost_usd,
                        "num_turns": evt.num_turns,
                    }),
                );
            }

            ThreadEvent::ThreadCompactBoundary(evt) => {
                self.emit_custom_event(
                    emitter,
                    "vtcode.thread_compact_boundary",
                    json!({
                        "thread_id": evt.thread_id,
                        "trigger": evt.trigger.as_str(),
                        "mode": evt.mode.as_str(),
                        "original_message_count": evt.original_message_count,
                        "compacted_message_count": evt.compacted_message_count,
                        "history_artifact_path": evt.history_artifact_path,
                    }),
                );
            }

            ThreadEvent::ContextReset(evt) => {
                self.emit_custom_event(
                    emitter,
                    "vtcode.context_reset",
                    json!({
                        "thread_id": evt.thread_id,
                        "turn_id": evt.turn_id,
                        "trigger": evt.trigger,
                        "plan_preserved": evt.plan_preserved,
                        "previous_context_usage_percent": evt.previous_context_usage_percent,
                        "tool_budget_reset": evt.tool_budget_reset,
                    }),
                );
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

            ThreadEvent::PlanApprovalRequested(evt) => {
                self.emit_custom_event(
                    emitter,
                    "vtcode.plan_approval_requested",
                    json!({
                        "thread_id": evt.thread_id,
                        "turn_id": evt.turn_id,
                        "plan_file": evt.plan_file,
                    }),
                );
            }

            ThreadEvent::PlanApprovalResolved(evt) => {
                self.emit_custom_event(
                    emitter,
                    "vtcode.plan_approval_resolved",
                    json!({
                        "thread_id": evt.thread_id,
                        "turn_id": evt.turn_id,
                        "decision": evt.decision,
                        "automatic": evt.automatic,
                    }),
                );
            }

            ThreadEvent::Error(evt) => {
                if self.response.status.is_terminal() {
                    return;
                }
                self.response.fail(OpenResponseError::server_error(&evt.message));
                emitter.response_failed(self.response.clone());
            }

            // Unknown events from newer schema versions are silently skipped.
            ThreadEvent::Unknown
            | ThreadEvent::PermissionRequested(_)
            | ThreadEvent::PermissionResolved(_)
            | ThreadEvent::Interjected(_)
            | ThreadEvent::MatrixUpdated(_) => {}
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
            ThreadItemDetails::ToolOutput(output) => tool_output_text(output),
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

    fn emit_custom_event<E: StreamEventEmitter>(&self, emitter: &mut E, event_type: &str, data: serde_json::Value) {
        emitter.emit(ResponseStreamEvent::CustomEvent {
            response_id: self.response.id.clone(),
            event_type: event_type.to_string(),
            sequence_number: self.next_output_index as u64,
            data,
        });
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
                let current_text = tool_output_text(output);
                let delta = if let Some(suffix) = current_text.strip_prefix(&state.prev_text) {
                    suffix
                } else {
                    current_text.as_str()
                };

                if !delta.is_empty() {
                    emitter.output_text_delta(
                        &self.response.id,
                        &item.id,
                        state.output_index,
                        state.content_index,
                        delta,
                    );
                    state.prev_text = current_text;
                }
            }

            _ => {
                // Other item types don't have incremental updates
            }
        }
    }

    fn handle_item_completed<E: StreamEventEmitter>(&mut self, item: &ThreadItem, emitter: &mut E) {
        let (was_started, output_index) = match self.item_id_to_index.get(&item.id) {
            Some(&idx) => (true, idx),
            None => {
                // Item was completed without being started (atomic item)
                let idx = self.next_output_index;
                self.next_output_index += 1;
                self.item_id_to_index.insert(item.id.clone(), idx);
                (false, idx)
            }
        };

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
            OutputItem::FunctionCallOutput(fco) if !fco.output.is_empty() => {
                emitter.emit(ResponseStreamEvent::OutputTextDone {
                    response_id: self.response.id.clone(),
                    item_id: item.id.clone(),
                    output_index,
                    content_index: 0,
                    text: fco.output.clone(),
                });
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

    fn resolve_tool_call_correlation_id(&mut self, harness_call_id: &str, raw_tool_call_id: Option<&str>) -> String {
        if let Some(existing) = self.tool_call_correlation_ids.get(harness_call_id) {
            return existing.clone();
        }

        let correlation_id = match raw_tool_call_id {
            Some(raw_id) if self.used_tool_call_ids.insert(raw_id.to_string()) => raw_id.to_string(),
            _ => harness_call_id.to_string(),
        };
        self.tool_call_correlation_ids
            .insert(harness_call_id.to_string(), correlation_id.clone());
        correlation_id
    }

    fn convert_thread_item(&mut self, item: &ThreadItem, status: ItemStatus) -> OutputItem {
        match &item.details {
            ThreadItemDetails::Decision(decision) => OutputItem::Custom(CustomItem {
                id: item.id.clone().into(),
                status,
                custom_type: "vtcode:decision".into(),
                data: json!({"decision": decision, "context": item.context}),
            }),
            ThreadItemDetails::AgentMessage(msg) => OutputItem::Message(MessageItem {
                id: item.id.clone().into(),
                status,
                role: MessageRole::Assistant,
                content: vec![ContentPart::output_text(&msg.text)],
            }),

            ThreadItemDetails::Reasoning(r) => OutputItem::Reasoning(ReasoningItem {
                id: item.id.clone().into(),
                status,
                summary: None,
                content: Some(r.text.clone()),
                encrypted_content: None,
            }),

            ThreadItemDetails::Plan(plan) => OutputItem::Custom(CustomItem {
                id: item.id.clone().into(),
                status,
                custom_type: "vtcode:plan".to_string(),
                data: json!({
                    "text": plan.text,
                }),
            }),

            ThreadItemDetails::CommandExecution(cmd) => OutputItem::Custom(CustomItem {
                id: item.id.clone().into(),
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

            ThreadItemDetails::ToolInvocation(invocation) => OutputItem::FunctionCall(FunctionCallItem {
                id: item.id.clone().into(),
                status,
                name: invocation.tool_name.clone(),
                arguments: invocation.arguments.clone().unwrap_or(json!({})),
                call_id: Some(self.resolve_tool_call_correlation_id(&item.id, invocation.tool_call_id.as_deref())),
            }),

            ThreadItemDetails::ToolOutput(output) => {
                OutputItem::FunctionCallOutput(crate::open_responses::FunctionCallOutputItem {
                    id: item.id.clone().into(),
                    status,
                    call_id: Some(
                        self.resolve_tool_call_correlation_id(&output.call_id, output.tool_call_id.as_deref()),
                    ),
                    output: tool_output_text(output),
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
                    id: item.id.clone().into(),
                    status,
                    custom_type: "vtcode:file_change".to_string(),
                    data: json!({
                        "changes": changes,
                        "status": format!("{:?}", fc.status).to_lowercase(),
                    }),
                })
            }

            ThreadItemDetails::McpToolCall(tc) => OutputItem::FunctionCall(FunctionCallItem {
                id: item.id.clone().into(),
                status,
                name: tc.tool_name.clone(),
                arguments: tc.arguments.clone().unwrap_or(json!({})),
                call_id: Some(item.id.clone()),
            }),

            ThreadItemDetails::WebSearch(ws) => OutputItem::Custom(CustomItem {
                id: item.id.clone().into(),
                status,
                custom_type: "vtcode:web_search".to_string(),
                data: json!({
                    "query": ws.query,
                    "provider": ws.provider,
                    "results": ws.results,
                }),
            }),

            ThreadItemDetails::Harness(event) => OutputItem::Custom(CustomItem {
                id: item.id.clone().into(),
                status,
                custom_type: "vtcode:harness_event".to_string(),
                data: json!({
                    "event": serde_json::to_value(&event.event).unwrap_or(serde_json::Value::Null),
                    "message": event.message,
                    "command": event.command,
                    "path": event.path,
                    "exit_code": event.exit_code,
                }),
            }),

            ThreadItemDetails::Error(err) => {
                // Errors are represented as custom items
                OutputItem::Custom(CustomItem {
                    id: item.id.clone().into(),
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
