use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::fmt::Write;

use crate::llm::provider::{LLMProvider, LLMRequest, Message, MessageRole};

pub mod summarizer;

/// Compaction configuration for context window management.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Threshold (0.0-1.0) at which to trigger compaction.
    pub trigger_threshold: f64,
    /// Target usage ratio (0.0-1.0) after compaction.
    pub target_threshold: f64,
    /// Prompt for summarization.
    pub summary_prompt: String,
    /// Number of recent messages to keep verbatim.
    pub keep_last_messages: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            trigger_threshold: 0.85,
            target_threshold: 0.50,
            summary_prompt: "Summarize the conversation so far. Preserve decisions, file paths, commands, TODOs, and open questions. Keep it concise but actionable."
                .to_string(),
            keep_last_messages: 10,
        }
    }
}

/// Compact conversation history using the configured summarizer.
pub async fn compact_history(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
) -> Result<Vec<Message>> {
    if history.len() <= config.keep_last_messages {
        return Ok(history.to_vec());
    }

    let summary_prompt = build_summary_prompt(history, &config.summary_prompt);
    let request = LLMRequest {
        messages: vec![Message::user(summary_prompt)],
        model: model.to_string(),
        ..Default::default()
    };

    let response = provider
        .generate(request)
        .await
        .context("Failed to generate compaction summary")?;

    let summary = response.content.unwrap_or_default().trim().to_string();

    let mut new_history = Vec::with_capacity(config.keep_last_messages + 1);
    new_history.push(Message::system(format!(
        "Previous conversation summary:\n{}",
        summary
    )));

    let keep_start = history.len().saturating_sub(config.keep_last_messages);
    new_history.extend_from_slice(&history[keep_start..]);
    Ok(new_history)
}

fn build_summary_prompt(history: &[Message], instructions: &str) -> String {
    let mut formatted = String::new();
    let now: DateTime<Utc> = Utc::now();
    let _ = writeln!(
        &mut formatted,
        "Summary requested at {}.\n{}",
        now.to_rfc3339(),
        instructions
    );

    for message in history {
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        let content = message.content.as_text();
        if content.trim().is_empty() {
            continue;
        }
        let _ = writeln!(&mut formatted, "\n[{}]\n{}", role, content.trim());
    }

    formatted
}
