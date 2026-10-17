use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::ToolPolicy;
use crate::config::mcp::{McpAllowListConfig, McpAllowListRules};
use crate::utils::tool_name_parsing::parse_canonical_mcp_tool_name;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPolicyStore {
    /// Active MCP allow list configuration
    #[serde(default = "default_secure_mcp_allowlist")]
    pub allowlist: McpAllowListConfig,
    /// Provider-specific tool policies (allow/prompt/deny)
    #[serde(default)]
    pub providers: IndexMap<String, McpProviderPolicy>,
}

impl Default for McpPolicyStore {
    fn default() -> Self {
        Self {
            allowlist: default_secure_mcp_allowlist(),
            providers: IndexMap::new(),
        }
    }
}

/// MCP provider policy entry containing per-tool permissions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpProviderPolicy {
    #[serde(default)]
    pub tools: IndexMap<String, ToolPolicy>,
}

// Helper constants to reduce allocations in MCP allowlist configuration
const MCP_LOGGING_EVENTS: &[&str] = &[
    "mcp.tool_execution",
    "mcp.tool_failed",
    "mcp.tool_denied",
    "mcp.tool_filtered",
    "mcp.provider_initialized",
];

const MCP_DEFAULT_LOGGING_EVENTS: &[&str] = &[
    "mcp.provider_initialized",
    "mcp.provider_initialization_failed",
    "mcp.tool_filtered",
    "mcp.tool_execution",
    "mcp.tool_failed",
    "mcp.tool_denied",
];

/// Helper to create standard MCP logging configuration
#[inline]
pub(crate) fn mcp_standard_logging() -> Vec<String> {
    MCP_LOGGING_EVENTS.iter().map(|s| (*s).into()).collect()
}

/// Helper to create provider configuration with max_concurrent_requests
#[inline]
fn mcp_provider_config_with(extra: (&str, Vec<&str>)) -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([
        ("provider".into(), vec!["max_concurrent_requests".into()]),
        (extra.0.into(), extra.1.into_iter().map(Into::into).collect()),
    ])
}

fn default_secure_mcp_allowlist() -> McpAllowListConfig {
    let default_logging = Some(MCP_DEFAULT_LOGGING_EVENTS.iter().map(|s| (*s).into()).collect());

    let default_configuration = Some(BTreeMap::from([
        (
            "client".into(),
            vec![
                "max_concurrent_connections".into(),
                "request_timeout_seconds".into(),
                "retry_attempts".into(),
                "startup_timeout_seconds".into(),
                "tool_timeout_seconds".into(),
                "experimental_use_rmcp_client".into(),
            ],
        ),
        ("ui".into(), vec!["mode".into(), "max_events".into(), "show_provider_names".into()]),
        (
            "server".into(),
            vec![
                "enabled".into(),
                "bind_address".into(),
                "port".into(),
                "transport".into(),
                "name".into(),
                "version".into(),
            ],
        ),
    ]));

    let time_rules = McpAllowListRules {
        tools: Some(vec![
            "get_*".into(),
            "list_*".into(),
            "convert_timezone".into(),
            "describe_timezone".into(),
            "time_*".into(),
        ]),
        resources: Some(vec!["timezone:*".into(), "location:*".into()]),
        logging: Some(mcp_standard_logging()),
        configuration: Some(mcp_provider_config_with(("time", vec!["local_timezone_override"]))),
        ..Default::default()
    };

    let context_rules = McpAllowListRules {
        tools: Some(vec![
            "search_*".into(),
            "fetch_*".into(),
            "list_*".into(),
            "context7_*".into(),
            "get_*".into(),
        ]),
        resources: Some(vec![
            "docs::*".into(),
            "snippets::*".into(),
            "repositories::*".into(),
            "context7::*".into(),
        ]),
        prompts: Some(vec!["context7::*".into(), "support::*".into(), "docs::*".into()]),
        logging: Some(mcp_standard_logging()),
        configuration: Some(mcp_provider_config_with(("context7", vec!["workspace", "search_scope", "max_results"]))),
    };

    let seq_rules = McpAllowListRules {
        tools: Some(vec![
            "plan".into(),
            "critique".into(),
            "reflect".into(),
            "decompose".into(),
            "sequential_*".into(),
        ]),
        resources: None,
        prompts: Some(vec![
            "sequential-thinking::*".into(),
            "plan".into(),
            "reflect".into(),
            "critique".into(),
        ]),
        logging: Some(mcp_standard_logging()),
        configuration: Some(mcp_provider_config_with(("sequencing", vec!["max_depth", "max_branches"]))),
    };

    let mut allowlist = McpAllowListConfig {
        enforce: true,
        default: McpAllowListRules {
            logging: default_logging,
            configuration: default_configuration,
            ..Default::default()
        },
        ..Default::default()
    };

    allowlist.providers.insert("time".into(), time_rules);
    allowlist.providers.insert("context7".into(), context_rules);
    allowlist.providers.insert("sequential-thinking".into(), seq_rules);

    allowlist
}

pub(crate) fn parse_mcp_policy_key(tool_name: &str) -> Option<(String, String)> {
    parse_canonical_mcp_tool_name(tool_name).map(|(provider, tool)| (provider.to_string(), tool.to_string()))
}
