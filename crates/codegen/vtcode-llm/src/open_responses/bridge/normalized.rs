//! Normalized provider-stream event handling.

use super::*;

impl ResponseBuilder {
    pub(crate) fn process_normalized_event<E: StreamEventEmitter>(
        &mut self,
        event: &NormalizedStreamEvent,
        emitter: &mut E,
    ) {
        if self.response.status.is_terminal() {
            return;
        }

        match event {
            NormalizedStreamEvent::TextDelta { delta } => {
                self.ensure_normalized_response_started(emitter);
                if delta.is_empty() {
                    return;
                }

                let (item_id, output_index) = self.ensure_normalized_message_item(emitter);
                self.append_message_delta(&item_id, output_index, delta);
                emitter.output_text_delta(&self.response.id, &item_id, output_index, 0, delta);
            }
            NormalizedStreamEvent::ReasoningDelta { delta, source } if source.is_public_summary() => {
                self.ensure_normalized_response_started(emitter);
                if delta.is_empty() {
                    return;
                }

                let (item_id, output_index) = self.ensure_normalized_reasoning_item(emitter);
                self.append_reasoning_delta(&item_id, output_index, delta);
                emitter.reasoning_delta(&self.response.id, &item_id, output_index, delta);
            }
            NormalizedStreamEvent::ReasoningDelta { .. } => {}
            NormalizedStreamEvent::ReasoningStage { .. } => {
                // Open Responses has no portable stage field. The VT Code
                // runtime lifecycle receives this event separately.
                self.ensure_normalized_response_started(emitter);
            }
            NormalizedStreamEvent::ToolCallStart { call_id, name } => {
                self.ensure_normalized_response_started(emitter);
                self.ensure_normalized_tool_call(call_id, name.as_deref(), emitter);
            }
            NormalizedStreamEvent::ToolCallDelta { call_id, delta } => {
                self.ensure_normalized_response_started(emitter);
                if delta.is_empty() {
                    return;
                }

                let (item_id, output_index) = self.ensure_normalized_tool_call(call_id, None, emitter);
                self.append_tool_call_delta(call_id, output_index, delta);
                emitter.emit(ResponseStreamEvent::FunctionCallArgumentsDelta {
                    response_id: self.response.id.clone(),
                    item_id,
                    output_index,
                    delta: delta.clone(),
                });
            }
            NormalizedStreamEvent::Usage { usage } => {
                self.ensure_normalized_response_started(emitter);
                self.response.usage = Some(OpenUsage::from_llm_usage(usage).into());
            }
            NormalizedStreamEvent::Done { response } => {
                self.ensure_normalized_response_started(emitter);
                self.finalize_normalized_response(response, emitter);
            }
        }
    }

    fn ensure_normalized_response_started<E: StreamEventEmitter>(&mut self, emitter: &mut E) {
        if self.normalized.response_started {
            return;
        }

        emitter.response_created(self.response.clone());
        self.response.status = ResponseStatus::InProgress;
        emitter.response_in_progress(self.response.clone());
        self.normalized.response_started = true;
    }

    fn ensure_normalized_message_item<E: StreamEventEmitter>(&mut self, emitter: &mut E) -> (String, usize) {
        if let Some(item_id) = self.normalized.message_item_id.clone()
            && let Some(state) = self.active_items.get(&item_id)
        {
            return (item_id, state.output_index);
        }

        let item_id = generate_item_id();
        let output_index = self.allocate_output_index(&item_id);
        let item = OutputItem::message(item_id.clone(), MessageRole::Assistant, vec![ContentPart::output_text("")]);

        self.response.add_output(item.clone());
        self.active_items.insert(
            item_id.clone(),
            ActiveItemState {
                output_index,
                content_index: 0,
                prev_text: String::new(),
            },
        );
        self.normalized.message_item_id = Some(item_id.clone());

        emitter.output_item_added(&self.response.id, output_index, item);
        emitter.emit(ResponseStreamEvent::ContentPartAdded {
            response_id: self.response.id.clone(),
            item_id: item_id.clone(),
            output_index,
            content_index: 0,
            part: ContentPart::output_text(""),
        });

        (item_id, output_index)
    }

