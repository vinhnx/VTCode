use crate::config::constants::models;
use crate::llm::provider::{LLMProvider, LLMRequest, Message, MessageRole};
use serde::{Deserialize, Serialize};
// std::collections::HashMap import removed as it's not used

/// Context compression configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompressionConfig {
    pub max_context_length: usize,
    pub compression_threshold: f64, // Percentage of max length to trigger compression
    pub summary_max_length: usize,
    pub preserve_recent_turns: usize, // Number of recent turns to always keep
    pub preserve_system_messages: bool,
    pub preserve_error_messages: bool,
}

impl Default for ContextCompressionConfig {
    fn default() -> Self {
        Self {
            max_context_length: 128000, // ~128K tokens
            compression_threshold: 0.8, // 80% of max length
            summary_max_length: 2000,
            preserve_recent_turns: 5,
            preserve_system_messages: true,
            preserve_error_messages: true,
        }
    }
}

/// Compressed context representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedContext {
    pub summary: String,
    pub preserved_messages: Vec<Message>,
    pub compression_ratio: f64,
    pub original_length: usize,
    pub compressed_length: usize,
    pub timestamp: u64,
}

/// Context compression engine
pub struct ContextCompressor {
    config: ContextCompressionConfig,
    llm_provider: Box<dyn LLMProvider>,
}

impl ContextCompressor {
    pub fn new(llm_provider: Box<dyn LLMProvider>) -> Self {
        Self {
            config: ContextCompressionConfig::default(),
            llm_provider,
        }
    }

    pub fn with_config(mut self, config: ContextCompressionConfig) -> Self {
        self.config = config;
        self
    }

    /// Check if context needs compression
    pub fn needs_compression(&self, messages: &[Message]) -> bool {
        let total_length = self.calculate_context_length(messages);
        total_length
            > (self.config.max_context_length as f64 * self.config.compression_threshold) as usize
    }

