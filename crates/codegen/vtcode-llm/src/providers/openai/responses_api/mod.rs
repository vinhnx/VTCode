use crate::error_display;
use crate::provider::{
    AssistantPhase, ContentPart, FinishReason, LLMError, LLMRequest, LLMResponse, MessageContent, MessageRole,
    ToolCall, Usage,
};
use crate::providers::common::append_normalized_reasoning_detail_items;
use crate::providers::openai::types::{InstructionSegmentKind, MAX_COMPLETION_TOKENS_FIELD, OpenAIResponsesPayload};
use crate::providers::shared::{
    RESPONSES_COMPLETION_TOKEN_KEYS, RESPONSES_PROMPT_TOKEN_KEYS, collect_tool_references_from_tool_search_output,
    function_output_value_from_message_content, parse_cache_write_tokens_from_usage,
    parse_cached_prompt_tokens_from_usage, tool_result_content_from_message_content, usage_u32_from_keys,
};
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::path::Path;

mod cache_diagnostics;

// OpenAI's file-input guide uses MIME-prefixed data URLs for inline files.
// Normalize at the wire boundary so raw base64 in existing history also works.
fn inline_file_data_url<'a>(filename: Option<&str>, data: &'a str) -> Cow<'a, str> {
    let data = data.trim();
    if data.starts_with("data:") {
        return Cow::Borrowed(data);
    }

    let extension = filename
        .and_then(|name| Path::new(name).extension())
        .and_then(|ext| ext.to_str());
    let mime_type = match extension.unwrap_or_default().to_ascii_lowercase().as_str() {
        "pdf" => "application/pdf",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "xml" => "text/xml",
        "csv" => "text/csv",
        "tsv" => "text/tsv",
        "iif" => "text/x-iif",
        "doc" | "dot" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "odt" => "application/vnd.oasis.opendocument.text",
        "rtf" => "application/rtf",
        "ppt" | "pps" | "pot" | "ppa" | "pwz" | "wiz" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xls" | "xla" | "xlb" | "xlc" | "xlm" | "xlt" | "xlw" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "text/plain",
    };
    Cow::Owned(format!("data:{mime_type};base64,{data}"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponsesToolCallKind {
    Function,
    Custom,
}

fn responses_tool_call_kind(call: &ToolCall) -> ResponsesToolCallKind {
    if call.is_custom() {
        ResponsesToolCallKind::Custom
    } else {
        ResponsesToolCallKind::Function
    }
}

fn responses_tool_call_output_type(kind: ResponsesToolCallKind) -> &'static str {
    match kind {
        ResponsesToolCallKind::Function => "function_call_output",
        ResponsesToolCallKind::Custom => "custom_tool_call_output",
    }
}

fn parse_responses_tool_call(item: &Value) -> Option<ToolCall> {
    let item_type = item.get("type").and_then(|value| value.as_str()).unwrap_or("");
    if item_type == "custom_tool_call" {
        let call_id = item
            .get("call_id")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("id").and_then(|v| v.as_str()))
            .unwrap_or("");
        let name = item.get("name").and_then(|value| value.as_str())?;
        let input = item.get("input").and_then(|value| value.as_str()).unwrap_or_default();
        return Some(ToolCall::custom(call_id.to_string(), name.to_string(), input.to_string()));
    }

    parse_responses_function_tool_call(item)
}

fn parse_responses_function_tool_call(item: &Value) -> Option<ToolCall> {
    let call_id = item
        .get("call_id")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("id").and_then(|v| v.as_str()))
        .unwrap_or("");
    let function_obj = item.get("function").and_then(|v| v.as_object());
    let namespace = item
        .get("namespace")
        .and_then(|v| v.as_str())
        .or_else(|| function_obj.and_then(|f| f.get("namespace").and_then(|n| n.as_str())))
        .map(ToOwned::to_owned);
    let name = function_obj
        .and_then(|f| f.get("name").and_then(|n| n.as_str()))
        .or_else(|| item.get("name").and_then(|n| n.as_str()))?;
    let arguments = function_obj.and_then(|f| f.get("arguments")).or_else(|| item.get("arguments"));

    let serialized = arguments.map_or("{}".to_owned(), |args| {
        if args.is_string() {
            args.as_str().unwrap_or("{}").to_string()
        } else {
            args.to_string()
        }
    });

    Some(ToolCall::function_with_namespace(call_id.to_string(), namespace, name.to_string(), serialized))
}

