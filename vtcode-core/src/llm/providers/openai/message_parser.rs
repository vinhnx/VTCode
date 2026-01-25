//! JSON message parsing for OpenAI-style chat payloads.

use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider as provider;
use crate::llm::providers::shared::parse_openai_tool_calls;
use serde_json::{Value, json};

pub(crate) fn parse_chat_request(
    value: &Value,
    default_model: &str,
) -> Option<provider::LLMRequest> {
    let messages_value = value.get("messages")?.as_array()?;
    let mut system_prompt = None;
    let mut messages = Vec::with_capacity(messages_value.len());

    for entry in messages_value {
        let role = entry
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or(crate::config::constants::message_roles::USER);
        let content = entry.get("content");
        let text_content = content.map(extract_content_text).unwrap_or_default();

        match role {
            "system" => {
                if system_prompt.is_none() && !text_content.is_empty() {
                    system_prompt = Some(text_content);
                }
            }
            "assistant" => {
                let tool_calls = entry
                    .get("tool_calls")
                    .and_then(|tc| tc.as_array())
                    .map(|calls| parse_openai_tool_calls(calls))
                    .filter(|calls| !calls.is_empty());

                let message = if let Some(calls) = tool_calls {
                    provider::Message::assistant_with_tools(text_content, calls)
                } else {
                    provider::Message::assistant(text_content)
                };
                messages.push(message);
            }
            "tool" => {
                let tool_call_id = entry
                    .get("tool_call_id")
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string());
                let content_value = entry
                    .get("content")
                    .map(|value| {
                        if text_content.is_empty() {
                            value.to_string()
                        } else {
                            text_content.clone()
                        }
                    })
                    .unwrap_or_else(|| text_content.clone());
                messages.push(if let Some(id) = tool_call_id {
                    provider::Message::tool_response(id, content_value)
                } else {
                    provider::Message {
                        role: provider::MessageRole::Tool,
                        content: provider::MessageContent::Text(content_value),
                        reasoning: None,
                        reasoning_details: None,
                        tool_calls: None,
                        tool_call_id: None,
                        origin_tool: None,
                    }
                });
            }
            _ => {
                messages.push(provider::Message::user(text_content));
            }
        }
    }

    if messages.is_empty() {
        return None;
    }

    let tools = value.get("tools").and_then(|tools_value| {
        let tools_array = tools_value.as_array()?;
        let converted: Vec<_> = tools_array
            .iter()
            .filter_map(|tool| {
                let function = tool.get("function")?;
                let name = function.get("name").and_then(|n| n.as_str())?;
                let description = function
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let parameters = function
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                Some(provider::ToolDefinition::function(
                    name.to_string(),
                    description,
                    parameters,
                ))
            })
            .collect();

        if converted.is_empty() {
            None
        } else {
            Some(converted)
        }
    });
    let temperature = value
        .get("temperature")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);
    let stream = value
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tool_choice = value.get("tool_choice").and_then(parse_tool_choice);
    let _parallel_tool_calls = value.get("parallel_tool_calls").and_then(|v| v.as_bool());
    let _reasoning_effort = value
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .and_then(ReasoningEffortLevel::parse)
        .or_else(|| {
            value
                .get("reasoning")
                .and_then(|r| r.get("effort"))
                .and_then(|effort| effort.as_str())
                .and_then(ReasoningEffortLevel::parse)
        });

    let model = value
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(default_model)
        .to_string();

    Some(provider::LLMRequest {
        messages,
        system_prompt,
        tools,
        model,
        temperature,
        stream,
        tool_choice,
        ..Default::default()
    })
}

fn extract_content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        part.get("content")
                            .and_then(|c| c.as_str())
                            .map(|s| s.to_string())
                    })
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn parse_tool_choice(choice: &Value) -> Option<provider::ToolChoice> {
    match choice {
        Value::String(value) => match value.as_str() {
            "auto" => Some(provider::ToolChoice::auto()),
            "none" => Some(provider::ToolChoice::none()),
            "required" => Some(provider::ToolChoice::any()),
            _ => None,
        },
        Value::Object(map) => {
            let choice_type = map.get("type").and_then(|t| t.as_str())?;
            match choice_type {
                "function" => map
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|name| provider::ToolChoice::function(name.to_string())),
                "auto" => Some(provider::ToolChoice::auto()),
                "none" => Some(provider::ToolChoice::none()),
                "any" | "required" => Some(provider::ToolChoice::any()),
                _ => None,
            }
        }
        _ => None,
    }
}