    /// Compress context by summarizing older messages
    pub async fn compress_context(
        &self,
        messages: &[Message],
    ) -> Result<CompressedContext, ContextCompressionError> {
        if messages.is_empty() {
            return Err(ContextCompressionError::EmptyContext);
        }

        let total_length = self.calculate_context_length(messages);

        // Separate messages to preserve and summarize
        let (to_preserve, to_summarize) = self.partition_messages(messages);

        if to_summarize.is_empty() {
            // No messages to summarize, return original
            return Ok(CompressedContext {
                summary: String::new(),
                preserved_messages: messages.to_vec(),
                compression_ratio: 1.0,
                original_length: total_length,
                compressed_length: total_length,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });
        }

        // Generate summary of messages to compress
        let summary = self.generate_summary(&to_summarize).await?;

        // Combine summary with preserved messages
        let mut compressed_messages = Vec::new();

        // Add summary as a system message if we have content to summarize
        if !summary.is_empty() {
            compressed_messages.push(Message {
                role: MessageRole::System,
                content: format!("Previous conversation summary: {}", summary),
                reasoning: None,
                reasoning_details: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Add preserved messages
        compressed_messages.extend_from_slice(&to_preserve);

        let compressed_length = self.calculate_context_length(&compressed_messages);
        let compression_ratio = if total_length > 0 {
            compressed_length as f64 / total_length as f64
        } else {
            1.0
        };

        Ok(CompressedContext {
            summary,
            preserved_messages: compressed_messages,
            compression_ratio,
            original_length: total_length,
            compressed_length,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }

    /// Partition messages into those to preserve and those to summarize
    fn partition_messages(&self, messages: &[Message]) -> (Vec<Message>, Vec<Message>) {
        let mut to_preserve = Vec::new();
        let mut to_summarize = Vec::new();

        let len = messages.len();

        for (i, message) in messages.iter().enumerate() {
            let should_preserve = self.should_preserve_message(message, i, len);

            if should_preserve {
                to_preserve.push(message.clone());
            } else {
                to_summarize.push(message.clone());
            }
        }

        (to_preserve, to_summarize)
    }

    /// Determine if a message should be preserved
    fn should_preserve_message(&self, message: &Message, index: usize, total_len: usize) -> bool {
        // Always preserve recent messages
        if index >= total_len.saturating_sub(self.config.preserve_recent_turns) {
            return true;
        }

        // Preserve decision ledger summaries explicitly
        if message.content.contains("[Decision Ledger]")
            || message
                .content
                .contains("Decision Ledger (most recent first)")
        {
            return true;
        }

        // Preserve system messages if configured
        if self.config.preserve_system_messages && matches!(message.role, MessageRole::System) {
            return true;
        }

        // Preserve messages that contain errors if configured
        if self.config.preserve_error_messages && self.contains_error_indicators(&message.content) {
            return true;
        }

        // Preserve tool calls and their results
        if message.tool_calls.is_some() || message.tool_call_id.is_some() {
            return true;
        }

        false
    }

    /// Check if message content contains error indicators
    fn contains_error_indicators(&self, content: &str) -> bool {
        let error_keywords = [
            "error",
            "failed",
            "exception",
            "crash",
            "bug",
            "issue",
            "problem",
            "unable",
            "cannot",
            "failed",
            "timeout",
            "connection refused",
        ];

        let content_lower = content.to_lowercase();
        error_keywords
            .iter()
            .any(|&keyword| content_lower.contains(keyword))
    }

    /// Generate summary of messages using LLM
    async fn generate_summary(
        &self,
        messages: &[Message],
    ) -> Result<String, ContextCompressionError> {
        if messages.is_empty() {
            return Ok(String::new());
        }

        // Create a prompt for summarization
        let conversation_text = self.messages_to_text(messages);

        let system_prompt = "You are a helpful assistant that summarizes conversations. \
                           Create a concise summary of the following conversation, \
                           focusing on key decisions, completed tasks, and important context. \
                           Keep the summary under 500 words."
            .to_string();

        let user_prompt = format!(
            "Please summarize the following conversation:\n\n{}",
            conversation_text
        );

        let request = LLMRequest {
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: system_prompt,
                    reasoning: None,
                    reasoning_details: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: MessageRole::User,
                    content: user_prompt,
                    reasoning: None,
                    reasoning_details: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            system_prompt: None,
            tools: None,
            model: models::GPT_5_MINI.to_string(), // Use a lightweight model for summarization
            max_tokens: Some(1000),
            temperature: Some(0.3),
            stream: false,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        };

        let response = self
            .llm_provider
            .generate(request)
            .await
            .map_err(|e| ContextCompressionError::LLMError(e.to_string()))?;

        Ok(response.content.unwrap_or_default())
    }

    /// Convert messages to readable text
    fn messages_to_text(&self, messages: &[Message]) -> String {
        let mut text = String::new();

        for message in messages {
            let role = match message.role {
                MessageRole::System => "System",
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::Tool => "Tool",
            };

            text.push_str(&format!("{}: {}\n\n", role, message.content));

            if let Some(tool_calls) = &message.tool_calls {
                for tool_call in tool_calls {
                    text.push_str(&format!(
                        "Tool Call: {}({})\n",
                        tool_call.function.name, tool_call.function.arguments
                    ));
                }
            }
        }

        text
    }

    /// Calculate total context length (approximate token count)
    fn calculate_context_length(&self, messages: &[Message]) -> usize {
        let mut total_chars = 0;

        for message in messages {
            total_chars += message.content.len();

            if let Some(tool_calls) = &message.tool_calls {
                for tool_call in tool_calls {
                    total_chars += tool_call.function.name.len();
                    total_chars += tool_call.function.arguments.len();
                }
            }
        }

        // Rough approximation: 1 token ≈ 4 characters
        total_chars / 4
    }
}

/// Context compression errors
#[derive(Debug, thiserror::Error)]
pub enum ContextCompressionError {
    #[error("Empty context provided")]
    EmptyContext,

    #[error("LLM error: {0}")]
    LLMError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{
        FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole,
    };

    #[test]
    fn test_context_length_calculation() {
        let compressor = ContextCompressor::new(Box::new(MockProvider::new()));

        let messages = vec![
            Message {
                role: MessageRole::User,
                content: "Hello world".to_string(),
                reasoning: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: MessageRole::Assistant,
                content: "Hi there! How can I help you?".to_string(),
                reasoning: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let length = compressor.calculate_context_length(&messages);
        assert_eq!(
            length,
            ("Hello worldHi there! How can I help you?".len()) / 4
        );
    }

    #[test]
    fn test_needs_compression() {
        let mut config = ContextCompressionConfig::default();
        config.max_context_length = 100;
        config.compression_threshold = 0.8;

        let compressor = ContextCompressor::new(Box::new(MockProvider::new())).with_config(config);

        let messages = vec![Message {
            role: MessageRole::User,
            content: "x".repeat(400), // ~100 tokens
            reasoning: None,
            tool_calls: None,
            tool_call_id: None,
        }];

        assert!(compressor.needs_compression(&messages));
    }

    // Mock provider for testing
    struct MockProvider;

    impl MockProvider {
        fn new() -> Self {
            Self
        }
    }

    #[async_trait::async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse {
                content: Some("Mock summary".to_string()),
                tool_calls: None,
                usage: None,
                finish_reason: FinishReason::Stop,
                reasoning: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["mock".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }
}
