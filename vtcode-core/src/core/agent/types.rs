//! Type definitions for the compaction system

use serde::{Deserialize, Serialize};
use std::fmt;

/// Compacted message representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactedMessage {
    /// Original message timestamp (Unix timestamp)
    pub timestamp: u64,
    /// Message type (user/assistant/tool)
    pub message_type: MessageType,
    /// Compacted content summary
    pub summary: String,
    /// Key information extracted
    pub key_info: Vec<String>,
    /// Compression ratio achieved
    pub compression_ratio: f64,
    /// Original message size (bytes)
    pub original_size: usize,
}

/// Message type classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    UserMessage,
    AssistantResponse,
    ToolCall,
    ToolResponse,
    SystemMessage,
}

/// Message priority levels for intelligent compaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MessagePriority {
    Low = 1,      // Can be compacted easily (routine tool calls, simple acknowledgments)
    Medium = 2,   // Moderate importance (general responses, standard operations)
    High = 3,     // Should be preserved (important decisions, code changes, user requests)
    Critical = 4, // Must be preserved (security operations, critical errors, user constraints)
}

/// Enhanced message with priority and semantic information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedMessage {
    pub base_message: CompactedMessage,
    pub priority: MessagePriority,
    pub semantic_tags: Vec<String>,
    pub context_references: Vec<String>,
    pub conversation_turn: usize,
    pub related_messages: Vec<usize>,
}

/// Compacted context data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactedContext {
    /// Timestamp of compaction (Unix timestamp)
    pub compacted_at: u64,
    /// Original context size
    pub original_size: usize,
    /// Compacted context size
    pub compacted_size: usize,
    /// Context key/identifier
    pub context_key: String,
    /// Compression ratio achieved
    pub compression_ratio: f64,
    /// Context confidence score
    pub confidence_score: f64,
}

/// Compaction suggestion for intelligent decision making
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionSuggestion {
    /// Suggested action
    pub action: String,
    /// Urgency level
    pub urgency: Urgency,
    /// Estimated memory savings
    pub estimated_savings: usize,
    /// Reasoning for the suggestion
    pub reasoning: String,
}

/// Urgency levels for compaction suggestions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Urgency {
    Low = 1,
    Medium = 2,
    High = 3,
}

/// Compaction operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    /// Number of messages processed
    pub messages_processed: usize,
    /// Number of messages compacted
    pub messages_compacted: usize,
    /// Total original size (bytes)
    pub original_size: usize,
    /// Total compacted size (bytes)
    pub compacted_size: usize,
    /// Overall compression ratio
    pub compression_ratio: f64,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
}

/// Compaction statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionStatistics {
    /// Total messages in history
    pub total_messages: usize,
    /// Messages by priority level
    pub messages_by_priority: std::collections::HashMap<MessagePriority, usize>,
    /// Total memory usage (bytes)
    pub total_memory_usage: usize,
    /// Average message size (bytes)
    pub average_message_size: usize,
    /// Last compaction timestamp
    pub last_compaction_timestamp: u64,
    /// Compaction frequency (operations per hour)
    pub compaction_frequency: f64,
}

/// Agent type for single-agent architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    Single,
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single => f.write_str("single"),
        }
    }
}
