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
use vtcode_acp::{AcpClient, AgentRegistry};
use vtcode_core::tools::traits::Tool;

/// Shared utilities for ACP tools to reduce duplication
mod shared {
    use super::*;

    const ERR_ARGS_OBJECT: &str = "Arguments must be an object";
    const ERR_CLIENT_UNINITIALIZED: &str = "ACP client not initialized";

    pub fn extract_args_object(args: &Value) -> anyhow::Result<&serde_json::Map<String, Value>> {
        args.as_object()
            .ok_or_else(|| anyhow::anyhow!(ERR_ARGS_OBJECT))
    }

    pub fn get_required_field<'a>(
        obj: &'a serde_json::Map<String, Value>,
        field: &str,
        custom_err: Option<&'static str>,
    ) -> anyhow::Result<&'a str> {
        obj.get(field).and_then(|v| v.as_str()).ok_or_else(|| {
            if let Some(err) = custom_err {
                anyhow::anyhow!(err)
            } else {
                anyhow::anyhow!("Invalid {}", field)
            }
        })
    }

    pub fn check_client_initialized(client: &Option<AcpClient>) -> anyhow::Result<&AcpClient> {
        client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!(ERR_CLIENT_UNINITIALIZED))
    }

    pub fn validate_field_exists(
        obj: &serde_json::Map<String, Value>,
        field: &str,
    ) -> anyhow::Result<()> {
        if !obj.contains_key(field) {
            return Err(anyhow::anyhow!("Missing required field: {}", field));
        }
        Ok(())
    }

    /// Wrap an ACP-layer error with a human-readable context prefix.
    /// Preserves the prior `<context>: <error>` message format.
    pub fn wrap<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> anyhow::Result<T> {
        result.map_err(|e| anyhow::anyhow!("{}: {}", context, e))
    }
}

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
        Arc::clone(&self.registry)
    }
}

impl Default for AcpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for AcpTool {
    fn name(&self) -> &str {
        "acp_call"
    }

    fn description(&self) -> &str {
        "Call a remote agent via the Agent Communication Protocol (ACP). \
         Allows inter-agent communication for distributed task execution."
    }

    fn validate_args(&self, args: &Value) -> anyhow::Result<()> {
        let obj = shared::extract_args_object(args)?;
        shared::validate_field_exists(obj, "action")?;
        shared::validate_field_exists(obj, "remote_agent_id")?;
        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = shared::extract_args_object(&args)?;

        let action = shared::get_required_field(obj, "action", None)?;
        let remote_agent_id = shared::get_required_field(obj, "remote_agent_id", None)?;
        let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("sync");

        let call_args = obj.get("args").cloned().unwrap_or(json!({}));

        let client = self.client.read().await;
        let client = shared::check_client_initialized(&client)?;

        match method {
            "sync" => shared::wrap(
                client
                    .call_sync(remote_agent_id, action.into(), call_args)
                    .await,
                "ACP call failed",
            ),

            "async" => {
                let message_id = shared::wrap(
                    client
                        .call_async(remote_agent_id, action.into(), call_args)
                        .await,
                    "ACP async call failed",
                )?;

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
    fn name(&self) -> &str {
        "acp_discover"
    }

    fn description(&self) -> &str {
        "Discover available agents and their capabilities. \
         Returns agent metadata including supported actions and endpoints."
    }

    fn validate_args(&self, args: &Value) -> anyhow::Result<()> {
        let obj = shared::extract_args_object(args)?;

        match obj.get("mode").and_then(|v| v.as_str()) {
            Some("by_capability") => shared::validate_field_exists(obj, "capability")?,
            Some("by_id") => shared::validate_field_exists(obj, "agent_id")?,
            Some(other) => return Err(anyhow::anyhow!("Unknown discovery mode: {}", other)),
            None => {}
        }

        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = shared::extract_args_object(&args)?;
        let mode = obj
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("list_online");

        let client = self.client.read().await;
        let client = shared::check_client_initialized(&client)?;

        match mode {
            "list_all" => {
                let agents =
                    shared::wrap(client.registry().list_all().await, "Failed to list agents")?;

                Ok(json!({
                    "agents": agents,
                    "count": agents.len(),
                }))
            }

            "list_online" => {
                let agents = shared::wrap(
                    client.registry().list_online().await,
                    "Failed to list online agents",
                )?;

                Ok(json!({
                    "agents": agents,
                    "count": agents.len(),
                }))
            }

            "by_capability" => {
                let capability = shared::get_required_field(obj, "capability", None)?;

                let agents = shared::wrap(
                    client.registry().find_by_capability(capability).await,
                    "Discovery failed",
                )?;

                Ok(json!({
                    "capability": capability,
                    "agents": agents,
                    "count": agents.len(),
                }))
            }

            "by_id" => {
                let agent_id = shared::get_required_field(obj, "agent_id", None)?;

                let agent =
                    shared::wrap(client.registry().find(agent_id).await, "Agent not found")?;

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
    fn name(&self) -> &str {
        "acp_health"
    }

    fn description(&self) -> &str {
        "Check the health status of remote agents. \
         Returns online/offline status and last seen timestamp."
    }

    fn validate_args(&self, args: &Value) -> anyhow::Result<()> {
        let obj = shared::extract_args_object(args)?;
        shared::validate_field_exists(obj, "agent_id")?;
        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = shared::extract_args_object(&args)?;
        let agent_id = shared::get_required_field(obj, "agent_id", None)?;

        let client = self.client.read().await;
        let client = shared::check_client_initialized(&client)?;

        let is_online = shared::wrap(client.ping(agent_id).await, "Health check failed")?;

        Ok(json!({
            "agent_id": agent_id,
            "online": is_online,
            "timestamp": crate::compat::current_timestamp_rfc3339(),
        }))
    }
}
