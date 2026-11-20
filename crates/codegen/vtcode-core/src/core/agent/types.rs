use serde::{Deserialize, Serialize};
use std::fmt;

/// Message type classification for intelligent routing
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MessageType {
    UserMessage,
    AssistantResponse,
    ToolCall,
    ToolResponse,
    SystemMessage,
}

/// Priority levels for messages and tasks
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Low = 1,
    #[default]
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Agent type for VT Code architecture
///
/// Supports both single-agent mode and specialized profiles for task delegation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AgentType {
    /// Single main agent (default mode)
    #[default]
    Single,
    /// Main orchestrator that delegates to specialized profiles.
    Orchestrator,
    /// Fast, read-only codebase exploration (haiku-equivalent)
    Explore,
    /// Research specialist for the Planning workflow.
    Plan,
    /// General-purpose profile for complex multi-step tasks.
    General,
    /// User-defined custom profile.
    Custom(String),
}

impl AgentType {
    /// Canonical built-in names, shared by `Display` and `FromStr`.
    const SINGLE: &'static str = "single";
    const ORCHESTRATOR: &'static str = "orchestrator";
    const EXPLORE: &'static str = "explore";
    const PLAN: &'static str = "plan";
    const GENERAL: &'static str = "general";
    const CUSTOM_PREFIX: &'static str = "custom:";

    /// Check if this is a specialized profile type.
    pub fn is_specialized(&self) -> bool {
        matches!(self, Self::Explore | Self::Plan | Self::General | Self::Custom(_))
    }

    /// Check if this agent has read-only capabilities.
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::Explore | Self::Plan)
    }

    /// Returns `true` when this agent type implies planning workflow is active.
    ///
    /// `AgentType::Plan` is the research specialist for the planning workflow;
    /// when it is the active agent type, planning workflow read-only enforcement
    /// should be enabled.
    pub fn implies_planning_workflow(&self) -> bool {
        matches!(self, Self::Plan)
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
            Self::Single => f.write_str(Self::SINGLE),
            Self::Orchestrator => f.write_str(Self::ORCHESTRATOR),
            Self::Explore => f.write_str(Self::EXPLORE),
            Self::Plan => f.write_str(Self::PLAN),
            Self::General => f.write_str(Self::GENERAL),
            Self::Custom(name) => write!(f, "{}{name}", Self::CUSTOM_PREFIX),
        }
    }
}

impl std::str::FromStr for AgentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            Self::SINGLE => Ok(Self::Single),
            Self::ORCHESTRATOR => Ok(Self::Orchestrator),
            Self::EXPLORE => Ok(Self::Explore),
            Self::PLAN => Ok(Self::Plan),
            Self::GENERAL => Ok(Self::General),
            _ if s.starts_with(Self::CUSTOM_PREFIX) => Ok(Self::Custom(s[Self::CUSTOM_PREFIX.len()..].to_string())),
            _ => Err(format!("Unknown agent type: {s}")),
        }
    }
}
