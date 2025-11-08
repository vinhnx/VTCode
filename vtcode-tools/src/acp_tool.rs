//! MCP Tool for ACP inter-agent communication
//!
//! This tool allows the main agent to:
//! - Discover remote agents
//! - Send requests to remote agents
//! - Manage agent registry
//! - Check agent health

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_acp_client::{AcpClient, AgentRegistry};
use vtcode_core::tools::traits::Tool;

/// ACP Inter-Agent Communication Tool
pub struct AcpTool {
    client: Arc<RwLock<Option<AcpClient>>>,
    registry: Arc<AgentRegistry>,
}

impl AcpTool {
    /// Create a new ACP tool
    pub fn new() -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            registry: Arc::new(AgentRegistry::new()),
        }
    }

    /// Initialize the ACP client with local agent ID
    pub async fn initialize(&self, local_agent_id: String) -> anyhow::Result<()> {
        let client = AcpClient::new(local_agent_id)?;
        let mut locked = self.client.write().await;
        *locked = Some(client);
        Ok(())
    }

    /// Get the registry
    pub fn registry(&self) -> Arc<AgentRegistry> {
        self.registry.clone()
    }
}

impl Default for AcpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for AcpTool {
    fn name(&self) -> &'static str {
        "acp_call"
    }

    fn description(&self) -> &'static str {
        "Call a remote agent via the Agent Communication Protocol (ACP). \
         Allows inter-agent communication for distributed task execution."
    }

    fn validate_args(&self, args: &Value) -> anyhow::Result<()> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Arguments must be an object"))?;

        if !obj.contains_key("action") {
            return Err(anyhow::anyhow!("Missing required field: action"));
        }

        if !obj.contains_key("remote_agent_id") {
            return Err(anyhow::anyhow!("Missing required field: remote_agent_id"));
        }

        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Arguments must be an object"))?;

        let action = obj
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid action"))?;

        let remote_agent_id = obj
            .get("remote_agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid remote_agent_id"))?;

        let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("sync"); // Default to sync

        let call_args = obj.get("args").cloned().unwrap_or(json!({}));

        let client = self.client.read().await;
        let client = client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ACP client not initialized"))?;

        match method {
            "sync" => client
                .call_sync(remote_agent_id, action.to_string(), call_args)
                .await
                .map_err(|e| anyhow::anyhow!("ACP call failed: {}", e)),

            "async" => {
                let message_id = client
                    .call_async(remote_agent_id, action.to_string(), call_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("ACP async call failed: {}", e))?;

                Ok(json!({
                    "message_id": message_id,
                    "status": "queued",
                    "remote_agent_id": remote_agent_id,
                    "action": action,
                }))
            }

            other => Err(anyhow::anyhow!("Unknown method: {}", other)),
        }
    }
}

/// Discovery tool for ACP
pub struct AcpDiscoveryTool {
    client: Arc<RwLock<Option<AcpClient>>>,
}

impl AcpDiscoveryTool {
    pub fn new(client: Arc<RwLock<Option<AcpClient>>>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for AcpDiscoveryTool {
    fn name(&self) -> &'static str {
        "acp_discover"
    }

    fn description(&self) -> &'static str {
        "Discover available agents and their capabilities. \
         Returns agent metadata including supported actions and endpoints."
    }

    fn validate_args(&self, args: &Value) -> anyhow::Result<()> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Arguments must be an object"))?;

        match obj.get("mode").and_then(|v| v.as_str()) {
            Some("by_capability") => {
                if !obj.contains_key("capability") {
                    return Err(anyhow::anyhow!(
                        "Missing required field for by_capability mode: capability"
                    ));
                }
            }
            Some("by_id") => {
                if !obj.contains_key("agent_id") {
                    return Err(anyhow::anyhow!(
                        "Missing required field for by_id mode: agent_id"
                    ));
                }
            }
            Some(other) => return Err(anyhow::anyhow!("Unknown discovery mode: {}", other)),
            None => {} // Default mode (list all)
        }

        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Arguments must be an object"))?;

        let mode = obj
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("list_online");

        let client = self.client.read().await;
        let client = client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ACP client not initialized"))?;

        match mode {
            "list_all" => {
                let agents = client
                    .registry()
                    .list_all()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to list agents: {}", e))?;

                Ok(json!({
                    "agents": agents,
                    "count": agents.len(),
                }))
            }

            "list_online" => {
                let agents = client
                    .registry()
                    .list_online()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to list online agents: {}", e))?;

                Ok(json!({
                    "agents": agents,
                    "count": agents.len(),
                }))
            }

            "by_capability" => {
                let capability = obj
                    .get("capability")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Invalid capability"))?;

                let agents = client
                    .registry()
                    .find_by_capability(capability)
                    .await
                    .map_err(|e| anyhow::anyhow!("Discovery failed: {}", e))?;

                Ok(json!({
                    "capability": capability,
                    "agents": agents,
                    "count": agents.len(),
                }))
            }

            "by_id" => {
                let agent_id = obj
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Invalid agent_id"))?;

                let agent = client
                    .registry()
                    .find(agent_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("Agent not found: {}", e))?;

                Ok(json!(agent))
            }

            other => Err(anyhow::anyhow!("Unknown discovery mode: {}", other)),
        }
    }
}

/// Health check tool for ACP
pub struct AcpHealthTool {
    client: Arc<RwLock<Option<AcpClient>>>,
}

impl AcpHealthTool {
    pub fn new(client: Arc<RwLock<Option<AcpClient>>>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for AcpHealthTool {
    fn name(&self) -> &'static str {
        "acp_health"
    }

    fn description(&self) -> &'static str {
        "Check the health status of remote agents. \
         Returns online/offline status and last seen timestamp."
    }

    fn validate_args(&self, args: &Value) -> anyhow::Result<()> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Arguments must be an object"))?;

        if !obj.contains_key("agent_id") {
            return Err(anyhow::anyhow!("Missing required field: agent_id"));
        }

        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Arguments must be an object"))?;

        let agent_id = obj
            .get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid agent_id"))?;

        let client = self.client.read().await;
        let client = client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ACP client not initialized"))?;

        let is_online = client
            .ping(agent_id)
            .await
            .map_err(|e| anyhow::anyhow!("Health check failed: {}", e))?;

        Ok(json!({
            "agent_id": agent_id,
            "online": is_online,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    }
}