    fn ensure_normalized_reasoning_item<E: StreamEventEmitter>(&mut self, emitter: &mut E) -> (String, usize) {
        if let Some(item_id) = self.normalized.reasoning_item_id.clone()
            && let Some(state) = self.active_items.get(&item_id)
        {
            return (item_id, state.output_index);
        }

        let item_id = generate_item_id();
        let output_index = self.allocate_output_index(&item_id);
        let item = OutputItem::reasoning(item_id.clone());

        self.response.add_output(item.clone());
        self.active_items.insert(
            item_id.clone(),
            ActiveItemState {
                output_index,
                content_index: 0,
                prev_text: String::new(),
            },
        );
        self.normalized.reasoning_item_id = Some(item_id.clone());

        emitter.output_item_added(&self.response.id, output_index, item);
        (item_id, output_index)
    }

    fn ensure_normalized_tool_call<E: StreamEventEmitter>(
        &mut self,
        call_id: &str,
        name: Option<&str>,
        emitter: &mut E,
    ) -> (String, usize) {
        if let Some(existing) = self.normalized.tool_calls.get_mut(call_id) {
            if existing.name.is_none()
                && let Some(name) = name
            {
                existing.name = Some(name.to_string());
                if let Some(OutputItem::FunctionCall(item)) = self.response.output.get_mut(existing.output_index) {
                    item.name = name.to_string();
                }
            }
            return (existing.item_id.clone(), existing.output_index);
        }

        let item_id = call_id.to_string();
        let output_index = self.allocate_output_index(&item_id);
        let item = OutputItem::FunctionCall(FunctionCallItem {
            id: item_id.clone().into(),
            status: ItemStatus::InProgress,
            name: name.unwrap_or_default().to_string(),
            arguments: serde_json::Value::String(String::new()),
            call_id: Some(call_id.to_string()),
        });

        self.response.add_output(item.clone());
        self.active_items.insert(
            item_id.clone(),
            ActiveItemState {
                output_index,
                content_index: 0,
                prev_text: String::new(),
            },
        );
        self.normalized.tool_calls.insert(
            call_id.to_string(),
            NormalizedFunctionCallState {
                item_id: item_id.clone(),
                output_index,
                name: name.map(ToOwned::to_owned),
                arguments: String::new(),
            },
        );

        emitter.output_item_added(&self.response.id, output_index, item);
        (item_id, output_index)
    }

    fn append_message_delta(&mut self, item_id: &str, output_index: usize, delta: &str) {
        if let Some(OutputItem::Message(message)) = self.response.output.get_mut(output_index)
            && let Some(ContentPart::OutputText(text)) = message.content.first_mut()
        {
            text.text.push_str(delta);
        }

        if let Some(state) = self.active_items.get_mut(item_id) {
            state.prev_text.push_str(delta);
        }
    }

    fn append_reasoning_delta(&mut self, item_id: &str, output_index: usize, delta: &str) {
        if let Some(OutputItem::Reasoning(reasoning)) = self.response.output.get_mut(output_index) {
            reasoning.content.get_or_insert_with(String::new).push_str(delta);
        }

        if let Some(state) = self.active_items.get_mut(item_id) {
            state.prev_text.push_str(delta);
        }
    }

    fn append_tool_call_delta(&mut self, call_id: &str, output_index: usize, delta: &str) {
        if let Some(state) = self.normalized.tool_calls.get_mut(call_id) {
            state.arguments.push_str(delta);
            if let Some(OutputItem::FunctionCall(item)) = self.response.output.get_mut(output_index) {
                item.arguments = normalized_tool_call_arguments(&state.arguments);
            }
        }

        if let Some(state) = self.active_items.get_mut(call_id) {
            state.prev_text.push_str(delta);
        }
    }