fn append_user_content_parts(content_parts: &mut Vec<Value>, message_content: &MessageContent) {
    match message_content {
        MessageContent::Text(text) => {
            if !text.trim().is_empty() {
                content_parts.push(json!({
                    "type": "input_text",
                    "text": text
                }));
            }
        }
        MessageContent::Parts(parts) => {
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if !text.trim().is_empty() {
                            content_parts.push(json!({
                                "type": "input_text",
                                "text": text
                            }));
                        }
                    }
                    ContentPart::Image { data, mime_type, .. } => {
                        // Providers accept only JPEG/PNG/GIF/WebP. Anything else
                        // (notably SVG auto-attached from quoted paths in diffs)
                        // fails the whole request with 400, so drop it here
                        // instead of letting one bad part poison the turn.
                        if !vtcode_commons::image::is_supported_image_mime_type(mime_type) {
                            tracing::warn!(
                                mime_type = %mime_type,
                                "dropping unsupported image MIME type for Responses API"
                            );
                            continue;
                        }
                        let image_url = {
                            let mut s = String::with_capacity(13 + mime_type.len() + data.len());
                            s.push_str("data:");
                            s.push_str(mime_type);
                            s.push_str(";base64,");
                            s.push_str(data);
                            s
                        };
                        content_parts.push(json!({
                            "type": "input_image",
                            "image_url": image_url
                        }));
                    }
                    ContentPart::File { filename, file_id, file_data, file_url, .. } => {
                        if file_id.is_none() && file_data.is_none() && file_url.is_none() {
                            continue;
                        }

                        let mut file_part = json!({
                            "type": "input_file"
                        });
                        if let Value::Object(ref mut map) = file_part {
                            if let Some(name) = filename {
                                map.insert("filename".to_owned(), json!(name));
                            }
                            if let Some(id) = file_id {
                                map.insert("file_id".to_owned(), json!(id));
                            }
                            if let Some(data) = file_data {
                                map.insert(
                                    "file_data".to_owned(),
                                    json!(inline_file_data_url(filename.as_deref(), data)),
                                );
                            }
                            if let Some(url) = file_url {
                                map.insert("file_url".to_owned(), json!(url));
                            }
                        }
                        content_parts.push(file_part);
                    }
                }
            }
        }
    }
}

fn assistant_input_item(content_parts: Vec<Value>, phase: Option<AssistantPhase>) -> Value {
    let mut item = json!({
        "role": "assistant",
        "content": content_parts
    });

    if let Some(phase) = phase
        && let Value::Object(ref mut map) = item
    {
        map.insert("phase".to_string(), json!(phase.as_str()));
    }

    item
}

fn append_assistant_text_to_instructions(
    instructions_segments: &mut Vec<(InstructionSegmentKind, String)>,
    text: &str,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    let mut s = String::with_capacity(30 + trimmed.len());
    s.push_str("Previous assistant response:\n");
    s.push_str(trimmed);
    instructions_segments.push((InstructionSegmentKind::FoldedHistory, s));
}

fn append_output_item_text(value: &Value, text: &mut String) {
    if let Some(part_text) = value.get("text").and_then(|value| value.as_str()) {
        text.push_str(part_text);
    }
    if let Some(part_output) = value.get("output").and_then(|value| value.as_str()) {
        text.push_str(part_output);
    }
    if let Some(refusal) = value.get("refusal").and_then(|value| value.as_str()) {
        text.push_str(refusal);
    }

    match value {
        Value::String(value) => text.push_str(value),
        Value::Array(parts) => {
            for part in parts {
                append_output_item_text(part, text);
            }
        }
        Value::Object(_) => {
            if let Some(content) = value.get("content") {
                append_output_item_text(content, text);
            }
        }
        _ => {}
    }
}

fn append_unique_replay_item(replay_items: &mut Vec<Value>, item: &Value) {
    if !replay_items.iter().any(|existing| existing == item) {
        replay_items.push(item.clone());
    }
}

fn reasoning_entry_needs_replay_preservation(entry: &Value) -> bool {
    let Some(entry_obj) = entry.as_object() else {
        return false;
    };

    entry_obj.contains_key("encrypted_content")
        || entry_obj.keys().any(|key| key != "type" && key != "text" && key != "summary")
}

fn tool_result_history_text(message_content: &MessageContent) -> String {
    let tool_content = tool_result_content_from_message_content(message_content);
    if tool_content.is_empty() {
        return String::new();
    }

    let mut text = String::new();
    for item in &tool_content {
        append_output_item_text(item, &mut text);
    }

    let trimmed = text.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    Value::Array(tool_content).to_string()
}

