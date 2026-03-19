//! Helpers for composing agent conversations and bridging provider-specific message formats.

use crate::core::agent::task::{ContextItem, Task};
use crate::llm::provider::{ContentPart, Message, MessageContent, MessageRole, ToolCall};
use crate::llm::providers::gemini::wire::{
    Content, FunctionCall, FunctionResponse, InlineData, Part,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Write;

/// Build the initial conversation payload (without the system instruction message).
pub fn build_conversation(task: &Task, contexts: &[ContextItem]) -> Vec<Content> {
    let mut conversation = Vec::with_capacity(3);
    let mut task_content = String::with_capacity(task.title.len() + task.description.len() + 20);
    let _ = write!(
        task_content,
        "Task: {}\nDescription: {}",
        task.title, task.description
    );
    conversation.push(Content::user_text(task_content));

    if let Some(instructions) = task.instructions.as_ref() {
        conversation.push(Content::user_text(instructions.clone()));
    }

    if !contexts.is_empty() {
        let mut context_content = String::from("Relevant Context:");
        for ctx in contexts {
            let _ = write!(context_content, "\nContext [{}]: {}", ctx.id, ctx.content);
        }
        conversation.push(Content::user_text(context_content));
    }

    conversation
}

/// Convert Gemini `Content` structures into universal provider messages.
pub fn messages_from_conversation(conversation: &[Content]) -> Vec<Message> {
    let mut messages = Vec::with_capacity(conversation.len());
    for content in conversation {
        let part_count = content.parts.len();
        let mut content_parts = Vec::with_capacity(part_count);
        let mut tool_calls = Vec::new();
        let mut tool_responses = Vec::new();

        for part in &content.parts {
            match part {
                Part::Text {
                    text: part_text, ..
                } => {
                    if let Some(ContentPart::Text { text }) = content_parts.last_mut() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(part_text);
                    } else if !part_text.is_empty() {
                        content_parts.push(ContentPart::text(part_text.clone()));
                    }
                }
                Part::InlineData { inline_data } => {
                    content_parts.push(ContentPart::image(
                        inline_data.data.clone(),
                        inline_data.mime_type.clone(),
                    ));
                }
                Part::FunctionCall {
                    function_call,
                    thought_signature,
                } => {
                    let mut tool_call = ToolCall::function(
                        function_call.id.clone().unwrap_or_default(),
                        function_call.name.clone(),
                        function_call.args.to_string(),
                    );
                    tool_call.thought_signature = thought_signature.clone();
                    tool_calls.push(tool_call);
                }
                Part::FunctionResponse {
                    function_response, ..
                } => {
                    let id = function_response
                        .id
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    let response_str = function_response.response.to_string();
                    tool_responses.push(Message::tool_response(id, response_str));
                }
                Part::CacheControl { .. } => {}
            }
        }

        if !tool_responses.is_empty() {
            messages.extend(tool_responses);
            if !content_parts.is_empty() {
                messages.push(Message::user_with_parts(content_parts));
            }
            continue;
        }

        let mut message = match content.role.as_str() {
            "model" => {
                if content_parts.is_empty() {
                    Message::assistant(String::new())
                } else {
                    Message::assistant_with_parts(content_parts)
                }
            }
            _ => {
                if content_parts.is_empty() {
                    Message::user(String::new())
                } else {
                    Message::user_with_parts(content_parts)
                }
            }
        };

        if !tool_calls.is_empty() {
            message.tool_calls = Some(tool_calls);
        }

        messages.push(message);
    }

    messages
}

/// Convert Gemini `Content` structures into universal provider messages.
///
/// System instructions travel separately via `LLMRequest.system_prompt` on the active request.
pub fn build_messages_from_conversation(conversation: &[Content]) -> Vec<Message> {
    messages_from_conversation(conversation)
}

fn parts_from_message_content(content: &MessageContent) -> Vec<Part> {
    match content {
        MessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Part::Text {
                    text: text.clone(),
                    thought_signature: None,
                }]
            }
        }
        MessageContent::Parts(parts) => {
            let mut converted = Vec::with_capacity(parts.len());
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if !text.is_empty() {
                            converted.push(Part::Text {
                                text: text.clone(),
                                thought_signature: None,
                            });
                        }
                    }
                    ContentPart::Image {
                        data, mime_type, ..
                    } => {
                        converted.push(Part::InlineData {
                            inline_data: InlineData {
                                mime_type: mime_type.clone(),
                                data: data.clone(),
                            },
                        });
                    }
                    ContentPart::File {
                        filename,
                        file_id,
                        file_url,
                        ..
                    } => {
                        let fallback = filename
                            .clone()
                            .or_else(|| file_id.clone())
                            .or_else(|| file_url.clone())
                            .unwrap_or_else(|| "attached file".to_string());
                        converted.push(Part::Text {
                            text: format!("[File input not directly supported: {fallback}]"),
                            thought_signature: None,
                        });
                    }
                }
            }
            converted
        }
    }
}

fn tool_call_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

fn tool_response_value(content: &MessageContent) -> Value {
    let text = content.as_text();
    serde_json::from_str(text.as_ref()).unwrap_or_else(|_| json!({ "result": text.as_ref() }))
}

