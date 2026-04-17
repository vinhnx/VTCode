//! Tool discovery and search functionality for MCP tools.
//!
//! This module implements progressive disclosure of MCP tools to agents,
//! allowing for context-efficient tool discovery without flooding the
//! model's context with full tool schemas.
//!
//! # Example
//!
//! ```ignore
//! let discovery = ToolDiscovery::new(mcp_client);
//!
//! // Search for tools by keyword
//! let results = discovery.search_tools("file", DetailLevel::NameOnly).await?;
//!
//! // Get detailed schema for a specific tool
//! let detail = discovery.get_tool_detail("read_file").await?;
//! ```

use crate::mcp::McpToolInfo;
use anyhow::Result;
use serde_json::Value;
use std::cmp::Ordering;
use std::sync::Arc;
use tracing::{debug, info};

/// Level of detail returned in tool search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailLevel {
    /// Only tool name (minimal context)
    NameOnly,
    /// Name and description (default)
    NameAndDescription,
    /// Full schema including input parameters
    Full,
}

impl DetailLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NameOnly => "name-only",
            Self::NameAndDescription => "name-and-description",
            Self::Full => "full",
        }
    }
}

/// Result of a tool discovery operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDiscoveryResult {
    pub name: String,
    pub provider: String,
    pub description: String,
    pub relevance_score: f32,
    /// Present only when detail_level is Full or NameAndDescription
    pub input_schema: Option<Value>,
}

impl ToolDiscoveryResult {
    /// Serialize to compact JSON based on detail level.
    pub fn to_json(&self, detail_level: DetailLevel) -> Value {
        match detail_level {
            DetailLevel::NameOnly => serde_json::json!({
                "name": self.name,
                "provider": self.provider,
            }),
            DetailLevel::NameAndDescription => serde_json::json!({
                "name": self.name,
                "provider": self.provider,
                "description": self.description,
            }),
            DetailLevel::Full => serde_json::json!({
                "name": self.name,
                "provider": self.provider,
                "description": self.description,
                "input_schema": self.input_schema,
            }),
        }
    }
}

/// Tool discovery service for progressive disclosure of MCP tools.
pub struct ToolDiscovery {
    mcp_client: Arc<dyn crate::mcp::McpToolExecutor>,
}

fn group_results_by_provider_preserving_order(
    tools: impl IntoIterator<Item = ToolDiscoveryResult>,
) -> Vec<(String, Vec<ToolDiscoveryResult>)> {
    let mut grouped: Vec<(String, Vec<ToolDiscoveryResult>)> = Vec::new();

    for tool in tools {
        let provider = tool.provider.clone();
        if let Some((_, provider_tools)) = grouped
            .iter_mut()
            .find(|(existing_provider, _)| *existing_provider == provider)
        {
            provider_tools.push(tool);
        } else {
            grouped.push((provider, vec![tool]));
        }
    }

    grouped
}

impl ToolDiscovery {
    /// Create a new tool discovery service.
    pub fn new(mcp_client: Arc<dyn crate::mcp::McpToolExecutor>) -> Self {
        Self { mcp_client }
    }

