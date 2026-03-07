use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::types::CapabilityLevel;
use crate::mcp::{McpClient, McpToolExecutor, McpToolInfo};
use crate::tool_policy::ToolPolicy;
use crate::tools::registry::ToolRegistration;
use crate::tools::traits::Tool;

pub const MCP_QUALIFIED_TOOL_PREFIX: &str = "mcp__";
const MCP_TOOL_NAME_MAX_LEN: usize = 64;
const MCP_HASH_SUFFIX_LEN: usize = 8;

pub fn is_legacy_mcp_tool_name(name: &str) -> bool {
    name.starts_with("mcp_") && !name.starts_with(MCP_QUALIFIED_TOOL_PREFIX)
}

pub fn legacy_mcp_tool_name(name: &str) -> Option<&str> {
    if is_legacy_mcp_tool_name(name) {
        name.strip_prefix("mcp_")
    } else {
        None
    }
}

pub fn model_visible_mcp_tool_name(provider: &str, tool_name: &str) -> String {
    let provider = sanitize_tool_segment(provider);
    let tool = sanitize_tool_segment(tool_name);
    let qualified = format!("{MCP_QUALIFIED_TOOL_PREFIX}{provider}__{tool}");

    if qualified.len() <= MCP_TOOL_NAME_MAX_LEN {
        return qualified;
    }

    let digest = Sha256::digest(qualified.as_bytes());
    let hash = format!("{:x}", digest);
    let hash = &hash[..MCP_HASH_SUFFIX_LEN];
    let keep = MCP_TOOL_NAME_MAX_LEN.saturating_sub(1 + MCP_HASH_SUFFIX_LEN);
    let prefix = &qualified[..keep];
    format!("{prefix}_{hash}")
}

fn sanitize_tool_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    if out.is_empty() {
        "tool".to_string()
    } else {
        out
    }
}

/// Build a ToolRegistration for a remote MCP tool.
///
/// Naming strategy:
/// - Primary: `mcp::<provider>::<tool>`
/// - Alias: `mcp__<provider>__<tool>` (sanitized and length-capped for model compatibility).
pub fn build_mcp_registration(
    client: Arc<McpClient>,
    provider: &str,
    tool: &McpToolInfo,
    server_hint: Option<String>,
) -> ToolRegistration {
    let primary_name = format!("mcp::{}::{}", provider, tool.name);

    let description = tool.description.as_str();
    let desc_with_hint = match server_hint.as_deref() {
        Some(hint) => format!("{description}\nHint: {hint}"),
        None => description.to_string(),
    };

    let aliases = vec![model_visible_mcp_tool_name(provider, &tool.name)];

    let proxy = McpProxyTool {
        client,
        remote_name: tool.name.clone(),
        input_schema: tool.input_schema.clone(),
    };

    let mut metadata = crate::tools::registry::ToolMetadata::default()
        .with_description(desc_with_hint)
        .with_parameter_schema(tool.input_schema.clone())
        .with_permission(ToolPolicy::Prompt)
        .with_aliases(aliases);
    if let Some(hint) = server_hint {
        metadata = metadata.with_server_hint(hint);
    }

    ToolRegistration::from_tool_with_metadata(
        primary_name,
        CapabilityLevel::Basic,
        Arc::new(proxy),
        metadata,
    )
    .with_llm_visibility(false)
}

struct McpProxyTool {
    client: Arc<McpClient>,
    remote_name: String,
    input_schema: Value,
}

#[async_trait]
impl Tool for McpProxyTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        self.client.execute_mcp_tool(&self.remote_name, &args).await
    }

    fn name(&self) -> &'static str {
        "mcp_proxy"
    }

    fn description(&self) -> &'static str {
        "MCP tool proxy"
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

#[cfg(test)]
mod tests {
    use super::{is_legacy_mcp_tool_name, legacy_mcp_tool_name, model_visible_mcp_tool_name};

    #[test]
    fn model_visible_name_uses_qualified_prefix() {
        let name = model_visible_mcp_tool_name("context7", "search-docs");
        assert_eq!(name, "mcp__context7__search-docs");
    }

    #[test]
    fn model_visible_name_is_capped() {
        let name = model_visible_mcp_tool_name("provider_with_a_very_long_name", &"x".repeat(80));
        assert!(name.len() <= 64);
    }

    #[test]
    fn legacy_detection_ignores_qualified_prefix() {
        assert!(is_legacy_mcp_tool_name("mcp_fetch"));
        assert!(!is_legacy_mcp_tool_name("mcp__context7__search"));
        assert_eq!(legacy_mcp_tool_name("mcp_fetch"), Some("fetch"));
        assert_eq!(legacy_mcp_tool_name("mcp__context7__search"), None);
    }
}
