//! `MergeStreamState`: streaming accumulator for the native Responses route.

use super::*;

pub(crate) fn merge_session_identity(request: &LLMRequest) -> Option<String> {
    crate::providers::shared::session_lineage_from_prompt_cache_key(request.prompt_cache_key.as_deref())
}

#[derive(Default)]
pub(crate) struct MergeStreamState {
    pub(crate) model: String,
    pub(crate) content: String,
    pub(crate) usage: Option<Usage>,
    pub(crate) request_id: Option<String>,
    pub(crate) final_response: Option<LLMResponse>,
    pub(crate) tool_call_order: Vec<String>,
    pub(crate) seen_tool_calls: HashSet<String>,
    pub(crate) tool_call_names: HashMap<String, String>,
    pub(crate) tool_call_args: HashMap<String, String>,
    /// Keep the latest cumulative snapshot as a rollback buffer until the
    /// stream proves that subsequent snapshots extend it.
    pub(crate) native_snapshot: Option<LLMResponse>,
    pub(crate) native_snapshot_streaming: bool,
    pub(crate) incomplete: bool,
    pub(crate) done: bool,
}

impl MergeStreamState {
    pub(crate) fn new(model: String) -> Self {
        Self { model, ..Default::default() }
    }

    pub(crate) fn has_streamed_output(&self) -> bool {
        !self.content.is_empty()
            || !self.tool_call_order.is_empty()
            || self.usage.is_some()
            || self.native_snapshot.is_some()
    }