    fn finalize_normalized_response<E: StreamEventEmitter>(
        &mut self,
        response: &crate::provider::LLMResponse,
        emitter: &mut E,
    ) {
        if let Some(usage) = response.usage.as_ref() {
            self.response.usage = Some(OpenUsage::from_llm_usage(usage).into());
        }
        if !response.model.trim().is_empty() {
            self.response.model = response.model.clone();
        }

        let message_text = response.content.clone().or_else(|| self.current_message_text());
        if let Some(text) = message_text
            && !text.is_empty()
        {
            self.complete_normalized_message_item(&text, emitter);
        }

        // The completed response may retain raw/continuation reasoning for the
        // next provider request. Only the normalized public-summary deltas are
        // safe to expose as an Open Responses reasoning item.
        let reasoning_text = self.current_reasoning_text();
        if let Some(text) = reasoning_text
            && !text.is_empty()
        {
            self.complete_normalized_reasoning_item(&text, emitter);
        }

        let mut finalized_call_ids = hashbrown::HashSet::new();
        if let Some(tool_calls) = response.tool_calls.as_ref() {
            for tool_call in tool_calls {
                self.complete_normalized_tool_call(tool_call, emitter);
                finalized_call_ids.insert(tool_call.id.clone());
            }
        }

        let pending_call_ids = self
            .normalized
            .tool_calls
            .keys()
            .filter(|call_id| !finalized_call_ids.contains(*call_id))
            .cloned()
            .collect::<Vec<_>>();
        for call_id in pending_call_ids {
            self.complete_normalized_tool_call_fallback(&call_id, emitter);
        }

        match &response.finish_reason {
            FinishReason::Length => {
                self.response
                    .incomplete(crate::open_responses::IncompleteReason::MaxOutputTokens);
                emitter.emit(ResponseStreamEvent::ResponseIncomplete { response: self.response.clone() });
            }
            FinishReason::ContentFilter => {
                self.response.incomplete(crate::open_responses::IncompleteReason::ContentFilter);
                emitter.emit(ResponseStreamEvent::ResponseIncomplete { response: self.response.clone() });
            }
            FinishReason::Error(message) => {
                self.response.fail(OpenResponseError::model_error(message));
                emitter.response_failed(self.response.clone());
            }
            _ => {
                self.response.complete();
                emitter.response_completed(self.response.clone());
            }
        }
    }

    fn complete_normalized_message_item<E: StreamEventEmitter>(&mut self, text: &str, emitter: &mut E) {
        let (item_id, output_index) = match self.normalized.message_item_id.clone() {
            Some(item_id) => (item_id.clone(), self.output_index_for_item(&item_id)),
            None => {
                let item_id = generate_item_id();
                let output_index = self.allocate_output_index(&item_id);
                let item =
                    OutputItem::message(item_id.clone(), MessageRole::Assistant, vec![ContentPart::output_text("")]);
                self.response.add_output(item.clone());
                emitter.output_item_added(&self.response.id, output_index, item);
                emitter.emit(ResponseStreamEvent::ContentPartAdded {
                    response_id: self.response.id.clone(),
                    item_id: item_id.clone(),
                    output_index,
                    content_index: 0,
                    part: ContentPart::output_text(""),
                });
                self.normalized.message_item_id = Some(item_id.clone());
                (item_id, output_index)
            }
        };

        let completed = OutputItem::completed_message(
            item_id.clone(),
            MessageRole::Assistant,
            vec![ContentPart::output_text(text)],
        );
        self.response.output[output_index] = completed.clone();
        self.active_items.remove(&item_id);

        emitter.emit(ResponseStreamEvent::OutputTextDone {
            response_id: self.response.id.clone(),
            item_id: item_id.clone(),
            output_index,
            content_index: 0,
            text: text.to_string(),
        });
        emitter.emit(ResponseStreamEvent::ContentPartDone {
            response_id: self.response.id.clone(),
            item_id: item_id.clone(),
            output_index,
            content_index: 0,
            part: ContentPart::output_text(text),
        });
        emitter.output_item_done(&self.response.id, output_index, completed);
    }

    fn complete_normalized_reasoning_item<E: StreamEventEmitter>(&mut self, text: &str, emitter: &mut E) {
        let (item_id, output_index) = match self.normalized.reasoning_item_id.clone() {
            Some(item_id) => (item_id.clone(), self.output_index_for_item(&item_id)),
            None => {
                let item_id = generate_item_id();
                let output_index = self.allocate_output_index(&item_id);
                let item = OutputItem::reasoning(item_id.clone());
                self.response.add_output(item.clone());
                emitter.output_item_added(&self.response.id, output_index, item);
                self.normalized.reasoning_item_id = Some(item_id.clone());
                (item_id, output_index)
            }
        };

        let completed = OutputItem::Reasoning(ReasoningItem {
            id: item_id.clone().into(),
            status: ItemStatus::Completed,
            summary: None,
            content: Some(text.to_string()),
            encrypted_content: None,
        });
        self.response.output[output_index] = completed.clone();
        self.active_items.remove(&item_id);

        emitter.emit(ResponseStreamEvent::ReasoningDone {
            response_id: self.response.id.clone(),
            item_id: item_id.clone(),
            output_index,
            item: completed.clone(),
        });
        emitter.emit(ResponseStreamEvent::ContentPartDone {
            response_id: self.response.id.clone(),
            item_id: item_id.clone(),
            output_index,
            content_index: 0,
            part: ContentPart::output_text(text),
        });
        emitter.output_item_done(&self.response.id, output_index, completed);
    }

