use super::*;

impl MergeGatewayProvider {
    pub(super) fn handle_native_stream_payload(
        raw_event: &str,
        payload_text: &str,
        state: &mut MergeStreamState,
    ) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let payload: Value = serde_json::from_str(payload_text)
            .map_err(|e| provider_error(format!("Invalid Merge Gateway SSE payload: {e}")))?;
        let event_name = Self::merge_event_name(raw_event, &payload);
        let data = payload.get("data").cloned().unwrap_or(payload.clone());
        let mut events = Vec::new();

        let fallback_restart = data
            .get("fallback_restart")
            .or_else(|| payload.get("fallback_restart"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if fallback_restart {
            let model = data.get("model").or_else(|| payload.get("model")).and_then(Value::as_str);
            state.reset_for_fallback_restart(model);
            return Ok(events);
        }

        if event_name.is_empty() {
            if data.get("response").is_some()
                || data.get("output").is_some()
                || data.get("content").is_some()
                || data.get("text").is_some()
            {
                let response_value = data.get("response").cloned().unwrap_or_else(|| data.clone());
                let response = Self::parse_native_response_payload(response_value, state.model.clone())?;
                state.native_snapshot = None;
                events.extend(state.apply_native_snapshot(response)?);
                state.done = true;
            }
            return Ok(events);
        }

        match event_name.as_str() {
            // Native `/v1/responses` streams cumulative snapshots. The
            // OpenAI-compatible `/v1/openai/responses` surface uses the
            // delta-oriented events handled below, so keep both protocols
            // explicit at this boundary.
            "response.stream" | "response.done" => {
                let response_value = data.get("response").cloned().or_else(|| {
                    if data.get("output").is_some() {
                        Some(data.clone())
                    } else {
                        None
                    }
                });

                let response = match response_value {
                    Some(response_value) => Self::parse_native_response_payload(response_value, state.model.clone())?,
                    None if event_name == "response.done" => state
                        .native_snapshot
                        .take()
                        .ok_or_else(|| provider_error("Merge Gateway response.done event was missing the response"))?,
                    None => return Ok(events),
                };

                if event_name == "response.done" {
                    state.native_snapshot = None;
                    events.extend(state.apply_native_snapshot(response)?);
                    state.done = true;
                } else {
                    if let Some(previous) = state.native_snapshot.take() {
                        if Self::native_snapshots_are_cumulative(&previous, &response) {
                            if !state.native_snapshot_streaming {
                                events.extend(state.apply_native_snapshot(previous)?);
                                state.native_snapshot_streaming = true;
                            }
                            events.extend(state.apply_native_snapshot(response.clone())?);
                        } else if state.native_snapshot_streaming {
                            // A provider replacement cannot retract already-emitted
                            // deltas, but it must not leak the old snapshot into the
                            // terminal response or subsequent deltas.
                            state.reset_native_snapshot_accumulator();
                        }
                    }
                    state.remember_native_snapshot(response);
                }
            }
            "response.output_text.delta" | "response.output_text.done" => {
                if let Some(fragment) = Self::stream_text_fragment(&data) {
                    if let Some(delta) = state.apply_text_fragment(fragment, event_name.ends_with(".done")) {
                        if !delta.is_empty() {
                            events.push(NormalizedStreamEvent::TextDelta { delta });
                        }
                    }
                }
            }
            "response.output_item.added" => {
                if let Some(item) = Self::stream_output_item(&data) {
                    events.extend(state.record_tool_use_item(item, false)?);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = Self::stream_output_item(&data) {
                    events.extend(state.record_tool_use_item(item, true)?);
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some((call_id, name, fragment)) = Self::stream_tool_arguments(&data) {
                    events.extend(state.record_tool_arguments(call_id, name, fragment, false)?);
                }
            }
            "response.function_call_arguments.done" => {
                if let Some((call_id, name, fragment)) = Self::stream_tool_arguments(&data) {
                    events.extend(state.record_tool_arguments(call_id, name, fragment, true)?);
                }
            }
            "response.usage" => {
                let usage_value = data.get("usage").unwrap_or(&data);
                state.usage = Self::parse_native_usage(Some(usage_value));
            }
            "response.completed" | "response.incomplete" => {
                if event_name == "response.incomplete" {
                    state.incomplete = true;
                }
                let response_value = data.get("response").cloned().or_else(|| {
                    if data.get("output").is_some() {
                        Some(data.clone())
                    } else {
                        None
                    }
                });

                if let Some(response_value) = response_value {
                    match Self::parse_native_response_payload(response_value, state.model.clone()) {
                        Ok(response) => {
                            state.final_response = Some(response);
                            state.done = true;
                        }
                        Err(err) if state.has_streamed_output() => {
                            state.request_id = Self::extract_request_id(&data);
                            state.done = true;
                            if matches!(event_name.as_str(), "response.completed") {
                                return Err(err);
                            }
                        }
                        Err(err) => return Err(err),
                    }
                } else if state.has_streamed_output() {
                    state.request_id = Self::extract_request_id(&data);
                    state.done = true;
                }
            }
            "response.failed" | "response.error" | "error" => {
                let message = data
                    .get("error")
                    .and_then(Value::as_object)
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| data.get("error").and_then(Value::as_str))
                    .or_else(|| data.get("message").and_then(Value::as_str))
                    .unwrap_or("Merge Gateway stream failed")
                    .to_string();
                return Err(provider_error(message));
            }
            _ => {}
        }

        if state.done && state.final_response.is_none() && !state.has_streamed_output() {
            state.final_response = Some(state.synthesize_response());
        }

        Ok(events)
    }

    fn merge_event_name(raw_event: &str, payload: &Value) -> String {
        for line in raw_event.lines() {
            if let Some(event) = line.strip_prefix("event:") {
                return event.trim().to_string();
            }
        }
        if let Some(event) = payload.get("event").and_then(Value::as_str) {
            return event.to_string();
        }
        if let Some(event) = payload.get("type").and_then(Value::as_str) {
            return event.to_string();
        }
        if let Some(event) = payload.get("object").and_then(Value::as_str)
            && (event.starts_with("response.") || event == "error")
        {
            return event.to_string();
        }
        String::new()
    }

    fn native_snapshots_are_cumulative(previous: &LLMResponse, current: &LLMResponse) -> bool {
        let content_is_cumulative = match (previous.content.as_deref(), current.content.as_deref()) {
            (Some(previous), Some(current)) => current.starts_with(previous),
            (Some(_), None) => false,
            (None, _) => true,
        };
        if !content_is_cumulative {
            return false;
        }

        let previous_calls = previous.tool_calls.as_deref().unwrap_or_default();
        let current_calls = current.tool_calls.as_deref().unwrap_or_default();
        previous_calls.iter().all(|previous_call| {
            let Some(current_call) = current_calls.iter().find(|call| call.id == previous_call.id) else {
                return false;
            };

            let previous_name = previous_call.tool_name().unwrap_or_default();
            let current_name = current_call.tool_name().unwrap_or_default();
            if previous_name != current_name {
                return false;
            }

            let previous_arguments = previous_call.raw_input().unwrap_or_default();
            previous_arguments.is_empty()
                || previous_arguments == "{}"
                || current_call.raw_input().unwrap_or_default().starts_with(previous_arguments)
        })
    }

    fn stream_text_fragment(data: &Value) -> Option<String> {
        data.get("delta")
            .or_else(|| data.get("output_text"))
            .or_else(|| data.get("text"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }

    fn stream_output_item(data: &Value) -> Option<&Value> {
        data.get("output_item").or_else(|| data.get("item")).or_else(|| {
            if data.get("type").is_some() || data.get("content").is_some() {
                Some(data)
            } else {
                None
            }
        })
    }

    fn stream_tool_arguments(data: &Value) -> Option<(String, Option<String>, String)> {
        let call_id = data
            .get("call_id")
            .or_else(|| data.get("item_id"))
            .or_else(|| data.get("tool_use_id"))
            .or_else(|| data.get("tool_call_id"))
            .or_else(|| data.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)?;
        let name = data.get("name").and_then(Value::as_str).map(ToOwned::to_owned).or_else(|| {
            Self::stream_output_item(data)
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
        let fragment = data
            .get("delta")
            .or_else(|| data.get("arguments"))
            .or_else(|| data.get("input"))
            .and_then(|value| Self::value_to_arguments(value).ok())
            .unwrap_or_default();
        Some((call_id, name, fragment))
    }

    pub(super) fn value_to_arguments(value: &Value) -> Result<String, LLMError> {
        match value {
            Value::String(text) => Ok(text.clone()),
            _ => serde_json::to_string(value)
                .map_err(|e| provider_error(format!("Failed to serialize Merge Gateway tool arguments: {e}"))),
        }
    }
}
