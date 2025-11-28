//! Chat functionality for the agent

use anyhow::Result;

/// Chat message structure
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Chat session for managing conversation state
#[derive(Default)]
pub struct ChatSession {
    messages: Vec<ChatMessage>,
}

impl ChatSession {
    /// Create a new chat session
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a message to the chat session
    pub fn add_message(&mut self, role: String, content: String) {
        self.messages.push(ChatMessage { role, content });
    }

    /// Get all messages in the session
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

/// Initialize chat functionality
pub fn init_chat() -> Result<ChatSession> {
    Ok(ChatSession::new())
}