    fn complete_normalized_tool_call<E: StreamEventEmitter>(&mut self, tool_call: &ToolCall, emitter: &mut E) {
        let arguments = tool_call
            .function
            .as_ref()
            .map(|function| function.arguments.as_str())
            .or(tool_call.text.as_deref())
            .unwrap_or_default();
        let name = tool_call
            .function
            .as_ref()
            .map(|function| function.name.clone())
            .unwrap_or_else(|| tool_call.call_type.clone());
        self.complete_tool_call_item(&tool_call.id, Some(name), arguments, emitter);
    }

    fn complete_normalized_tool_call_fallback<E: StreamEventEmitter>(&mut self, call_id: &str, emitter: &mut E) {
        let Some(state) = self.normalized.tool_calls.get(call_id).cloned() else {
            return;
        };
        self.complete_tool_call_item(call_id, state.name, &state.arguments, emitter);
    }

    fn complete_tool_call_item<E: StreamEventEmitter>(
        &mut self,
        call_id: &str,
        name: Option<String>,
        arguments: &str,
        emitter: &mut E,
    ) {
        let (item_id, output_index, final_name) = match self.normalized.tool_calls.get(call_id) {
            Some(state) => {
                (state.item_id.clone(), state.output_index, name.or_else(|| state.name.clone()).unwrap_or_default())
            }
            None => {
                let item_id = call_id.to_string();
                let output_index = self.allocate_output_index(&item_id);
                let item = OutputItem::FunctionCall(FunctionCallItem {
                    id: item_id.clone().into(),
                    status: ItemStatus::InProgress,
                    name: name.clone().unwrap_or_default(),
                    arguments: serde_json::Value::String(String::new()),
                    call_id: Some(call_id.to_string()),
                });
                self.response.add_output(item.clone());
                emitter.output_item_added(&self.response.id, output_index, item);
                (item_id, output_index, name.unwrap_or_default())
            }
        };

        let completed = OutputItem::FunctionCall(FunctionCallItem {
            id: item_id.clone().into(),
            status: ItemStatus::Completed,
            name: final_name,
            arguments: normalized_tool_call_arguments(arguments),
            call_id: Some(call_id.to_string()),
        });
        self.response.output[output_index] = completed.clone();
        self.active_items.remove(&item_id);
        self.normalized.tool_calls.remove(call_id);

        emitter.emit(ResponseStreamEvent::FunctionCallArgumentsDone {
            response_id: self.response.id.clone(),
            item_id: item_id.clone(),
            output_index,
            arguments: arguments.to_string(),
        });
        emitter.output_item_done(&self.response.id, output_index, completed);
    }

    fn current_message_text(&self) -> Option<String> {
        let item_id = self.normalized.message_item_id.as_ref()?;
        let output_index = *self.item_id_to_index.get(item_id)?;
        let OutputItem::Message(message) = self.response.output.get(output_index)? else {
            return None;
        };
        match message.content.first() {
            Some(ContentPart::OutputText(text)) => Some(text.text.clone()),
            _ => None,
        }
    }

    fn current_reasoning_text(&self) -> Option<String> {
        let item_id = self.normalized.reasoning_item_id.as_ref()?;
        let output_index = *self.item_id_to_index.get(item_id)?;
        let OutputItem::Reasoning(reasoning) = self.response.output.get(output_index)? else {
            return None;
        };
        reasoning.content.clone()
    }

    fn output_index_for_item(&mut self, item_id: &str) -> usize {
        self.item_id_to_index
            .get(item_id)
            .copied()
            .unwrap_or_else(|| self.allocate_output_index(item_id))
    }

    fn allocate_output_index(&mut self, item_id: &str) -> usize {
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        self.item_id_to_index.insert(item_id.to_string(), output_index);
        output_index
    }
}

fn normalized_tool_call_arguments(arguments: &str) -> serde_json::Value {
    if arguments.trim().is_empty() {
        return json!({});
    }

    serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()))
}
