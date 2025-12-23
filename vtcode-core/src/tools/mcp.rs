use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::config::types::CapabilityLevel;
use crate::mcp::{McpClient, McpToolExecutor, McpToolInfo};
use crate::tool_policy::ToolPolicy;
use crate::tools::registry::ToolRegistration;
use crate::tools::traits::Tool;

/// Build a ToolRegistration for a remote MCP tool.
///
/// Naming strategy:
/// - Primary: `mcp::<provider>::<tool>`
/// - Aliases: `mcp_<tool>` and `mcp_<provider>_<tool>` for backward compatibility.
pub fn build_mcp_registration(
    client: Arc<McpClient>,
    provider: &str,
    tool: &McpToolInfo,
    server_hint: Option<String>,
) -> ToolRegistration {
    let primary_name = format!("mcp::{}::{}", provider, tool.name);
    // Leak to obtain &'static str for ToolRegistration; number of MCP tools is bounded by provider output.
    let primary_static: &'static str = Box::leak(primary_name.into_boxed_str());

    let description = tool.description.as_str();
    let desc_with_hint = match server_hint {
        Some(hint) => format!("{description}\nHint: {hint}"),
        None => description.to_string(),
    };
    let description_static: &'static str = Box::leak(desc_with_hint.clone().into_boxed_str());

    let aliases = vec![
        format!("mcp_{}", tool.name),
        format!("mcp_{}_{}", provider, tool.name),
    ];

    let proxy = McpProxyTool {
        client,
        remote_name: tool.name.clone(),
        name: primary_static,
        description: description_static,
        input_schema: tool.input_schema.clone(),
    };

    ToolRegistration::from_tool_instance(primary_static, CapabilityLevel::Basic, proxy)
        .with_parameter_schema(tool.input_schema.clone())
        .with_permission(ToolPolicy::Prompt)
        .with_aliases(aliases)
        .with_server_hint(desc_with_hint)
}

struct McpProxyTool {
    client: Arc<McpClient>,
    remote_name: String,
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

#[async_trait]
impl Tool for McpProxyTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        self.client.execute_mcp_tool(&self.remote_name, &args).await
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(self.input_schema.clone())
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        None
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Prompt
    }
}