/// Rebuild Gemini-style conversation content from archived provider messages.
///
/// System messages are skipped because exec regenerates the current system prompt.
pub fn conversation_from_messages(messages: &[Message]) -> Vec<Content> {
    let mut conversation = Vec::with_capacity(messages.len());
    let mut tool_names_by_call_id: HashMap<String, String> = HashMap::with_capacity(messages.len());

    for message in messages {
        match message.role {
            MessageRole::System => {}
            MessageRole::User => {
                let parts = parts_from_message_content(&message.content);
                if !parts.is_empty() {
                    conversation.push(Content {
                        role: "user".to_string(),
                        parts,
                    });
                }
            }
            MessageRole::Assistant => {
                let mut parts = parts_from_message_content(&message.content);
                if let Some(tool_calls) = &message.tool_calls {
                    for tool_call in tool_calls {
                        let Some(function) = &tool_call.function else {
                            continue;
                        };

                        let id = (!tool_call.id.is_empty()).then(|| tool_call.id.clone());
                        if let Some(call_id) = id.as_ref() {
                            tool_names_by_call_id.insert(call_id.clone(), function.name.clone());
                        }

                        parts.push(Part::FunctionCall {
                            function_call: FunctionCall {
                                name: function.name.clone(),
                                args: tool_call_arguments(&function.arguments),
                                id,
                            },
                            thought_signature: tool_call.thought_signature.clone(),
                        });
                    }
                }

                if !parts.is_empty() {
                    conversation.push(Content {
                        role: "model".to_string(),
                        parts,
                    });
                }
            }
            MessageRole::Tool => {
                let Some(call_id) = message
                    .tool_call_id
                    .as_ref()
                    .filter(|value| !value.is_empty())
                    .cloned()
                else {
                    let parts = parts_from_message_content(&message.content);
                    if !parts.is_empty() {
                        conversation.push(Content {
                            role: "user".to_string(),
                            parts,
                        });
                    }
                    continue;
                };

                let tool_name = message
                    .origin_tool
                    .clone()
                    .or_else(|| tool_names_by_call_id.get(&call_id).cloned())
                    .unwrap_or_else(|| "tool".to_string());

                conversation.push(Content {
                    role: "function".to_string(),
                    parts: vec![Part::FunctionResponse {
                        function_response: FunctionResponse {
                            name: tool_name,
                            response: tool_response_value(&message.content),
                            id: Some(call_id),
                        },
                        thought_signature: None,
                    }],
                });
            }
        }
    }

    conversation
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{FunctionCall, ToolCall};

    fn sample_task() -> Task {
        Task {
            id: "task-1".to_owned(),
            title: "Example".to_owned(),
            description: "Do something".to_owned(),
            instructions: Some("Follow steps".to_owned()),
        }
    }

    #[test]
    fn conversation_builds_expected_steps() {
        let task = sample_task();
        let contexts = vec![ContextItem {
            id: "ctx1".into(),
            content: "Data".into(),
        }];
        let conversation = build_conversation(&task, &contexts);
        assert_eq!(conversation.len(), 3);
    }

    #[test]
    fn messages_mirror_conversation_without_system_prompt() {
        let task = sample_task();
        let conversation = build_conversation(&task, &[]);
        let messages = build_messages_from_conversation(&conversation);
        assert_eq!(messages.len(), conversation.len());
        assert!(
            messages
                .iter()
                .all(|message| message.role != MessageRole::System)
        );
    }

    #[test]
    fn archived_messages_rebuild_function_history() {
        let history = vec![
            Message::system("Base".to_string()),
            Message::user("Inspect src/main.rs".to_string()),
            Message::assistant_with_tools(
                "Running read_file".to_string(),
                vec![ToolCall {
                    id: "call-1".to_string(),
                    call_type: "function".to_string(),
                    function: Some(FunctionCall {
                        name: "read_file".to_string(),
                        arguments: "{\"path\":\"src/main.rs\"}".to_string(),
                    }),
                    text: None,
                    thought_signature: None,
                }],
            ),
            Message::tool_response(
                "call-1".to_string(),
                "{\"content\":\"fn main() {}\"}".to_string(),
            ),
            Message::assistant("Done".to_string()),
        ];

        let conversation = conversation_from_messages(&history);
        let rebuilt = build_messages_from_conversation(&conversation);

        assert_eq!(rebuilt[0].role, MessageRole::User);
        assert_eq!(rebuilt[0].content.as_text().as_ref(), "Inspect src/main.rs");
        assert_eq!(rebuilt[1].role, MessageRole::Assistant);
        assert_eq!(
            rebuilt[1]
                .tool_calls
                .as_ref()
                .and_then(|calls| calls.first())
                .and_then(|call| call.function.as_ref())
                .map(|function| function.name.as_str()),
            Some("read_file")
        );
        assert_eq!(rebuilt[2].role, MessageRole::Tool);
        assert_eq!(rebuilt[2].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(rebuilt[3].role, MessageRole::Assistant);
        assert_eq!(rebuilt[3].content.as_text().as_ref(), "Done");
    }
}
