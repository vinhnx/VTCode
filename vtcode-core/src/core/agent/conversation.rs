//! Helpers for composing agent conversations and bridging provider-specific message formats.

use crate::core::agent::task::{ContextItem, Task};
use crate::gemini::{Content, Part};
use crate::llm::provider::Message;

/// Compose the full system instruction text combining the base prompt with task metadata.
pub fn compose_system_instruction(
    base_prompt: &str,
    task: &Task,
    contexts: &[ContextItem],
) -> String {
    let mut instruction = base_prompt.to_string();
    instruction.push_str(&format!("\n\nTask: {}\n{}", task.title, task.description));

    if !contexts.is_empty() {
        instruction.push_str("\n\nRelevant Context:");
        for ctx in contexts {
            instruction.push_str(&format!("\n[{}] {}", ctx.id, ctx.content));
        }
    }

    instruction
}

/// Build the initial conversation payload (without the system instruction message).
pub fn build_conversation(task: &Task, contexts: &[ContextItem]) -> Vec<Content> {
    let mut conversation = Vec::new();
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
    let mut messages = Vec::new();
    messages.push(Message::system(system_instruction.to_string()));

    for content in conversation {
        let mut text = String::new();
        for part in &content.parts {
            if let Part::Text { text: part_text } = part {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(part_text);
            }
        }

        let message = match content.role.as_str() {
            "model" => Message::assistant(text),
            _ => Message::user(text),
        };
        messages.push(message);
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task() -> Task {
        Task {
            id: "task-1".to_string(),
            title: "Example".to_string(),
            description: "Do something".to_string(),
            instructions: Some("Follow steps".to_string()),
        }
    }

    #[test]
    fn system_instruction_composes_context() {
        let task = sample_task();
        let contexts = vec![ContextItem {
            id: "ctx1".to_string(),
            content: "Details".to_string(),
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
