//! Agent Card for A2A Protocol
//!
//! Implements the Agent Card structure used for agent discovery and capability
//! advertisement, typically served at `/.well-known/agent-card.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A2A Protocol version
pub const A2A_PROTOCOL_VERSION: &str = "1.0";

/// Agent Card - metadata describing an A2A agent
///
/// Agent Cards are used for agent discovery. They are typically served at
/// `/.well-known/agent-card.json` and describe the agent's identity, capabilities,
/// and security requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    /// The version of the A2A protocol supported
    pub protocol_version: String,
    /// Agent name
    pub name: String,
    /// Agent description
    pub description: String,
    /// Agent version
    pub version: String,
    /// The preferred endpoint URL for the agent's A2A service
    pub url: String,
    /// Organization/provider details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,
    /// Features supported by this agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<AgentCapabilities>,
    /// Default supported input MIME types
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub default_input_modes: Vec<String>,
    /// Default supported output MIME types
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub default_output_modes: Vec<String>,
    /// List of specific skills/capabilities
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skills: Vec<AgentSkill>,
    /// Security schemes following OpenAPI specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_schemes: Option<HashMap<String, serde_json::Value>>,
    /// Security requirements
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<HashMap<String, Vec<String>>>>,
    /// Whether a more detailed card is available post-authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_authenticated_extended_card: Option<bool>,
    /// JWS signatures for card verification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<AgentCardSignature>>,
}

impl AgentCard {
    /// Create a new Agent Card with required fields
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: A2A_PROTOCOL_VERSION.to_string(),
            name: name.into(),
            description: description.into(),
            version: version.into(),
            url: String::new(),
            provider: None,
            capabilities: None,
            default_input_modes: vec!["text/plain".to_string()],
            default_output_modes: vec!["text/plain".to_string()],
            skills: Vec::new(),
            security_schemes: None,
            security: None,
            supports_authenticated_extended_card: None,
            signatures: None,
        }
    }

    /// Create a default VTCode agent card
    pub fn vtcode_default(url: impl Into<String>) -> Self {
        let mut card = Self::new(
            "vtcode-agent",
            "VTCode AI coding agent - a terminal-based coding assistant supporting multiple LLM providers",
            env!("CARGO_PKG_VERSION"),
        );
        card.url = url.into();
        card.provider = Some(AgentProvider {
            organization: "VTCode".to_string(),
            url: Some("https://github.com/vinhnx/vtcode".to_string()),
        });
        card.capabilities = Some(AgentCapabilities {
            streaming: true,
            push_notifications: false,
            state_transition_history: true,
            extensions: Vec::new(),
        });
        card.default_input_modes = vec![
            "text/plain".to_string(),
            "application/json".to_string(),
        ];
        card.default_output_modes = vec![
            "text/plain".to_string(),
            "application/json".to_string(),
            "text/markdown".to_string(),
        ];
        card
    }

    /// Set the URL
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// Set the provider
    pub fn with_provider(mut self, provider: AgentProvider) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Set the capabilities
    pub fn with_capabilities(mut self, capabilities: AgentCapabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }

    /// Add a skill
    pub fn add_skill(mut self, skill: AgentSkill) -> Self {
        self.skills.push(skill);
        self
    }

    /// Check if streaming is supported
    pub fn supports_streaming(&self) -> bool {
        self.capabilities
            .as_ref()
            .map(|c| c.streaming)
            .unwrap_or(false)
    }

    /// Check if push notifications are supported
    pub fn supports_push_notifications(&self) -> bool {
        self.capabilities
            .as_ref()
            .map(|c| c.push_notifications)
            .unwrap_or(false)
    }
}

/// Agent provider/organization details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProvider {
    /// Organization name
    pub organization: String,
    /// Organization URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Agent capabilities declaration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// Whether streaming via SSE is supported
    #[serde(default)]
    pub streaming: bool,
    /// Whether push notifications are supported
    #[serde(default)]
    pub push_notifications: bool,
    /// Whether state transition history is maintained
    #[serde(default)]
    pub state_transition_history: bool,
    /// List of supported extensions
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extensions: Vec<String>,
}

impl AgentCapabilities {
    /// Create capabilities with streaming enabled
    pub fn with_streaming() -> Self {
        Self {
            streaming: true,
            ..Default::default()
        }
    }

    /// Create capabilities with all features enabled
    pub fn full() -> Self {
        Self {
            streaming: true,
            push_notifications: true,
            state_transition_history: true,
            extensions: Vec::new(),
        }
    }
}

/// A specific capability/skill of an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    /// Unique skill identifier
    pub id: String,
    /// Human-readable skill name
    pub name: String,
    /// Skill description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Tags for categorization
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
    /// Example inputs/outputs
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub examples: Vec<SkillExample>,
    /// Input modes specific to this skill
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_modes: Option<Vec<String>>,
    /// Output modes specific to this skill
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_modes: Option<Vec<String>>,
}

impl AgentSkill {
    /// Create a new skill
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            tags: Vec::new(),
            examples: Vec::new(),
            input_modes: None,
            output_modes: None,
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Add an example
    pub fn add_example(mut self, example: SkillExample) -> Self {
        self.examples.push(example);
        self
    }
}

/// Example input/output for a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExample {
    /// Example input
    pub input: String,
    /// Example output
    pub output: String,
}

impl SkillExample {
    /// Create a new example
    pub fn new(input: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
        }
    }
}

/// Agent Card signature for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCardSignature {
    /// Algorithm used
    pub algorithm: String,
    /// Key ID
    pub key_id: String,
    /// The signature value
    pub signature: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_card_creation() {
        let card = AgentCard::new("test-agent", "A test agent", "1.0.0");
        assert_eq!(card.name, "test-agent");
        assert_eq!(card.protocol_version, A2A_PROTOCOL_VERSION);
    }

    #[test]
    fn test_vtcode_default_card() {
        let card = AgentCard::vtcode_default("http://localhost:8080");
        assert_eq!(card.name, "vtcode-agent");
        assert_eq!(card.url, "http://localhost:8080");
        assert!(card.supports_streaming());
        assert!(!card.supports_push_notifications());
    }

    #[test]
    fn test_agent_card_serialization() {
        let card = AgentCard::vtcode_default("http://localhost:8080");
        let json = serde_json::to_string_pretty(&card).expect("serialize");
        assert!(json.contains("\"protocolVersion\""));
        assert!(json.contains("vtcode-agent"));
    }

    #[test]
    fn test_agent_skill() {
        let skill = AgentSkill::new("code-gen", "Code Generation")
            .with_description("Generate code from natural language")
            .with_tags(vec!["coding".to_string(), "generation".to_string()])
            .add_example(SkillExample::new(
                "Create a Python function to sort a list",
                "def sort_list(items): return sorted(items)",
            ));

        assert_eq!(skill.id, "code-gen");
        assert_eq!(skill.tags.len(), 2);
        assert_eq!(skill.examples.len(), 1);
    }

    #[test]
    fn test_capabilities() {
        let caps = AgentCapabilities::full();
        assert!(caps.streaming);
        assert!(caps.push_notifications);
        assert!(caps.state_transition_history);
    }
}
