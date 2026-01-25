//! Message parsing utilities for Anthropic provider
//!
//! Parses incoming JSON prompts into LLMRequest format,
//! handling the conversion of Anthropic-style messages.

use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider::{
    LLMRequest, Message, ParallelToolConfig, ToolCall, ToolChoice, ToolDefinition,
};
use serde_json::{Value, json};

pub fn parse_messages_request(value: &Value, default_model: &str) -> Option<LLMRequest> {
    let messages_value = value.get("messages")?.as_array()?;
    let mut system_prompt = value
        .get("system")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let mut messages = Vec::new();

    for entry in messages_value {
        let role = entry
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or(crate::config::constants::message_roles::USER);

        match role {
            "assistant" => {
                let (text_content, tool_calls) = parse_assistant_content(entry);
                let message = if tool_calls.is_empty() {
                    Message::assistant(text_content)
                } else {
                    Message::assistant_with_tools(text_content, tool_calls)
                };
                messages.push(message);
            }
            "user" => {
                parse_user_content(entry, &mut messages);
            }
            "system" => {
                if system_prompt.is_none() {
                    let extracted = extract_system_content(entry);
                    if !extracted.is_empty() {
                        system_prompt = Some(extracted);
                    }
                }
            }
            _ => {
                if let Some(content_text) = entry.get("content").and_then(|c| c.as_str()) {
                    messages.push(Message::user(content_text.to_string()));
                }
            }
        }
    }

    if messages.is_empty() {
        return None;
    }

    let tools = parse_tools(value);
    let temperature = value
        .get("temperature")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);
    let stream = value
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tool_choice = value.get("tool_choice").and_then(parse_tool_choice);
    let parallel_tool_calls = value.get("parallel_tool_calls").and_then(|v| v.as_bool());
    let parallel_tool_config = value
        .get("parallel_tool_config")
        .cloned()
        .and_then(|cfg| serde_json::from_value::<ParallelToolConfig>(cfg).ok());
    let reasoning_effort = parse_reasoning_effort(value);
    let output_format = value.get("output_format").cloned();

    let model = value
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(default_model)
        .to_string();

    Some(LLMRequest {
        messages,
        system_prompt,
        tools,
        model,
        temperature,
        stream,
        tool_choice,
        parallel_tool_calls,
        parallel_tool_config,
        reasoning_effort,
        output_format,
        ..Default::default()
    })
}

fn parse_assistant_content(entry: &Value) -> (String, Vec<ToolCall>) {
    let mut text_content = String::new();
    let mut tool_calls = Vec::new();

    if let Some(content_array) = entry.get("content").and_then(|c| c.as_array()) {
        for block in content_array {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_content.push_str(text);
                    }
                }
                Some("tool_use") => {
                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let arguments =
                        serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                    if !id.is_empty() && !name.is_empty() {
                        tool_calls.push(ToolCall::function(
                            id.to_string(),
                            name.to_string(),
                            arguments,
                        ));
                    }
                }
                _ => {}
            }
        }
    } else if let Some(content_text) = entry.get("content").and_then(|c| c.as_str()) {
        text_content.push_str(content_text);
    }

    (text_content, tool_calls)
}

fn parse_user_content(entry: &Value, messages: &mut Vec<Message>) {
    let mut text_buffer = String::new();
    let mut pending_tool_results = Vec::new();

    if let Some(content_array) = entry.get("content").and_then(|c| c.as_array()) {
        for block in content_array {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_buffer.push_str(text);
                    }
                }
                Some("tool_result") => {
                    if !text_buffer.is_empty() {
                        messages.push(Message::user(text_buffer.clone()));
                        text_buffer.clear();
                    }
                    if let Some(tool_use_id) = block.get("tool_use_id").and_then(|id| id.as_str()) {
                        let serialized = flatten_tool_result_content(block);
                        pending_tool_results.push((tool_use_id.to_string(), serialized));
                    }
                }
                _ => {}
            }
        }
    } else if let Some(content_text) = entry.get("content").and_then(|c| c.as_str()) {
        text_buffer.push_str(content_text);
    }

    if !text_buffer.is_empty() {
        messages.push(Message::user(text_buffer));
    }

    for (tool_use_id, payload) in pending_tool_results {
        messages.push(Message::tool_response(tool_use_id, payload));
    }
}

fn extract_system_content(entry: &Value) -> String {
    if let Some(content_array) = entry.get("content").and_then(|c| c.as_array()) {
        content_array
            .iter()
            .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
            .collect::<String>()
    } else {
        entry
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string()
    }
}

pub fn flatten_tool_result_content(block: &Value) -> String {
    if let Some(items) = block.get("content").and_then(|content| content.as_array()) {
        let mut aggregated = String::new();
        for item in items {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                aggregated.push_str(text);
            } else {
                aggregated.push_str(&item.to_string());
            }
        }
        if aggregated.is_empty() {
            block
                .get("content")
                .map(|v| v.to_string())
                .unwrap_or_default()
        } else {
            aggregated
        }
    } else if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
        text.to_string()
    } else {
        block.to_string()
    }
}

fn parse_tools(value: &Value) -> Option<Vec<ToolDefinition>> {
    let tools_value = value.get("tools")?;
    let tools_array = tools_value.as_array()?;

    let converted: Vec<_> = tools_array
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(|n| n.as_str())?;
            let description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let parameters = tool
                .get("input_schema")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let mut tool_def = ToolDefinition::function(name.to_string(), description, parameters);
            if let Some(strict_val) = tool.get("strict").and_then(|v| v.as_bool()) {
                tool_def = tool_def.with_strict(strict_val);
            }
            Some(tool_def)
        })
        .collect();

    if converted.is_empty() {
        None
    } else {
        Some(converted)
    }
}

pub fn parse_tool_choice(choice: &Value) -> Option<ToolChoice> {
    match choice {
        Value::String(value) => match value.as_str() {
            "auto" => Some(ToolChoice::auto()),
            "none" => Some(ToolChoice::none()),
            "any" => Some(ToolChoice::any()),
            _ => None,
        },
        Value::Object(map) => {
            let choice_type = map.get("type").and_then(|t| t.as_str())?;
            match choice_type {
                "auto" => Some(ToolChoice::auto()),
                "none" => Some(ToolChoice::none()),
                "any" => Some(ToolChoice::any()),
                "tool" => map
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|name| ToolChoice::function(name.to_string())),
                _ => None,
            }
        }
        _ => None,
    }
}

fn parse_reasoning_effort(value: &Value) -> Option<ReasoningEffortLevel> {
    value
        .get("reasoning_effort")
        .and_then(|r| r.as_str())
        .and_then(ReasoningEffortLevel::parse)
        .or_else(|| {
            value
                .get("reasoning")
                .and_then(|r| r.get("effort"))
                .and_then(|effort| effort.as_str())
                .and_then(ReasoningEffortLevel::parse)
        })
}
