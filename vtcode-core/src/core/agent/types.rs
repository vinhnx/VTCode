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

/// Agent type for VT Code architecture
///
/// Supports both single-agent mode and specialized subagents for task delegation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    /// Single main agent (default mode)
    Single,
    /// Main orchestrator that delegates to subagents
    Orchestrator,
    /// Fast, read-only codebase exploration (haiku-equivalent)
    Explore,
    /// Research specialist for planning mode
    Plan,
    /// General-purpose subagent for complex multi-step tasks
    General,
    /// User-defined custom subagent
    Custom(String),
}

impl Default for AgentType {
    fn default() -> Self {
        Self::Single
    }
}

impl AgentType {
    /// Check if this is a subagent type
    pub fn is_subagent(&self) -> bool {
        matches!(
            self,
            Self::Explore | Self::Plan | Self::General | Self::Custom(_)
        )
    }

    /// Check if this agent operates in read-only mode
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::Explore | Self::Plan)
    }

    /// Get the recommended model tier for this agent type
    pub fn model_tier(&self) -> &'static str {
        match self {
            Self::Single | Self::Orchestrator | Self::General => "sonnet",
            Self::Explore => "haiku",
            Self::Plan => "sonnet",
            Self::Custom(_) => "sonnet", // Default, can be overridden
        }
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single => f.write_str("single"),
            Self::Orchestrator => f.write_str("orchestrator"),
            Self::Explore => f.write_str("explore"),
            Self::Plan => f.write_str("plan"),
            Self::General => f.write_str("general"),
            Self::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

impl std::str::FromStr for AgentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "single" => Ok(Self::Single),
            "orchestrator" => Ok(Self::Orchestrator),
            "explore" => Ok(Self::Explore),
            "plan" => Ok(Self::Plan),
            "general" => Ok(Self::General),
            _ if s.starts_with("custom:") => Ok(Self::Custom(s[7..].to_string())),
            _ => Err(format!("Unknown agent type: {}", s)),
        }
    }
}
