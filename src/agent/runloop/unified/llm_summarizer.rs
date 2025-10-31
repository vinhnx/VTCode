use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{instrument, warn};
use vtcode_core::core::conversation_summarizer::ConversationTurn;

/// Configuration for LLM-based summarization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct LlmSummarizerConfig {
    /// Maximum number of concurrent LLM summarization requests
    pub max_concurrent_requests: usize,

    /// Maximum number of tokens per request
    pub max_tokens_per_request: usize,

    /// Model to use for summarization
    pub model_name: String,

    /// Temperature for generation (0.0 to 1.0)
    pub temperature: f32,

    /// Maximum number of retries for failed requests
    pub max_retries: u32,

    /// Delay between retries in seconds
    pub retry_delay_seconds: u64,
}

impl Default for LlmSummarizerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 4,
            max_tokens_per_request: 2000,
            model_name: "gpt-3.5-turbo".to_string(),
            temperature: 0.3,
            max_retries: 3,
            retry_delay_seconds: 2,
        }
    }
}

/// Trait for LLM summarization backends
#[allow(dead_code)]
#[async_trait]
pub trait LlmBackend: Send + Sync + 'static {
    /// Summarize the given text
    async fn summarize(&self, text: &str, config: &LlmSummarizerConfig) -> Result<String>;
}

/// LLM-based summarizer that manages rate limiting and concurrency
#[allow(dead_code)]
pub struct LlmSummarizer<B: LlmBackend> {
    backend: Arc<B>,
    config: LlmSummarizerConfig,
    semaphore: Arc<Semaphore>,
}

#[allow(dead_code)]
impl<B: LlmBackend> LlmSummarizer<B> {
    /// Create a new LLM summarizer with the given backend and config
    pub fn new(backend: B, config: LlmSummarizerConfig) -> Self {
        Self {
            backend: Arc::new(backend),
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_requests)),
            config,
        }
    }

    /// Summarize a conversation using the LLM
    #[instrument(skip(self, turns))]
    pub async fn summarize_conversation(
        &self,
        turns: &[ConversationTurn],
    ) -> Result<Vec<ConversationTurn>> {
        if turns.is_empty() {
            return Ok(Vec::new());
        }

        // Convert turns to text
        let text = turns
            .iter()
            .map(|t| format!("{}: {}", t.role, t.content))
            .collect::<Vec<_>>()
            .join("\n");

        // Get a permit for rate limiting
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to acquire semaphore: {}", e))?;

        // Try with retries
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.try_summarize(&text).await {
                Ok(summary) => {
                    // Parse the summary back into turns
                    let summary_turns = parse_summary_into_turns(&summary);
                    return Ok(summary_turns);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries {
                        let delay = std::time::Duration::from_secs(self.config.retry_delay_seconds);
                        warn!(
                            "Attempt {}/{} failed, retrying in {:?}: {}",
                            attempt + 1,
                            self.config.max_retries,
                            delay,
                            last_error.as_ref().unwrap()
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "Failed after {} attempts: {}",
            self.config.max_retries,
            last_error.unwrap()
        ))
    }

    async fn try_summarize(&self, text: &str) -> Result<String> {
        // Split text into chunks if needed
        let chunks = self.split_into_chunks(text);
        let mut summaries = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            let summary = self.backend.summarize(&chunk, &self.config).await?;
            summaries.push(summary);
        }

        // If we have multiple chunks, combine the summaries
        if summaries.len() > 1 {
            let combined = summaries.join("\n\n");
            self.backend.summarize(&combined, &self.config).await
        } else {
            Ok(summaries.into_iter().next().unwrap_or_default())
        }
    }

    fn split_into_chunks(&self, text: &str) -> Vec<String> {
        let max_chunk_size = self.config.max_tokens_per_request * 4; // Rough estimate
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut current_size = 0;

        for line in text.lines() {
            let line_size = line.len() + 1; // +1 for newline

            if current_size + line_size > max_chunk_size && !current_chunk.is_empty() {
                chunks.push(std::mem::take(&mut current_chunk));
                current_size = 0;
            }

            current_chunk.push_str(line);
            current_chunk.push('\n');
            current_size += line_size;
        }

        if !current_chunk.is_empty() {
            chunks.push(current_chunk);
        }

        chunks
    }
}

/// Parse the LLM's summary back into conversation turns
#[allow(dead_code)]
fn parse_summary_into_turns(summary: &str) -> Vec<ConversationTurn> {
    let mut turns = Vec::new();
    let mut current_role = None;
    let mut current_content = String::new();
    let _turn_number = 0;

    for line in summary.lines() {
        if let Some(role_end) = line.find(':') {
            // Check if this looks like a role prefix (e.g., "User: ")
            let role = line[..role_end].trim();
            if is_valid_role(role) {
                // Save the previous turn if any
                if let Some(role) = current_role.take() {
                    turns.push(ConversationTurn {
                        role,
                        content: std::mem::take(&mut current_content),
                        turn_number: turns.len(),
                        task_info: None, // Add default task_info
                    });
                }

                current_role = Some(role.to_string());
                current_content = line[role_end + 1..].trim().to_string();
                continue;
            }
        }

        // If we're in a message, append to current content
        if !current_content.is_empty() {
            current_content.push('\n');
        }
        current_content.push_str(line);
    }

    // Add the last turn if any
    if let Some(role) = current_role {
        turns.push(ConversationTurn {
            role,
            content: current_content,
            turn_number: turns.len(),
            task_info: None,
        });
    }

    turns
}

#[allow(dead_code)]
fn is_valid_role(role: &str) -> bool {
    matches!(
        role.to_lowercase().as_str(),
        "user" | "assistant" | "system" | "tool"
    )
}

#[async_trait]
impl<B: LlmBackend> LlmBackend for Arc<B> {
    async fn summarize(&self, text: &str, config: &LlmSummarizerConfig) -> Result<String> {
        self.as_ref().summarize(text, config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use vtcode_core::core::conversation_summarizer::ConversationTurn;

    struct MockLlmBackend;

    #[async_trait]
    impl LlmBackend for MockLlmBackend {
        async fn summarize(&self, text: &str, _config: &LlmSummarizerConfig) -> Result<String> {
            // Simple mock that just returns the first 100 chars as a summary
            Ok(text.chars().take(100).collect())
        }
    }

    #[tokio::test]
    async fn test_summarize_conversation() {
        let backend = MockLlmBackend;
        let config = LlmSummarizerConfig::default();
        let summarizer = LlmSummarizer::new(backend, config);

        let turns = vec![
            ConversationTurn {
                role: "user".to_string(),
                content: "Hello, how are you?".to_string(),
                turn_number: 1,
                task_info: None,
            },
            ConversationTurn {
                role: "assistant".to_string(),
                content: "I'm doing well, thank you for asking! How can I help you today?"
                    .to_string(),
                turn_number: 2,
                task_info: None,
            },
        ];

        let result = summarizer.summarize_conversation(&turns).await;
        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(!summary.is_empty());
    }

    #[test]
    fn test_parse_summary_into_turns() {
        let summary = r#"
        user: Hello
        How are you?

        assistant: I'm doing well
        How can I help you today?
        "#;

        let turns = parse_summary_into_turns(summary);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert!(turns[0].content.contains("Hello"));
        assert_eq!(turns[1].role, "assistant");
        assert!(turns[1].content.contains("I'm doing well"));
    }
}
