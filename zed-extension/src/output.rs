/// VT Code Output Channel
///
/// Manages output display in Zed's output panel.
/// Handles formatting, history, and user interaction.
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// A single message in the output channel
#[derive(Debug, Clone)]
pub struct OutputMessage {
    /// Timestamp of the message
    pub timestamp: SystemTime,
    /// Message content
    pub content: String,
    /// Message type (info, success, error, warning)
    pub message_type: MessageType,
}

/// Type of output message
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Info,
    Success,
    Error,
    Warning,
}

impl MessageType {
    pub fn prefix(&self) -> &'static str {
        match self {
            MessageType::Info => "[INFO]",
            MessageType::Success => "[OK]",
            MessageType::Error => "[ERR]",
            MessageType::Warning => "[W]",
        }
    }
}

/// Output channel for displaying VT Code results
pub struct OutputChannel {
    /// Message history
    messages: Arc<Mutex<Vec<OutputMessage>>>,
    /// Maximum history size
    max_history: usize,
}

impl OutputChannel {
    /// Create a new output channel
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            max_history: 1000,
        }
    }

    /// Add an info message
    pub fn info(&self, content: String) {
        self.add_message(content, MessageType::Info);
    }

    /// Add a success message
    pub fn success(&self, content: String) {
        self.add_message(content, MessageType::Success);
    }

    /// Add an error message
    pub fn error(&self, content: String) {
        self.add_message(content, MessageType::Error);
    }

    /// Add a warning message
    pub fn warning(&self, content: String) {
        self.add_message(content, MessageType::Warning);
    }

    /// Add a message to the channel
    fn add_message(&self, content: String, message_type: MessageType) {
        let message = OutputMessage {
            timestamp: SystemTime::now(),
            content,
            message_type,
        };

        if let Ok(mut messages) = self.messages.lock() {
            messages.push(message);

            // Trim history if needed
            if messages.len() > self.max_history {
                messages.remove(0);
            }
        }
    }

    /// Get all messages
    pub fn messages(&self) -> Result<Vec<OutputMessage>, String> {
        self.messages
            .lock()
            .map(|m| m.clone())
            .map_err(|e| format!("Failed to lock messages: {}", e))
    }

    /// Get formatted output as a single string
    pub fn formatted_output(&self) -> Result<String, String> {
        let messages = self.messages()?;
        let formatted = messages
            .iter()
            .map(|msg| {
                let time = msg
                    .timestamp
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()
                    .map(|d| format!("{}", d.as_secs()))
                    .unwrap_or_default();

                format!("[{}] {} {}", time, msg.message_type.prefix(), msg.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(formatted)
    }

    /// Clear all messages
    pub fn clear(&self) -> Result<(), String> {
        self.messages
            .lock()
            .map(|mut m| m.clear())
            .map_err(|e| format!("Failed to lock messages: {}", e))
    }

    /// Get the number of messages
    pub fn message_count(&self) -> usize {
        self.messages.lock().map(|m| m.len()).unwrap_or(0)
    }
}

impl Default for OutputChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_type_prefix() {
        assert_eq!(MessageType::Info.prefix(), "[INFO]");
        assert_eq!(MessageType::Success.prefix(), "[OK]");
        assert_eq!(MessageType::Error.prefix(), "[ERR]");
        assert_eq!(MessageType::Warning.prefix(), "[W]");
    }

    #[test]
    fn test_output_channel_creation() {
        let channel = OutputChannel::new();
        assert_eq!(channel.message_count(), 0);
    }

    #[test]
    fn test_add_messages() {
        let channel = OutputChannel::new();
        channel.info("Test message".to_string());
        assert_eq!(channel.message_count(), 1);

        channel.success("Success message".to_string());
        assert_eq!(channel.message_count(), 2);

        channel.error("Error message".to_string());
        assert_eq!(channel.message_count(), 3);

        channel.warning("Warning message".to_string());
        assert_eq!(channel.message_count(), 4);
    }

    #[test]
    fn test_clear_messages() {
        let channel = OutputChannel::new();
        channel.info("Test".to_string());
        assert_eq!(channel.message_count(), 1);

        channel.clear().unwrap();
        assert_eq!(channel.message_count(), 0);
    }

    #[test]
    fn test_formatted_output() {
        let channel = OutputChannel::new();
        channel.info("Info message".to_string());
        channel.error("Error message".to_string());

        let output = channel.formatted_output().unwrap();
        assert!(output.contains("[INFO]"));
        assert!(output.contains("[ERR]"));
        assert!(output.contains("Info message"));
        assert!(output.contains("Error message"));
    }
}