fn append_tool_result_to_instructions(
    instructions_segments: &mut Vec<(InstructionSegmentKind, String)>,
    tool_call_id: Option<&str>,
    message_content: &MessageContent,
) {
    let text = tool_result_history_text(message_content);
    if text.is_empty() {
        return;
    }

    let (heading_str, heading_cap) = match tool_call_id {
        Some(id) if !id.is_empty() => (None, 26 + id.len()),
        _ => (Some("Previous tool result:"), 0),
    };
    let mut s = String::with_capacity(heading_str.map_or(heading_cap, |h| h.len()) + 1 + text.len());
    match heading_str {
        Some(h) => s.push_str(h),
        None => {
            s.push_str("Previous tool result (");
            s.push_str(tool_call_id.expect("heading_str is None only when tool_call_id is Some"));
            s.push_str("):");
        }
    }
    s.push('\n');
    s.push_str(&text);
    instructions_segments.push((InstructionSegmentKind::FoldedHistory, s));
}

/// Parse a server completion and record its served tier once. Replay parsing
/// uses `parse_responses_payload` directly and must not emit completion evidence.
pub(crate) fn parse_responses_completion(
    response_json: Value,
    model: String,
    include_cached_prompt_metrics: bool,
) -> Result<LLMResponse, LLMError> {
    let cache_diagnostics = cache_diagnostics::parse(&response_json);
    let service_tier = response_json
        .get("service_tier")
        .and_then(Value::as_str)
        .filter(|tier| matches!(*tier, "default" | "standard" | "auto" | "flex" | "priority" | "fast" | "ultrafast"))
        .map(compact_str::CompactString::new);
    // Streaming can deliver usable deltas with an empty authoritative output.
    // Its existing fallback reconstructs the answer after this parser rejects
    // the empty array; cache evidence belongs to the completion envelope and
    // must be recorded independently of that content reconstruction.
    if let Some(diagnostics) = cache_diagnostics {
        tracing::info!(
            model = %model,
            cache_comparison_type = diagnostics.kind,
            cache_miss_reason = diagnostics.reason,
            cache_missed_tokens = diagnostics.missed_tokens,
            comparison_reusable_tokens = diagnostics.reusable_tokens,
            "OpenAI Responses prompt cache comparison"
        );
    }
    let response = parse_responses_payload(response_json, model, include_cached_prompt_metrics)?;
    if let Some(service_tier) = service_tier {
        tracing::info!(
            service_tier = service_tier.as_str(),
            "OpenAI Responses completed with server-reported service tier"
        );
    }
    Ok(response)
}

