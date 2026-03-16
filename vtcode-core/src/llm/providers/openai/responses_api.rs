use crate::llm::error_display;
use crate::llm::provider::{
    AssistantPhase, ContentPart, FinishReason, LLMError, LLMRequest, LLMResponse, MessageContent,
    MessageRole, ToolCall, Usage,
};
use crate::llm::providers::common::append_normalized_reasoning_detail_items;
use crate::llm::providers::openai::types::OpenAIResponsesPayload;
use crate::llm::providers::shared::{
    function_output_value_from_message_content, tool_result_content_from_message_content,
};
use hashbrown::{HashMap, HashSet};
use serde_json::{Value, json};

fn parse_responses_tool_call(item: &Value) -> Option<ToolCall> {
    let call_id = item
        .get("call_id")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("id").and_then(|v| v.as_str()))
        .unwrap_or("");
    let function_obj = item.get("function").and_then(|v| v.as_object());
    let name = function_obj
        .and_then(|f| f.get("name").and_then(|n| n.as_str()))
        .or_else(|| item.get("name").and_then(|n| n.as_str()))?;
    let arguments = function_obj
        .and_then(|f| f.get("arguments"))
        .or_else(|| item.get("arguments"));

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

fn append_assistant_text_to_instructions(instructions_segments: &mut Vec<String>, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    instructions_segments.push(format!("Previous assistant response:\n{}", trimmed));
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
    instructions_segments: &mut Vec<String>,
    tool_call_id: Option<&str>,
    message_content: &MessageContent,
) {
    let text = tool_result_history_text(message_content);
    if text.is_empty() {
        return;
    }

    let heading = match tool_call_id {
        Some(tool_call_id) if !tool_call_id.is_empty() => {
            format!("Previous tool result ({tool_call_id}):")
        }
        _ => "Previous tool result:".to_string(),
    };
    instructions_segments.push(format!("{heading}\n{text}"));
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
    })
}

