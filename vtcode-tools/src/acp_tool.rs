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

/// Shared utilities for ACP tools to reduce duplication
mod shared {
    use super::*;

    const ERR_ARGS_OBJECT: &str = "Arguments must be an object";
    const ERR_CLIENT_UNINITIALIZED: &str = "ACP client not initialized";
    /// Maximum allowed length for agent IDs to prevent DoS via oversized strings.
    const MAX_AGENT_ID_LEN: usize = 256;
    /// Maximum allowed length for action names.
    const MAX_ACTION_LEN: usize = 128;
    /// Maximum JSON depth for call_args to prevent stack overflow.
    const MAX_JSON_DEPTH: usize = 32;
    /// Maximum size for call_args payload in bytes.
    const MAX_ARGS_SIZE: usize = 1024 * 1024; // 1MB

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

    /// Validate agent ID format: alphanumeric, hyphens, underscores only, length limit.
    pub fn validate_agent_id(agent_id: &str) -> anyhow::Result<()> {
        if agent_id.is_empty() {
            return Err(anyhow::anyhow!("agent_id cannot be empty"));
        }
        if agent_id.len() > MAX_AGENT_ID_LEN {
            return Err(anyhow::anyhow!(
                "agent_id exceeds maximum length of {} characters",
                MAX_AGENT_ID_LEN
            ));
        }
        if !agent_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(anyhow::anyhow!(
                "agent_id contains invalid characters (allowed: alphanumeric, hyphen, underscore, dot)"
            ));
        }
        Ok(())
    }

    /// Validate action name format.
    pub fn validate_action(action: &str) -> anyhow::Result<()> {
        if action.is_empty() {
            return Err(anyhow::anyhow!("action cannot be empty"));
        }
        if action.len() > MAX_ACTION_LEN {
            return Err(anyhow::anyhow!(
                "action exceeds maximum length of {} characters",
                MAX_ACTION_LEN
            ));
        }
        if !action
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(anyhow::anyhow!(
                "action contains invalid characters"
            ));
        }
        Ok(())
    }

    /// Validate call_args size and depth.
    pub fn validate_call_args(args: &Value) -> anyhow::Result<()> {
        let serialized = serde_json::to_string(args)
            .map_err(|e| anyhow::anyhow!("Failed to serialize args: {}", e))?;
        if serialized.len() > MAX_ARGS_SIZE {
            return Err(anyhow::anyhow!(
                "call_args exceeds maximum size of {} bytes",
                MAX_ARGS_SIZE
            ));
        }
        if json_depth(args) > MAX_JSON_DEPTH {
            return Err(anyhow::anyhow!(
                "call_args exceeds maximum nesting depth of {}",
                MAX_JSON_DEPTH
            ));
        }
        Ok(())
    }

    /// Calculate JSON nesting depth.
    fn json_depth(value: &Value) -> usize {
        match value {
            Value::Array(arr) => 1 + arr.iter().map(json_depth).max().unwrap_or(0),
            Value::Object(obj) => 1 + obj.values().map(json_depth).max().unwrap_or(0),
            _ => 0,
        }
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
    fn name(&self) -> &'static str {
        "acp_call"
    }

    fn description(&self) -> &'static str {
        "Call a remote agent via the Agent Communication Protocol (ACP). \
         Allows inter-agent communication for distributed task execution."
    }

    fn validate_args(&self, args: &Value) -> anyhow::Result<()> {
        let obj = shared::extract_args_object(args)?;
        shared::validate_field_exists(obj, "action")?;
        shared::validate_field_exists(obj, "remote_agent_id")?;
        // Validate formats
        if let Some(action) = obj.get("action").and_then(|v| v.as_str()) {
            shared::validate_action(action)?;
        }
        if let Some(agent_id) = obj.get("remote_agent_id").and_then(|v| v.as_str()) {
            shared::validate_agent_id(agent_id)?;
        }
        // Validate method if provided
        if let Some(method) = obj.get("method").and_then(|v| v.as_str()) {
            if method != "sync" && method != "async" {
                return Err(anyhow::anyhow!("Invalid method '{}': must be 'sync' or 'async'", method));
            }
        }
        // Validate call_args if provided
        if let Some(call_args) = obj.get("args") {
            shared::validate_call_args(call_args)?;
        }
        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = shared::extract_args_object(&args)?;

        let action = shared::get_required_field(obj, "action", None)?;
        let remote_agent_id = shared::get_required_field(obj, "remote_agent_id", None)?;
        let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("sync");

        // Validate inputs before use
        shared::validate_action(action)?;
        shared::validate_agent_id(remote_agent_id)?;

        let call_args = obj.get("args").cloned().unwrap_or(json!({}));
        shared::validate_call_args(&call_args)?;

        let client = self.client.read().await;
        let client = shared::check_client_initialized(&*client)?;

        match method {
            "sync" => client
                .call_sync(remote_agent_id, action.into(), call_args)
                .await
                .map_err(|e| anyhow::anyhow!("ACP call failed: {}", e)),

            "async" => {
                let message_id = client
                    .call_async(remote_agent_id, action.into(), call_args)
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
        let client = shared::check_client_initialized(&*client)?;

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
                let capability = shared::get_required_field(obj, "capability", None)?;

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
                let agent_id = shared::get_required_field(obj, "agent_id", None)?;

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
        let obj = shared::extract_args_object(args)?;
        shared::validate_field_exists(obj, "agent_id")?;
        Ok(())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let obj = shared::extract_args_object(&args)?;
        let agent_id = shared::get_required_field(obj, "agent_id", None)?;

        let client = self.client.read().await;
        let client = shared::check_client_initialized(&*client)?;

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
