//! Agent discovery and registry functionality

use crate::error::{AcpError, AcpResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Information about a registered agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Unique agent identifier
    pub id: String,

    /// Agent display name
    pub name: String,

    /// Base URL for agent communication
    pub base_url: String,

    /// Agent description
    pub description: Option<String>,

    /// Supported actions/tools
    pub capabilities: Vec<String>,

    /// Agent metadata (version, tags, etc.)
    #[serde(default)]
    pub metadata: HashMap<String, Value>,

    /// Whether agent is currently online
    #[serde(default = "default_online")]
    pub online: bool,

    /// Last heartbeat/update timestamp
    pub last_seen: Option<String>,
}

fn default_online() -> bool {
    true
}

/// Agent registry for discovery and lookup
#[derive(Clone)]
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<String, AgentInfo>>>,
}

impl AgentRegistry {
    /// Create a new agent registry
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an agent
    pub async fn register(&self, agent: AgentInfo) -> AcpResult<()> {
        let mut agents = self.agents.write().await;
        agents.insert(agent.id.clone(), agent);
        Ok(())
    }

    /// Unregister an agent
    pub async fn unregister(&self, agent_id: &str) -> AcpResult<()> {
        let mut agents = self.agents.write().await;
        agents.remove(agent_id);
        Ok(())
    }

    /// Find agent by ID
    pub async fn find(&self, agent_id: &str) -> AcpResult<AgentInfo> {
        let agents = self.agents.read().await;
        agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| AcpError::AgentNotFound(agent_id.to_string()))
    }

    /// Find agents by capability
    pub async fn find_by_capability(&self, capability: &str) -> AcpResult<Vec<AgentInfo>> {
        let agents = self.agents.read().await;
        let matching = agents
            .values()
            .filter(|a| a.online && a.capabilities.contains(&capability.to_string()))
            .cloned()
            .collect();
        Ok(matching)
    }

    /// List all registered agents
    pub async fn list_all(&self) -> AcpResult<Vec<AgentInfo>> {
        let agents = self.agents.read().await;
        Ok(agents.values().cloned().collect())
    }

    /// List online agents
    pub async fn list_online(&self) -> AcpResult<Vec<AgentInfo>> {
        let agents = self.agents.read().await;
        Ok(agents.values().filter(|a| a.online).cloned().collect())
    }

    /// Update agent status
    pub async fn update_status(&self, agent_id: &str, online: bool) -> AcpResult<()> {
        let mut agents = self.agents.write().await;
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.online = online;
            agent.last_seen = Some(chrono::Utc::now().to_rfc3339());
            Ok(())
        } else {
            Err(AcpError::AgentNotFound(agent_id.to_string()))
        }
    }

    /// Get agent count
    pub async fn count(&self) -> usize {
        self.agents.read().await.len()
    }

    /// Clear all agents
    pub async fn clear(&self) {
        self.agents.write().await.clear();
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_registry() {
        let registry = AgentRegistry::new();

        let agent = AgentInfo {
            id: "test-agent".to_string(),
            name: "Test Agent".to_string(),
            base_url: "http://localhost:8080".to_string(),
            description: Some("A test agent".to_string()),
            capabilities: vec!["bash".to_string(), "python".to_string()],
            metadata: HashMap::new(),
            online: true,
            last_seen: None,
        };

        registry.register(agent.clone()).await.unwrap();

        let found = registry.find("test-agent").await.unwrap();
        assert_eq!(found.id, "test-agent");

        assert_eq!(registry.count().await, 1);

        registry.unregister("test-agent").await.unwrap();
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_find_by_capability() {
        let registry = AgentRegistry::new();

        let agent1 = AgentInfo {
            id: "agent-1".to_string(),
            name: "Agent 1".to_string(),
            base_url: "http://localhost:8080".to_string(),
            description: None,
            capabilities: vec!["bash".to_string()],
            metadata: HashMap::new(),
            online: true,
            last_seen: None,
        };

        let agent2 = AgentInfo {
            id: "agent-2".to_string(),
            name: "Agent 2".to_string(),
            base_url: "http://localhost:8081".to_string(),
            description: None,
            capabilities: vec!["bash".to_string(), "python".to_string()],
            metadata: HashMap::new(),
            online: true,
            last_seen: None,
        };

        registry.register(agent1).await.unwrap();
        registry.register(agent2).await.unwrap();

        let bash_agents = registry.find_by_capability("bash").await.unwrap();
        assert_eq!(bash_agents.len(), 2);

        let python_agents = registry.find_by_capability("python").await.unwrap();
        assert_eq!(python_agents.len(), 1);
    }
}
