use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::types::CapabilityLevel;
use crate::mcp::{McpClient, McpToolExecutor, McpToolInfo};
use crate::tool_policy::ToolPolicy;
use crate::tools::native_cgp_tool_factory;
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

pub fn parse_canonical_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.splitn(3, "::");
    match (parts.next()?, parts.next(), parts.next()) {
        ("mcp", Some(provider), Some(tool)) if !provider.is_empty() && !tool.is_empty() => {
            Some((provider, tool))
        }
        _ => None,
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
    let remote_name = tool.name.clone();
    let input_schema = tool.input_schema.clone();

    let proxy = McpProxyTool {
        client: Arc::clone(&client),
        remote_name: remote_name.clone(),
        input_schema: input_schema.clone(),
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
    .with_native_cgp_factory(native_cgp_tool_factory(move || McpProxyTool {
        client: Arc::clone(&client),
        remote_name: remote_name.clone(),
        input_schema: input_schema.clone(),
    }))
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
    use super::{
        build_mcp_registration, is_legacy_mcp_tool_name, legacy_mcp_tool_name,
        model_visible_mcp_tool_name, parse_canonical_mcp_tool_name,
    };
    use crate::mcp::{McpClient, McpToolInfo};
    use crate::tool_policy::ToolPolicy;
    use crate::tools::CgpRuntimeMode;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

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

    #[test]
    fn parse_canonical_name_extracts_provider_and_tool() {
        assert_eq!(
            parse_canonical_mcp_tool_name("mcp::context7::search-docs"),
            Some(("context7", "search-docs"))
        );
        assert_eq!(parse_canonical_mcp_tool_name("mcp__context7__search"), None);
    }

    #[test]
    fn build_mcp_registration_exposes_native_cgp_factory() {
        let client = Arc::new(McpClient::new(
            vtcode_config::mcp::McpClientConfig::default(),
        ));
        let tool = McpToolInfo {
            name: "search-docs".to_string(),
            description: "Search docs".to_string(),
            provider: "context7".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
        };

        let registration =
            build_mcp_registration(client, "context7", &tool, Some("provider hint".to_string()));
        let native_factory = registration
            .native_cgp_factory()
            .expect("MCP registration should expose native CGP factory");
        let wrapped = native_factory(
            &registration,
            PathBuf::from("/tmp/test"),
            CgpRuntimeMode::Interactive,
        );

        assert_eq!(wrapped.name(), "mcp::context7::search-docs");
        assert_eq!(wrapped.description(), "Search docs\nHint: provider hint");
        assert_eq!(wrapped.parameter_schema(), Some(tool.input_schema.clone()));
        assert_eq!(wrapped.default_permission(), ToolPolicy::Prompt);
    }
}
