//! Helpers for composing agent conversations and bridging provider-specific message formats.

use crate::core::agent::task::{ContextItem, Task};
use crate::gemini::{Content, Part};
use crate::llm::provider::Message;
use std::fmt::Write;

/// Compose the full system instruction text combining the base prompt with task metadata.
pub fn compose_system_instruction(
    base_prompt: &str,
    task: &Task,
    contexts: &[ContextItem],
) -> String {
    let mut instruction = base_prompt.to_string();
    let _ = write!(
        instruction,
        "\n\nTask: {}\n{}",
        task.title, task.description
    );

    if !contexts.is_empty() {
        instruction.push_str("\n\nRelevant Context:");
        for ctx in contexts {
            let _ = write!(instruction, "\n[{}] {}", ctx.id, ctx.content);
        }
    }

    instruction
}

/// Build the initial conversation payload (without the system instruction message).
pub fn build_conversation(task: &Task, contexts: &[ContextItem]) -> Vec<Content> {
    let mut conversation = Vec::with_capacity(3);
    conversation.push(Content::user_text(format!(
        "Task: {}\nDescription: {}",
        task.title, task.description
    )));

    if let Some(instructions) = task.instructions.as_ref() {
        conversation.push(Content::user_text(instructions.clone()));
    }

    if !contexts.is_empty() {
        let context_content: Vec<String> = contexts
            .iter()
            .map(|ctx| format!("Context [{}]: {}", ctx.id, ctx.content))
            .collect();
        conversation.push(Content::user_text(format!(
            "Relevant Context:\n{}",
            context_content.join("\n")
        )));
    }

    conversation
}

/// Convert Gemini `Content` structures into universal provider messages.
pub fn build_messages_from_conversation(
    system_instruction: &str,
    conversation: &[Content],
) -> Vec<Message> {
    let mut messages = Vec::with_capacity(conversation.len() + 1);
    messages.push(Message::system(system_instruction.to_string()));

    for content in conversation {
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_responses = Vec::new();

        for part in &content.parts {
            match part {
                Part::Text {
                    text: part_text, ..
                } => {
                    if let Some(crate::llm::provider::ContentPart::Text { text }) =
                        content_parts.last_mut()
                    {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(part_text);
                    } else if !part_text.is_empty() {
                        content_parts
                            .push(crate::llm::provider::ContentPart::text(part_text.clone()));
                    }
                }
                Part::InlineData { inline_data } => {
                    content_parts.push(crate::llm::provider::ContentPart::image(
                        inline_data.data.clone(),
                        inline_data.mime_type.clone(),
                    ));
                }
                Part::FunctionCall {
                    function_call,
                    thought_signature,
                } => {
                    tool_calls.push(crate::llm::provider::ToolCall {
                        id: function_call.id.clone().unwrap_or_default(),
                        call_type: "function".to_string(),
                        function: Some(crate::llm::provider::FunctionCall {
                            name: function_call.name.clone(),
                            arguments: function_call.args.to_string(),
                        }),
                        text: None,
                        thought_signature: thought_signature.clone(),
                    });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task() -> Task {
        Task {
            id: "task-1".to_owned(),
            title: "Example".to_owned(),
            description: "Do something".to_owned(),
            instructions: Some("Follow steps".to_owned()),
        }
    }

    #[test]
    fn system_instruction_composes_context() {
        let task = sample_task();
        let contexts = vec![ContextItem {
            id: "ctx1".to_owned(),
            content: "Details".to_owned(),
        }];

        let instruction = compose_system_instruction("Base", &task, &contexts);
        assert!(instruction.contains("Task: Example"));
        assert!(instruction.contains("[ctx1] Details"));
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
    fn messages_include_system_prompt() {
        let task = sample_task();
        let conversation = build_conversation(&task, &[]);
        let messages = build_messages_from_conversation("Base", &conversation);
        assert_eq!(messages.len(), conversation.len() + 1);
    }
}
