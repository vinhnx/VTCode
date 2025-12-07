use serde::{Deserialize, Serialize};

/// Configuration for system prompt generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPromptConfig {
    /// Enable verbose mode
    pub verbose: bool,
    /// Include tool descriptions
    pub include_tools: bool,
    /// Include workspace context
    pub include_workspace: bool,
    /// Custom system instruction
    pub custom_instruction: Option<String>,
    /// Agent personality
    pub personality: AgentPersonality,
    /// Response style
    pub response_style: ResponseStyle,
}

impl Default for SystemPromptConfig {
    fn default() -> Self {
        Self {
            verbose: false,
            include_tools: true,
            include_workspace: true,
            custom_instruction: None,
            personality: AgentPersonality::Professional,
            response_style: ResponseStyle::Concise,
        }
    }
}

/// Agent personality options
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum AgentPersonality {
    Professional,
    Friendly,
    Technical,
    Creative,
}

/// Response style options
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum ResponseStyle {
    Concise,
    Detailed,
    Conversational,
    Technical,
}