    /// Search for tools by keyword with configurable detail level.
    ///
    /// This implements progressive disclosure: agents can search with
    /// low detail to find relevant tools, then request full schemas
    /// only for tools they intend to use.
    ///
    /// Follows AGENTS.md guidelines: limits results to 5 items with overflow indication.
    pub async fn search_tools(
        &self,
        keyword: &str,
        detail_level: DetailLevel,
    ) -> Result<Vec<ToolDiscoveryResult>> {
        let tools = self.mcp_client.list_mcp_tools().await?;

        debug!(
            keyword = keyword,
            count = tools.len(),
            "Searching tools for keyword"
        );

        // Pre-allocate with estimated capacity
        let mut results = Vec::with_capacity(tools.len() / 4);

        for tool in tools {
            let relevance_score = self.calculate_relevance(&tool, keyword);

            // Filter out tools with no relevance
            if relevance_score <= 0.0 {
                continue;
            }

            // Only clone input_schema when needed (Full detail level)
            let input_schema = match detail_level {
                DetailLevel::Full => Some(tool.input_schema.clone()),
                _ => None,
            };

            results.push(ToolDiscoveryResult {
                name: tool.name.clone(),
                provider: tool.provider.clone(),
                description: tool.description.clone(),
                relevance_score,
                input_schema,
            });
        }

        // Sort by relevance score (highest first)
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(Ordering::Equal)
        });

        // Apply AGENTS.md compliance: limit to 5 results with overflow indication
        let total_results = results.len();
        if total_results > 5 {
            info!(
                keyword = keyword,
                matched = total_results,
                displayed = 5,
                overflow = total_results - 5,
                detail_level = detail_level.as_str(),
                "Tool search completed with overflow"
            );
            results.truncate(5);
        } else {
            info!(
                keyword = keyword,
                matched = total_results,
                detail_level = detail_level.as_str(),
                "Tool search completed"
            );
        }

        Ok(results)
    }

    /// Get detailed information about a specific tool.
    pub async fn get_tool_detail(&self, tool_name: &str) -> Result<Option<ToolDiscoveryResult>> {
        let tools = self.mcp_client.list_mcp_tools().await?;

        for tool in tools {
            if tool.name.eq_ignore_ascii_case(tool_name) {
                return Ok(Some(ToolDiscoveryResult {
                    name: tool.name.clone(),
                    provider: tool.provider.clone(),
                    description: tool.description.clone(),
                    relevance_score: 1.0,
                    input_schema: Some(tool.input_schema),
                }));
            }
        }

        Ok(None)
    }

    /// List all available tools grouped by provider.
    pub async fn list_tools_by_provider(&self) -> Result<Vec<(String, Vec<ToolDiscoveryResult>)>> {
        let tools = self.mcp_client.list_mcp_tools().await?;

        Ok(group_results_by_provider_preserving_order(
            tools.into_iter().map(|tool| ToolDiscoveryResult {
                name: tool.name,
                provider: tool.provider,
                description: tool.description,
                relevance_score: 1.0,
                input_schema: None,
            }),
        ))
    }

    /// Calculate relevance score for a tool based on keyword match.
    ///
    /// Uses fuzzy matching on name and description to score relevance.
    fn calculate_relevance(&self, tool: &McpToolInfo, keyword: &str) -> f32 {
        let keyword_lower = keyword.to_lowercase();

        // Exact name match: highest score
        if tool.name.eq_ignore_ascii_case(keyword) {
            return 1.0;
        }

        // Name contains keyword: high score
        if tool.name.to_lowercase().contains(&keyword_lower) {
            return 0.8;
        }

        // Description contains keyword: medium-high score
        if tool.description.to_lowercase().contains(&keyword_lower) {
            return 0.6;
        }

        // Calculate fuzzy match score for partial matches
        let name_fuzzy = self.fuzzy_score(&tool.name.to_lowercase(), &keyword_lower);
        if name_fuzzy > 0.3 {
            return 0.5 * name_fuzzy;
        }

        let desc_fuzzy = self.fuzzy_score(&tool.description.to_lowercase(), &keyword_lower);
        if desc_fuzzy > 0.3 {
            return 0.3 * desc_fuzzy;
        }

        0.0
    }

    /// Simple fuzzy matching score (0.0 to 1.0).
    fn fuzzy_score(&self, haystack: &str, needle: &str) -> f32 {
        if needle.is_empty() {
            return 1.0;
        }

        if haystack.is_empty() {
            return 0.0;
        }

        let mut score = 0.0;
        let mut haystack_idx = 0;

        for needle_char in needle.chars() {
            if let Some(pos) = haystack[haystack_idx..].find(needle_char) {
                haystack_idx += pos + 1;
                score += 1.0;
            } else {
                return 0.0;
            }
        }

        // Normalize score by needle length
        score / needle.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mock_tool(provider: &str, name: &str, description: &str) -> McpToolInfo {
        McpToolInfo {
            name: name.to_string(),
            description: description.to_string(),
            provider: provider.to_string(),
            input_schema: json!({}),
        }
    }

    #[test]
    fn fuzzy_score_exact_match() {
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient::default()));
        assert_eq!(discovery.fuzzy_score("read_file", "read_file"), 1.0);
    }

    #[test]
    fn fuzzy_score_partial_match() {
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient::default()));
        let score = discovery.fuzzy_score("read_file_contents", "read");
        assert!(score > 0.5 && score <= 1.0);
    }

    #[test]
    fn fuzzy_score_no_match() {
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient::default()));
        assert_eq!(discovery.fuzzy_score("read_file", "xyz"), 0.0);
    }

    #[tokio::test]
    async fn list_tools_by_provider_preserves_first_seen_provider_and_tool_order() {
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient {
            tools: vec![
                mock_tool("gmail", "send_email", "Send an email."),
                mock_tool("calendar", "create_event", "Create a calendar event."),
                mock_tool("gmail", "read_email", "Read an email."),
                mock_tool("docs", "search", "Search docs."),
                mock_tool("calendar", "list_events", "List calendar events."),
            ],
        }));

        let grouped = discovery
            .list_tools_by_provider()
            .await
            .expect("grouped tools");

        let providers = grouped
            .iter()
            .map(|(provider, _)| provider.as_str())
            .collect::<Vec<_>>();
        assert_eq!(providers, vec!["gmail", "calendar", "docs"]);

        let tool_names = grouped
            .into_iter()
            .map(|(_, tools)| tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(
            tool_names,
            vec![
                vec!["send_email".to_string(), "read_email".to_string()],
                vec!["create_event".to_string(), "list_events".to_string()],
                vec!["search".to_string()],
            ]
        );
    }

    // Mock for testing
    #[derive(Default)]
    struct MockMcpClient {
        tools: Vec<McpToolInfo>,
    }

    #[async_trait::async_trait]
    impl crate::mcp::McpToolExecutor for MockMcpClient {
        async fn execute_mcp_tool(&self, _tool_name: &str, _args: &Value) -> Result<Value> {
            Ok(Value::Null)
        }

        async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
            Ok(self.tools.clone())
        }

        async fn has_mcp_tool(&self, _tool_name: &str) -> Result<bool> {
            Ok(false)
        }

        fn get_status(&self) -> crate::mcp::McpClientStatus {
            crate::mcp::McpClientStatus {
                enabled: true,
                provider_count: 0,
                active_connections: 0,
                configured_providers: vec![],
            }
        }
    }
}