/// Build a standard (non-Codex) Responses API payload.
pub fn build_standard_responses_payload(
    request: &LLMRequest,
    include_structured_history_in_input: bool,
) -> Result<OpenAIResponsesPayload, LLMError> {
    let mut input = Vec::new();
    let mut active_tool_call_ids: HashSet<String> = HashSet::new();
    let mut pending_tool_call_order: Vec<String> = Vec::new();
    let mut deferred_tool_outputs: HashMap<String, Value> = HashMap::new();
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
                if include_structured_history_in_input
                    && let Some(reasoning_details) = &msg.reasoning_details
                {
                    append_normalized_reasoning_detail_items(&mut input, reasoning_details);
                }

                let mut content_parts = Vec::new();
                let mut function_call_items = Vec::new();
                if !msg.content.is_empty() {
                    if include_structured_history_in_input {
                        content_parts.push(json!({
                            "type": "output_text",
                            "text": msg.content.as_text()
                        }));
                    } else {
                        append_assistant_text_to_instructions(
                            &mut instructions_segments,
                            &msg.content.as_text(),
                        );
                    }
                }

                if let Some(tool_calls) = &msg.tool_calls {
                    for call in tool_calls {
                        if let Some(ref func) = call.function {
                            if active_tool_call_ids.insert(call.id.clone()) {
                                pending_tool_call_order.push(call.id.clone());
                            }
                            if include_structured_history_in_input {
                                function_call_items.push(json!({
                                    "type": "function_call",
                                    "call_id": &call.id,
                                    "name": &func.name,
                                    "arguments": &func.arguments
                                }));
                                if let Some(deferred_output) =
                                    deferred_tool_outputs.remove(&call.id)
                                {
                                    active_tool_call_ids.remove(&call.id);
                                    function_call_items.push(json!({
                                        "type": "function_call_output",
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
                input.extend(function_call_items);
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
                    if include_structured_history_in_input {
                        deferred_tool_outputs.insert(
                            tool_call_id.clone(),
                            function_output_value_from_message_content(&msg.content),
                        );
                    }
                    continue;
                }

                if !include_structured_history_in_input {
                    append_tool_result_to_instructions(
                        &mut instructions_segments,
                        Some(tool_call_id),
                        &msg.content,
                    );
                    active_tool_call_ids.remove(tool_call_id);
                    continue;
                }

                active_tool_call_ids.remove(tool_call_id);
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": tool_call_id,
                    "output": function_output_value_from_message_content(&msg.content),
                }));
            }
        }
    }

    // Responses API requires every function_call item to have a paired
    // function_call_output. Synthesize any missing outputs so replay cannot
    // fail on partially paired history.
    if include_structured_history_in_input {
        for call_id in pending_tool_call_order {
            if !active_tool_call_ids.contains(&call_id) {
                continue;
            }
            input.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": "aborted",
            }));
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

#[cfg(test)]
mod tests {
    use super::{build_standard_responses_payload, parse_responses_payload};
    use crate::llm::provider::{LLMRequest, Message, ToolCall};
    use serde_json::{Value, json};

    fn assert_multimodal_tool_result(payload: super::OpenAIResponsesPayload) {
        let tool_msg = payload
            .input
            .iter()
            .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call_output"))
            .expect("function_call_output should exist");
        let tool_result_content = tool_msg
            .get("output")
            .and_then(Value::as_array)
            .expect("function_call_output output should be an array");

        assert_eq!(tool_result_content.len(), 2);
        assert_eq!(tool_result_content[0]["type"], "output_text");
        assert_eq!(tool_result_content[0]["text"], "inline image note");
        assert_eq!(tool_result_content[1]["type"], "input_image");
        assert_eq!(
            tool_result_content[1]["image_url"],
            "data:image/png;base64,abc"
        );
    }

    #[test]
    fn standard_payload_normalizes_stringified_reasoning_details_items() {
        let request = LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![
                Message::assistant("answer".to_string()).with_reasoning_details(Some(vec![
                    json!(r#"{"type":"compaction","id":"cmp_1","encrypted_content":"opaque"}"#),
                    json!("plain-text"),
                ])),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, true).expect("payload should build");
        assert_eq!(payload.input.len(), 2);
        assert_eq!(payload.input[0]["type"], "compaction");
    }

    #[test]
    fn standard_payload_preserves_multimodal_tool_result_content() {
        let request = LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_1".to_string(),
                        "view_image".to_string(),
                        "{\"path\":\"./img.png\"}".to_string(),
                    )],
                ),
                Message::tool_response(
                    "call_1".to_string(),
                    r#"[{"type":"input_text","text":"inline image note"},{"type":"input_image","image_url":"data:image/png;base64,abc"}]"#
                        .to_string(),
                ),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, true).expect("payload should build");
        assert_multimodal_tool_result(payload);
    }

    #[test]
    fn standard_payload_uses_responses_function_call_items_for_structured_tool_history() {
        let request = LLMRequest {
            model: "gpt-5.3-codex".to_string(),
            messages: vec![
                Message::user("run cargo fmt".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "direct_unified_exec_1".to_string(),
                        "unified_exec".to_string(),
                        "{\"command\":\"cargo fmt\"}".to_string(),
                    )],
                ),
                Message::tool_response(
                    "direct_unified_exec_1".to_string(),
                    "{\"output\":\"\",\"exit_code\":0,\"backend\":\"pipe\"}".to_string(),
                ),
                Message::assistant("cargo fmt completed successfully.".to_string()),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, true).expect("payload should build");

        assert_eq!(payload.input.len(), 4);
        assert_eq!(payload.input[0]["role"], "user");
        assert_eq!(payload.input[1]["type"], "function_call");
        assert!(payload.input[1].get("id").is_none());
        assert_eq!(payload.input[1]["call_id"], "direct_unified_exec_1");
        assert_eq!(payload.input[2]["type"], "function_call_output");
        assert_eq!(payload.input[2]["call_id"], "direct_unified_exec_1");
        assert_eq!(
            payload.input[2]["output"],
            "{\"output\":\"\",\"exit_code\":0,\"backend\":\"pipe\"}"
        );
        assert_eq!(payload.input[3]["role"], "assistant");
    }

    #[test]
    fn standard_payload_synthesizes_missing_function_call_output_for_orphan_call() {
        let request = LLMRequest {
            model: "gpt-5.3-codex".to_string(),
            messages: vec![
                Message::user("run cargo fmt".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_orphan".to_string(),
                        "unified_exec".to_string(),
                        "{\"command\":\"cargo fmt\"}".to_string(),
                    )],
                ),
                Message::user("continue".to_string()),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, true).expect("payload should build");

        assert!(payload.input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call")
                && item.get("call_id").and_then(Value::as_str) == Some("call_orphan")
        }));
        assert!(payload.input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some("call_orphan")
                && item.get("output").and_then(Value::as_str) == Some("aborted")
        }));
    }

    #[test]
    fn standard_payload_pairs_deferred_tool_output_when_output_precedes_call() {
        let request = LLMRequest {
            model: "gpt-5.3-codex".to_string(),
            messages: vec![
                Message::user("continue".to_string()),
                Message::tool_response("call_1".to_string(), "{\"output\":\"late\"}".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_1".to_string(),
                        "unified_exec".to_string(),
                        "{\"command\":\"echo late\"}".to_string(),
                    )],
                ),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, true).expect("payload should build");

        let call_index = payload
            .input
            .iter()
            .position(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call")
                    && item.get("call_id").and_then(Value::as_str) == Some("call_1")
            })
            .expect("function_call should exist");
        let output_index = payload
            .input
            .iter()
            .position(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call_output")
                    && item.get("call_id").and_then(Value::as_str) == Some("call_1")
            })
            .expect("function_call_output should exist");

        assert!(output_index > call_index);
        assert_eq!(
            payload.input[output_index]["output"],
            "{\"output\":\"late\"}"
        );
        assert_ne!(payload.input[output_index]["output"], "aborted");
    }

    #[test]
    fn standard_payload_omits_function_call_id_for_codex_replay_shape() {
        let request = LLMRequest {
            model: "gpt-5.1-codex".to_string(),
            messages: vec![
                Message::user("run cargo fmt and report".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_T4IsdQtJifUHQUXutDlwoFLd".to_string(),
                        "unified_exec".to_string(),
                        r#"{"command":"cd /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode && cargo fmt","workdir":"/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode","sandbox_permissions":"use_default","additional_permissions":{"fs_read":[],"fs_write":[]}}"#.to_string(),
                    )],
                ),
                Message::tool_response(
                    "call_T4IsdQtJifUHQUXutDlwoFLd".to_string(),
                    r#"{"output":"","exit_code":0,"backend":"pipe"}"#.to_string(),
                ),
                Message::system(
                    "Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration.".to_string(),
                ),
                Message::user("ok".to_string()),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, true).expect("payload should build");
        let function_call = payload
            .input
            .iter()
            .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
            .expect("function_call item should exist");

        assert_eq!(
            function_call.get("call_id").and_then(Value::as_str),
            Some("call_T4IsdQtJifUHQUXutDlwoFLd")
        );
        assert!(
            function_call.get("id").is_none(),
            "function_call replay items should omit id"
        );
    }

    #[test]
    fn parse_responses_payload_prefers_call_id_for_tool_correlation() {
        let response = json!({
            "output": [
                {
                    "type": "function_call",
                    "id": "fc_123",
                    "call_id": "call_123",
                    "name": "unified_exec",
                    "arguments": "{\"command\":\"cargo fmt\"}"
                }
            ]
        });

        let parsed = parse_responses_payload(response, "gpt-5.3-codex".to_string(), false)
            .expect("payload should parse");

        let tool_calls = parsed.tool_calls.expect("tool calls should exist");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
        assert_eq!(
            tool_calls[0]
                .function
                .as_ref()
                .map(|function| function.name.as_str()),
            Some("unified_exec")
        );
    }

    #[test]
    fn standard_payload_can_move_assistant_text_history_into_instructions() {
        let request = LLMRequest {
            model: "gpt-5.2-codex".to_string(),
            messages: vec![
                Message::user("What is this project?".to_string()),
                Message::assistant("VT Code is a Rust Cargo workspace.".to_string()),
                Message::user("Tell me more.".to_string()),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, false).expect("payload should build");

        assert_eq!(payload.input.len(), 2);
        assert_eq!(payload.input[0]["role"], "user");
        assert_eq!(payload.input[1]["role"], "user");
        assert_eq!(
            payload.instructions.as_deref(),
            Some("Previous assistant response:\nVT Code is a Rust Cargo workspace.")
        );
    }

    #[test]
    fn standard_payload_can_omit_reasoning_details_from_input() {
        let request = LLMRequest {
            model: "gpt-5.2-codex".to_string(),
            messages: vec![
                Message::assistant("answer".to_string()).with_reasoning_details(Some(vec![
                    json!({
                        "type": "reasoning",
                        "id": "rs_1",
                        "summary": [{"type":"summary_text","text":"opaque"}]
                    }),
                ])),
                Message::user("next".to_string()),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, false).expect("payload should build");

        assert_eq!(payload.input.len(), 1);
        assert_eq!(payload.input[0]["role"], "user");
    }

    #[test]
    fn standard_payload_can_move_tool_turn_history_into_instructions() {
        let request = LLMRequest {
            model: "gpt-5.2-codex".to_string(),
            messages: vec![
                Message::user("run cargo check".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_1".to_string(),
                        "unified_exec".to_string(),
                        "{\"command\":\"cargo check\"}".to_string(),
                    )],
                ),
                Message::tool_response(
                    "call_1".to_string(),
                    "{\"output\":\"Finished `dev` profile\",\"exit_code\":0}".to_string(),
                ),
                Message::assistant("cargo check completed successfully.".to_string()),
                Message::user("tell me more".to_string()),
            ],
            ..Default::default()
        };

        let payload =
            build_standard_responses_payload(&request, false).expect("payload should build");

        assert_eq!(payload.input.len(), 2);
        assert_eq!(payload.input[0]["role"], "user");
        assert_eq!(payload.input[1]["role"], "user");
        let instructions = payload.instructions.expect("instructions should exist");
        assert!(instructions.contains("Previous tool result (call_1):"));
        assert!(instructions.contains("Finished `dev` profile"));
        assert!(
            instructions
                .contains("Previous assistant response:\ncargo check completed successfully.")
        );
    }

    #[test]
    fn parse_responses_payload_ignores_hosted_shell_trace_items() {
        let response = json!({
            "output": [
                {
                    "type": "shell_call",
                    "id": "sh_1",
                    "status": "completed",
                    "action": { "type": "command", "command": ["pwd"] }
                },
                {
                    "type": "shell_call_output",
                    "id": "sho_1",
                    "call_id": "sh_1",
                    "output": "workspace\n"
                },
                {
                    "type": "message",
                    "content": [
                        { "type": "output_text", "text": "Done." }
                    ]
                }
            ]
        });

        let parsed =
            parse_responses_payload(response, "gpt-5".to_string(), false).expect("should parse");

        assert_eq!(parsed.content.as_deref(), Some("Done."));
        assert!(parsed.tool_calls.unwrap_or_default().is_empty());
    }
}