    pub(crate) fn reset_for_fallback_restart(&mut self, model: Option<&str>) {
        self.reset_native_snapshot_accumulator();

        if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
            self.model = model.to_owned();
        }
    }

    pub(crate) fn reset_native_snapshot_accumulator(&mut self) {
        self.content.clear();
        self.usage = None;
        self.request_id = None;
        self.final_response = None;
        self.tool_call_order.clear();
        self.seen_tool_calls.clear();
        self.tool_call_names.clear();
        self.tool_call_args.clear();
        self.native_snapshot = None;
        self.native_snapshot_streaming = false;
        self.incomplete = false;
        self.done = false;
    }

    pub(crate) fn remember_native_snapshot(&mut self, response: LLMResponse) {
        if response.usage.is_some() {
            self.usage = response.usage.clone();
        }
        if response.request_id.is_some() {
            self.request_id = response.request_id.clone();
        }
        self.native_snapshot = Some(response);
    }

    pub(crate) fn apply_native_snapshot(
        &mut self,
        response: LLMResponse,
    ) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let mut events = Vec::new();

        if let Some(content) = response.content.clone()
            && let Some(delta) = self.apply_text_fragment(content, true)
            && !delta.is_empty()
        {
            events.push(NormalizedStreamEvent::TextDelta { delta });
        }

        if let Some(tool_calls) = &response.tool_calls {
            for call in tool_calls {
                let call_id = call.id.clone();
                let name = call.tool_name().map(ToOwned::to_owned);
                let arguments = call.raw_input().unwrap_or_default().to_owned();

                if arguments.trim().is_empty() || arguments.trim() == "{}" {
                    if let Some(start) = self.record_tool_start(call_id, name) {
                        events.push(start);
                    }
                    continue;
                }

                events.extend(self.apply_tool_fragment(call_id, name, arguments, true)?);
            }
        }

        if response.usage.is_some() {
            self.usage = response.usage.clone();
        }
        if response.request_id.is_some() {
            self.request_id = response.request_id.clone();
        }
        self.final_response = Some(response);

        Ok(events)
    }

    pub(crate) fn apply_text_fragment(&mut self, fragment: String, full_value: bool) -> Option<String> {
        if full_value {
            let delta = if self.content.is_empty() {
                fragment.clone()
            } else if fragment == self.content {
                String::new()
            } else if fragment.starts_with(&self.content) {
                fragment[self.content.len()..].to_string()
            } else {
                fragment.clone()
            };
            self.content = fragment;
            if delta.is_empty() { None } else { Some(delta) }
        } else {
            self.content.push_str(&fragment);
            if fragment.is_empty() { None } else { Some(fragment) }
        }
    }

    fn record_tool_start(&mut self, call_id: String, name: Option<String>) -> Option<NormalizedStreamEvent> {
        if self.seen_tool_calls.insert(call_id.clone()) {
            self.tool_call_order.push(call_id.clone());
            if let Some(name) = name.clone() {
                self.tool_call_names.insert(call_id.clone(), name.clone());
            }
            Some(NormalizedStreamEvent::ToolCallStart { call_id, name })
        } else {
            if let Some(name) = name {
                self.tool_call_names.entry(call_id).or_insert(name);
            }
            None
        }
    }

    pub(crate) fn record_tool_arguments(
        &mut self,
        call_id: String,
        name: Option<String>,
        fragment: String,
        full_value: bool,
    ) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        self.apply_tool_fragment(call_id, name, fragment, full_value)
    }

    fn apply_tool_fragment(
        &mut self,
        call_id: String,
        name: Option<String>,
        fragment: String,
        full_value: bool,
    ) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let mut events = Vec::new();
        if let Some(start) = self.record_tool_start(call_id.clone(), name) {
            events.push(start);
        }

        let current = self.tool_call_args.entry(call_id.clone()).or_default();
        let delta = if full_value {
            let delta = if current.is_empty() {
                if fragment.is_empty() {
                    None
                } else {
                    Some(fragment.clone())
                }
            } else if fragment == *current {
                None
            } else if fragment.starts_with(current.as_str()) {
                Some(fragment[current.len()..].to_string())
            } else {
                Some(fragment.clone())
            };
            *current = fragment;
            delta
        } else {
            current.push_str(&fragment);
            if fragment.is_empty() { None } else { Some(fragment) }
        };

        if let Some(delta) = delta
            && !delta.is_empty()
        {
            events.push(NormalizedStreamEvent::ToolCallDelta { call_id, delta });
        }

        Ok(events)
    }

    pub(crate) fn record_tool_use_item(
        &mut self,
        item: &Value,
        full_value: bool,
    ) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let mut events = Vec::new();

        if let Some(parts) = item.get("content").and_then(Value::as_array) {
            for part in parts {
                events.extend(self.record_tool_use_part(part, full_value)?);
            }
        } else {
            events.extend(self.record_tool_use_part(item, full_value)?);
        }

        Ok(events)
    }

    fn record_tool_use_part(&mut self, part: &Value, full_value: bool) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
        if !matches!(part_type, "tool_use" | "function_call") && part.get("name").is_none() {
            return Ok(Vec::new());
        }

        let call_id = part
            .get("id")
            .or_else(|| part.get("call_id"))
            .or_else(|| part.get("tool_use_id"))
            .or_else(|| part.get("tool_call_id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(generate_tool_call_id);
        let name = part
            .get("name")
            .or_else(|| part.get("function").and_then(|func| func.get("name")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        let mut events = Vec::new();
        if let Some(start) = self.record_tool_start(call_id.clone(), name) {
            events.push(start);
        }

        if full_value && let Some(input) = part.get("input").or_else(|| part.get("arguments")) {
            let fragment = match input {
                Value::String(text) => text.clone(),
                _ => serde_json::to_string(input).map_err(|e| {
                    provider_error(format!("Failed to serialize Merge Gateway streamed tool input: {e}"))
                })?,
            };
            events.extend(self.apply_tool_fragment(call_id, None, fragment, true)?);
        }

        Ok(events)
    }

    pub(crate) fn synthesize_response(&self) -> LLMResponse {
        let tool_calls = self.build_tool_calls();
        LLMResponse {
            content: if self.content.is_empty() {
                None
            } else {
                Some(self.content.clone())
            },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            model: self.model.clone(),
            usage: self.usage.clone(),
            finish_reason: if self.incomplete {
                FinishReason::Length
            } else if self.tool_call_order.is_empty() {
                FinishReason::Stop
            } else {
                FinishReason::ToolCalls
            },
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: self.request_id.clone(),
            organization_id: None,
            compaction: None,
        }
    }

    fn build_tool_calls(&self) -> Vec<ToolCall> {
        self.tool_call_order
            .iter()
            .map(|call_id| {
                let name = self.tool_call_names.get(call_id).cloned().unwrap_or_default();
                let arguments = self.tool_call_args.get(call_id).cloned().unwrap_or_else(|| "{}".to_string());
                ToolCall::function(
                    call_id.clone(),
                    name,
                    if arguments.trim().is_empty() {
                        "{}".to_string()
                    } else {
                        arguments
                    },
                )
            })
            .collect()
    }

    fn merge_into_response(&self, response: &mut LLMResponse) {
        if response.content.as_deref().is_none_or(str::is_empty) && !self.content.is_empty() {
            response.content = Some(self.content.clone());
        } else if let Some(content) = &mut response.content
            && !self.content.is_empty()
            && !content.contains(&self.content)
        {
            content.push_str(&self.content);
        }

        if response.tool_calls.as_ref().is_none_or(Vec::is_empty) && !self.tool_call_order.is_empty() {
            response.tool_calls = Some(self.build_tool_calls());
        }

        if response.usage.is_none() {
            response.usage = self.usage.clone();
        }
        if response.request_id.is_none() {
            response.request_id = self.request_id.clone();
        }
        if response.model.trim().is_empty() {
            response.model = self.model.clone();
        }
        if matches!(response.finish_reason, FinishReason::Stop)
            && response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
        {
            response.finish_reason = FinishReason::ToolCalls;
        }
        if self.incomplete && matches!(response.finish_reason, FinishReason::Stop) {
            response.finish_reason = FinishReason::Length;
        }
    }

    pub(crate) fn finish(mut self) -> Result<LLMResponse, LLMError> {
        let mut response = self.final_response.take().unwrap_or_else(|| self.synthesize_response());
        self.merge_into_response(&mut response);
        Ok(response)
    }
}