pub(crate) fn parse_responses_payload(
    response_json: Value,
    model: String,
    include_cached_prompt_metrics: bool,
) -> Result<LLMResponse, LLMError> {
    let output = response_json.get("output").and_then(|value| value.as_array()).ok_or_else(|| {
        let formatted_error =
            error_display::format_llm_error("OpenAI", "Invalid Responses API format: missing output array");
        LLMError::Provider { message: formatted_error, metadata: None }
    })?;

    if output.is_empty() {
        let formatted_error = error_display::format_llm_error("OpenAI", "No output in response");
        return Err(LLMError::Provider { message: formatted_error, metadata: None });
    }

    let mut content_fragments: Vec<String> = Vec::new();
    let mut reasoning_text_fragments: Vec<String> = Vec::new();
    let mut reasoning_items: Vec<Value> = Vec::new();
    let mut tool_calls_vec: Vec<ToolCall> = Vec::new();
    let mut tool_references: Vec<String> = Vec::new();

    for item in output {
        let item_type = item.get("type").and_then(|value| value.as_str()).unwrap_or("");

        match item_type {
            "message" => {
                if let Some(content_array) = item.get("content").and_then(|value| value.as_array()) {
                    for entry in content_array {
                        let entry_type = entry.get("type").and_then(|value| value.as_str()).unwrap_or("");

                        match entry_type {
                            "text" | "output_text" => {
                                if let Some(text) = entry.get("text").and_then(|value| value.as_str())
                                    && !text.is_empty()
                                {
                                    content_fragments.push(text.to_string());
                                }
                            }
                            "reasoning" => {
                                if let Some(text) = entry.get("text").and_then(|value| value.as_str())
                                    && !text.is_empty()
                                {
                                    reasoning_text_fragments.push(text.to_string());
                                }
                                if reasoning_entry_needs_replay_preservation(entry) {
                                    append_unique_replay_item(&mut reasoning_items, entry);
                                }
                            }
                            "function_call" | "tool_call" | "custom_tool_call" => {
                                if let Some(call) = parse_responses_tool_call(entry) {
                                    tool_calls_vec.push(call);
                                }
                            }
                            "refusal" => {
                                if let Some(refusal_text) = entry.get("refusal").and_then(|value| value.as_str())
                                    && !refusal_text.is_empty()
                                {
                                    content_fragments.push(format!("[Refusal: {refusal_text}]"));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "function_call" | "tool_call" | "custom_tool_call" => {
                if let Some(call) = parse_responses_tool_call(item) {
                    tool_calls_vec.push(call);
                }
            }
            "tool_search_output" => {
                collect_tool_references_from_tool_search_output(item, &mut tool_references);
            }
            "web_search" | "file_search" => {
                if let Some(results) = item.get("results").and_then(|r| r.as_array()) {
                    let citations: Vec<String> = results
                        .iter()
                        .filter_map(|r| {
                            let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled");
                            let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
                            if !url.is_empty() {
                                Some(format!("[{title}]({url})"))
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !citations.is_empty() {
                        content_fragments.push(format!("\n\nSources:\n{}", citations.join("\n")));
                    }
                }
            }
            "reasoning" => {
                append_unique_replay_item(&mut reasoning_items, item);

                if let Some(summary_array) = item.get("summary").and_then(|v| v.as_array()) {
                    for summary_part in summary_array {
                        if let Some(text) = summary_part.get("text").and_then(|v| v.as_str())
                            && !text.is_empty()
                        {
                            reasoning_text_fragments.push(text.to_string());
                        }
                    }
                }
            }
            _ => {
                if !item_type.is_empty() {
                    append_unique_replay_item(&mut reasoning_items, item);
                }
            }
        }
    }

    let content = if content_fragments.is_empty() {
        None
    } else {
        Some(content_fragments.join(""))
    };

    let reasoning = if reasoning_text_fragments.is_empty() {
        None
    } else {
        Some(reasoning_text_fragments.join("\n\n"))
    };

    let reasoning_details = if reasoning_items.is_empty() {
        None
    } else {
        Some(reasoning_items.into_iter().map(|v| v.to_string()).collect())
    };

    let finish_reason = if !tool_calls_vec.is_empty() {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    };

    let tool_calls = if tool_calls_vec.is_empty() {
        None
    } else {
        Some(tool_calls_vec)
    };

    let usage = response_json.get("usage").map(|usage_value| {
        let cached_prompt_tokens = parse_cached_prompt_tokens_from_usage(usage_value, include_cached_prompt_metrics);
        let cache_creation_tokens = parse_cache_write_tokens_from_usage(usage_value, include_cached_prompt_metrics);

        Usage {
            prompt_tokens: usage_u32_from_keys(usage_value, RESPONSES_PROMPT_TOKEN_KEYS),
            completion_tokens: usage_u32_from_keys(usage_value, RESPONSES_COMPLETION_TOKEN_KEYS),
            total_tokens: usage_u32_from_keys(usage_value, &["total_tokens"]),
            cached_prompt_tokens,
            cache_creation_tokens,
            cache_read_tokens: None,
            iterations: None,
        }
    });

    Ok(LLMResponse {
        content,
        tool_calls,
        model,
        usage,
        finish_reason,
        reasoning,
        reasoning_details,
        tool_references,
        request_id: response_json
            .get("id")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
            .or_else(|| {
                response_json
                    .get("request_id")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned)
            }),
        organization_id: None,
        compaction: None,
    })
}

/// Build a standard (non-Codex) Responses API payload.
pub(crate) fn build_standard_responses_payload(
    request: &LLMRequest,
    include_structured_history_in_input: bool,
) -> Result<OpenAIResponsesPayload, LLMError> {
    let mut input = Vec::new();
    let mut active_tool_calls: HashMap<String, ResponsesToolCallKind> = HashMap::new();
    let mut pending_tool_call_order: Vec<String> = Vec::new();
    let mut deferred_tool_outputs: HashMap<String, Value> = HashMap::new();
    let mut instructions_segments: Vec<(InstructionSegmentKind, String)> = Vec::new();

    if let Some(system_prompt) = &request.system_prompt {
        let trimmed = system_prompt.trim();
        if !trimmed.is_empty() {
            instructions_segments.push((InstructionSegmentKind::SystemPrompt, trimmed.to_string()));
        }
    }

    for msg in request.messages.iter() {
        match msg.role {
            MessageRole::System => {
                let content_text = msg.content.as_text();
                let trimmed = content_text.trim();
                if !trimmed.is_empty() {
                    instructions_segments.push((InstructionSegmentKind::HistorySystem, trimmed.to_string()));
                }
            }
            MessageRole::User => {
                let mut content_parts: Vec<Value> = Vec::new();
                append_user_content_parts(&mut content_parts, &msg.content);

                if !content_parts.is_empty() {
                    input.push(json!({
                        "role": "user",
                        "content": content_parts
                    }));
                }
            }
            MessageRole::Assistant => {
                // Inject any persisted reasoning items from previous turns
                if include_structured_history_in_input && let Some(reasoning_details) = &msg.reasoning_details {
                    append_normalized_reasoning_detail_items(&mut input, reasoning_details);
                }

                let mut content_parts = Vec::new();
                let mut tool_call_items = Vec::new();
                if !msg.content.is_empty() {
                    if include_structured_history_in_input {
                        content_parts.push(json!({
                            "type": "output_text",
                            "text": msg.content.as_text()
                        }));
                    } else {
                        append_assistant_text_to_instructions(&mut instructions_segments, &msg.content.as_text());
                    }
                }

                if let Some(tool_calls) = &msg.tool_calls {
                    for call in tool_calls {
                        if let Some(ref func) = call.function {
                            let call_kind = responses_tool_call_kind(call);
                            if active_tool_calls.insert(call.id.clone(), call_kind).is_none() {
                                pending_tool_call_order.push(call.id.clone());
                            }
                            if include_structured_history_in_input {
                                let replay_item = match call_kind {
                                    ResponsesToolCallKind::Function => json!({
                                        "type": "function_call",
                                        "call_id": &call.id,
                                        "name": &func.name,
                                        "arguments": &func.arguments
                                    }),
                                    ResponsesToolCallKind::Custom => json!({
                                        "type": "custom_tool_call",
                                        "call_id": &call.id,
                                        "name": &func.name,
                                        "input": call.text.as_deref().unwrap_or(&func.arguments)
                                    }),
                                };
                                tool_call_items.push(replay_item);
                                if let Some(deferred_output) = deferred_tool_outputs.remove(&call.id) {
                                    active_tool_calls.remove(&call.id);
                                    tool_call_items.push(json!({
                                        "type": responses_tool_call_output_type(call_kind),
                                        "call_id": &call.id,
                                        "output": deferred_output,
                                    }));
                                }
                            }
                        }
                    }
                }

                if !content_parts.is_empty() {
                    input.push(assistant_input_item(content_parts, msg.phase));
                }
                input.extend(tool_call_items);
            }
            MessageRole::Tool => {
                let tool_call_id = msg.tool_call_id.as_ref().ok_or_else(|| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        "Tool messages must include tool_call_id for Responses API",
                    );
                    LLMError::InvalidRequest { message: formatted_error, metadata: None }
                })?;

                if !active_tool_calls.contains_key(tool_call_id) {
                    if include_structured_history_in_input {
                        deferred_tool_outputs
                            .insert(tool_call_id.clone(), function_output_value_from_message_content(&msg.content));
                    }
                    continue;
                }

                if !include_structured_history_in_input {
                    append_tool_result_to_instructions(&mut instructions_segments, Some(tool_call_id), &msg.content);
                    active_tool_calls.remove(tool_call_id);
                    continue;
                }

                let call_kind = active_tool_calls
                    .remove(tool_call_id)
                    .unwrap_or(ResponsesToolCallKind::Function);
                input.push(json!({
                    "type": responses_tool_call_output_type(call_kind),
                    "call_id": tool_call_id,
                    "output": function_output_value_from_message_content(&msg.content),
                }));
            }
        }
    }

    // Responses API requires every tool call item to have a paired output item.
    // Synthesize any missing outputs so replay cannot
    // fail on partially paired history.
    if include_structured_history_in_input {
        for call_id in pending_tool_call_order {
            let Some(call_kind) = active_tool_calls.remove(&call_id) else {
                continue;
            };
            input.push(json!({
                "type": responses_tool_call_output_type(call_kind),
                "call_id": call_id,
                "output": "aborted",
            }));
        }
    }

    let (instructions, instruction_segments) = finalize_instruction_segments(instructions_segments);

    Ok(OpenAIResponsesPayload { input, instructions, instruction_segments })
}

/// Join labeled instruction segments into the wire string plus provenance.
///
/// Returns `(None, None)` when there are no segments. Provenance is always
/// emitted with the segments: dynamic sections can live inside the system
/// prompt itself, so volatile-segment detection belongs to the consumer
/// (`separate_dynamic_instructions`), not here.
fn finalize_instruction_segments(
    segments: Vec<(InstructionSegmentKind, String)>,
) -> (Option<String>, Option<Vec<(InstructionSegmentKind, String)>>) {
    if segments.is_empty() {
        return (None, None);
    }
    let joined = segments.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>().join("\n\n");
    (Some(joined), Some(segments))
}

#[cfg(test)]
mod tests;
