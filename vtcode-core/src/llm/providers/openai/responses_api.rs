use crate::llm::error_display;
use crate::llm::provider::{
    ContentPart, FinishReason, LLMError, LLMRequest, LLMResponse, MessageContent, MessageRole,
    ToolCall, Usage,
};
use crate::llm::providers::openai::types::OpenAIResponsesPayload;
use crate::prompts::system::default_system_prompt;
use serde_json::{Value, json};
use std::collections::HashSet;

fn parse_responses_tool_call(item: &Value) -> Option<ToolCall> {
    let call_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let function_obj = item.get("function").and_then(|v| v.as_object());
    let name = function_obj.and_then(|f| f.get("name").and_then(|n| n.as_str()))?;
    let arguments = function_obj.and_then(|f| f.get("arguments"));

    let serialized = arguments.map_or("{}".to_owned(), |args| {
        if args.is_string() {
            args.as_str().unwrap_or("{}").to_string()
        } else {
            args.to_string()
        }
    });

    Some(ToolCall::function(
        call_id.to_string(),
        name.to_string(),
        serialized,
    ))
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
                    ContentPart::Image {
                        data, mime_type, ..
                    } => {
                        content_parts.push(json!({
                            "type": "input_image",
                            "image_url": format!("data:{};base64,{}", mime_type, data)
                        }));
                    }
                    ContentPart::File {
                        filename,
                        file_id,
                        file_data,
                        file_url,
                        ..
                    } => {
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
                                map.insert("file_data".to_owned(), json!(data));
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

pub fn parse_responses_payload(
    response_json: Value,
    model: String,
    include_cached_prompt_metrics: bool,
) -> Result<LLMResponse, LLMError> {
    let output = response_json
        .get("output")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "Invalid Responses API format: missing output array",
            );
            LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

    if output.is_empty() {
        let formatted_error = error_display::format_llm_error("OpenAI", "No output in response");
        return Err(LLMError::Provider {
            message: formatted_error,
            metadata: None,
        });
    }

    let mut content_fragments: Vec<String> = Vec::new();
    let mut reasoning_text_fragments: Vec<String> = Vec::new();
    let mut reasoning_items: Vec<Value> = Vec::new();
    let mut tool_calls_vec: Vec<ToolCall> = Vec::new();

    for item in output {
        let item_type = item
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match item_type {
            "message" => {
                if let Some(content_array) = item.get("content").and_then(|value| value.as_array())
                {
                    for entry in content_array {
                        let entry_type = entry
                            .get("type")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");

                        match entry_type {
                            "text" | "output_text" => {
                                if let Some(text) =
                                    entry.get("text").and_then(|value| value.as_str())
                                    && !text.is_empty()
                                {
                                    content_fragments.push(text.to_string());
                                }
                            }
                            "reasoning" => {
                                if let Some(text) =
                                    entry.get("text").and_then(|value| value.as_str())
                                    && !text.is_empty()
                                {
                                    reasoning_text_fragments.push(text.to_string());
                                }
                            }
                            "function_call" | "tool_call" => {
                                if let Some(call) = parse_responses_tool_call(entry) {
                                    tool_calls_vec.push(call);
                                }
                            }
                            "refusal" => {
                                if let Some(refusal_text) =
                                    entry.get("refusal").and_then(|value| value.as_str())
                                    && !refusal_text.is_empty()
                                {
                                    content_fragments.push(format!("[Refusal: {}]", refusal_text));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "function_call" | "tool_call" => {
                if let Some(call) = parse_responses_tool_call(item) {
                    tool_calls_vec.push(call);
                }
            }
            "web_search" | "file_search" => {
                if let Some(results) = item.get("results").and_then(|r| r.as_array()) {
                    let citations: Vec<String> = results
                        .iter()
                        .filter_map(|r| {
                            let title = r
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Untitled");
                            let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
                            if !url.is_empty() {
                                Some(format!("[{}]({})", title, url))
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
                reasoning_items.push(item.clone());

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
            _ => {}
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
        let cached_prompt_tokens = if include_cached_prompt_metrics {
            usage_value
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .or_else(|| usage_value.get("prompt_cache_hit_tokens"))
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok())
        } else {
            None
        };

        Usage {
            prompt_tokens: usage_value
                .get("input_tokens")
                .or_else(|| usage_value.get("prompt_tokens"))
                .and_then(|pt| pt.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            completion_tokens: usage_value
                .get("output_tokens")
                .or_else(|| usage_value.get("completion_tokens"))
                .and_then(|ct| ct.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|tt| tt.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            cached_prompt_tokens,
            cache_creation_tokens: None,
            cache_read_tokens: None,
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
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
    })
}

/// Build a standard (non-Codex) Responses API payload.
pub fn build_standard_responses_payload(
    request: &LLMRequest,
) -> Result<OpenAIResponsesPayload, LLMError> {
    let mut input = Vec::new();
    let mut active_tool_call_ids: HashSet<String> = HashSet::new();
    let mut instructions_segments = Vec::new();

    if let Some(system_prompt) = &request.system_prompt {
        let trimmed = system_prompt.trim();
        if !trimmed.is_empty() {
            instructions_segments.push(trimmed.to_string());
        }
    }

    for msg in &request.messages {
        match msg.role {
            MessageRole::System => {
                let content_text = msg.content.as_text();
                let trimmed = content_text.trim();
                if !trimmed.is_empty() {
                    instructions_segments.push(trimmed.to_string());
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
                if let Some(reasoning_details) = &msg.reasoning_details {
                    for item in reasoning_details {
                        input.push(item.clone());
                    }
                }

                let mut content_parts = Vec::new();
                if !msg.content.is_empty() {
                    content_parts.push(json!({
                        "type": "output_text",
                        "text": msg.content.as_text()
                    }));
                }

                if let Some(tool_calls) = &msg.tool_calls {
                    for call in tool_calls {
                        if let Some(ref func) = call.function {
                            active_tool_call_ids.insert(call.id.clone());
                            content_parts.push(json!({
                                "type": "tool_call",
                                "id": &call.id,
                                "function": {
                                    "name": &func.name,
                                    "arguments": &func.arguments
                                }
                            }));
                        }
                    }
                }

                if !content_parts.is_empty() {
                    input.push(json!({
                        "role": "assistant",
                        "content": content_parts
                    }));
                }
            }
            MessageRole::Tool => {
                let tool_call_id = msg.tool_call_id.as_ref().ok_or_else(|| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        "Tool messages must include tool_call_id for Responses API",
                    );
                    LLMError::InvalidRequest {
                        message: formatted_error,
                        metadata: None,
                    }
                })?;

                if !active_tool_call_ids.contains(tool_call_id) {
                    continue;
                }

                let mut tool_content = Vec::new();
                let content_text = msg.content.as_text();
                if !content_text.trim().is_empty() {
                    tool_content.push(json!({
                        "type": "output_text",
                        "text": content_text
                    }));
                }

                let mut tool_result = json!({
                    "type": "tool_result",
                    "tool_call_id": tool_call_id
                });

                active_tool_call_ids.remove(tool_call_id);

                if !tool_content.is_empty()
                    && let Value::Object(ref mut map) = tool_result
                {
                    map.insert("content".to_owned(), json!(tool_content));
                }

                input.push(json!({
                    "role": "tool",
                    "content": [tool_result]
                }));
            }
        }
    }

    let instructions = if instructions_segments.is_empty() {
        None
    } else {
        Some(instructions_segments.join("\n\n"))
    };

    Ok(OpenAIResponsesPayload {
        input,
        instructions,
    })
}

/// Build a Codex-specific Responses API payload with specialized instructions.
pub fn build_codex_responses_payload(
    request: &LLMRequest,
) -> Result<OpenAIResponsesPayload, LLMError> {
    let mut additional_guidance = Vec::new();

    if let Some(system_prompt) = &request.system_prompt {
        let trimmed = system_prompt.trim();
        if !trimmed.is_empty() {
            additional_guidance.push(trimmed.to_owned());
        }
    }

    let mut input = Vec::new();
    let mut active_tool_call_ids: HashSet<String> = HashSet::new();

    for msg in &request.messages {
        match msg.role {
            MessageRole::System => {
                let content_text = msg.content.as_text();
                let trimmed = content_text.trim();
                if !trimmed.is_empty() {
                    additional_guidance.push(trimmed.to_owned());
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
                // CRITICAL for Codex: Inject any persisted reasoning items from previous turns
                // Codex models experience ~30% performance degradation when reasoning traces
                // are dropped (vs ~3% for standard GPT-5). Always preserve reasoning continuity.
                if let Some(reasoning_details) = &msg.reasoning_details {
                    for item in reasoning_details {
                        input.push(item.clone());
                    }
                }

                let mut content_parts = Vec::new();
                if !msg.content.is_empty() {
                    content_parts.push(json!({
                        "type": "output_text",
                        "text": msg.content.as_text()
                    }));
                }

                if let Some(tool_calls) = &msg.tool_calls {
                    for call in tool_calls {
                        if let Some(ref func) = call.function {
                            active_tool_call_ids.insert(call.id.clone());
                            content_parts.push(json!({
                                "type": "tool_call",
                                "id": &call.id,
                                "function": {
                                    "name": &func.name,
                                    "arguments": &func.arguments
                                }
                            }));
                        }
                    }
                }

                if !content_parts.is_empty() {
                    input.push(json!({
                        "role": "assistant",
                        "content": content_parts
                    }));
                }
            }
            MessageRole::Tool => {
                let tool_call_id = msg.tool_call_id.clone().ok_or_else(|| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        "Tool messages must include tool_call_id for Responses API",
                    );
                    LLMError::InvalidRequest {
                        message: formatted_error,
                        metadata: None,
                    }
                })?;

                if !active_tool_call_ids.contains(&tool_call_id) {
                    continue;
                }

                let mut tool_content = Vec::new();
                let content_text = msg.content.as_text();
                if !content_text.trim().is_empty() {
                    tool_content.push(json!({
                        "type": "output_text",
                        "text": content_text
                    }));
                }

                let mut tool_result = json!({
                    "type": "tool_result",
                    "tool_call_id": tool_call_id
                });

                active_tool_call_ids.remove(&tool_call_id);

                if !tool_content.is_empty()
                    && let Value::Object(ref mut map) = tool_result
                {
                    map.insert("content".to_string(), json!(tool_content));
                }

                input.push(json!({
                    "role": "tool",
                    "content": [tool_result]
                }));
            }
        }
    }

    // Use collected guidance, or fall back to default system prompt if empty
    let instructions = if additional_guidance.is_empty() {
        format!("You are Codex, based on GPT-5. {}", default_system_prompt())
    } else {
        additional_guidance.join("\n\n")
    };

    Ok(OpenAIResponsesPayload {
        input,
        instructions: Some(instructions),
    })
}
