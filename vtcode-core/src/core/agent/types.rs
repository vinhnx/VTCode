use serde::{Deserialize, Serialize};
use std::fmt;

/// Message type classification for intelligent routing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MessageType {
    UserMessage,
    AssistantResponse,
    ToolCall,
    ToolResponse,
    SystemMessage,
}

/// Priority levels for messages and tasks
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Low = 1,
    #[default]
    Medium = 2,
    High = 3,
    Critical = 4,
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
