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

use crate::McpToolInfo;
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
    description: String,
    relevance_score: f32,
    /// Present only when detail_level is Full or NameAndDescription
    input_schema: Option<Value>,
    /// Present only when detail_level is Full and the server advertises it
    output_schema: Option<Value>,
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
            DetailLevel::Full => {
                let mut item = serde_json::json!({
                    "name": self.name,
                    "provider": self.provider,
                    "description": self.description,
                    "input_schema": self.input_schema,
                });
                if let Some(schema) = self.output_schema.as_ref()
                    && let Some(object) = item.as_object_mut()
                {
                    drop(object.insert("output_schema".to_string(), schema.clone()));
                }
                item
            }
        }
    }
}

/// Tool discovery service for progressive disclosure of MCP tools.
pub struct ToolDiscovery {
    mcp_client: Arc<dyn crate::McpToolExecutor>,
}

fn group_results_by_provider_preserving_order(
    tools: impl IntoIterator<Item = ToolDiscoveryResult>,
) -> Vec<(String, Vec<ToolDiscoveryResult>)> {
    let mut grouped: Vec<(String, Vec<ToolDiscoveryResult>)> = Vec::new();

    for tool in tools {
        let provider = tool.provider.clone();
        if let Some((_, provider_tools)) =
            grouped.iter_mut().find(|(existing_provider, _)| *existing_provider == provider)
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
    pub fn new(mcp_client: Arc<dyn crate::McpToolExecutor>) -> Self {
        Self { mcp_client }
    }

    /// Search for tools by keyword with configurable detail level.
    ///
    /// This implements progressive disclosure: agents can search with
    /// low detail to find relevant tools, then request full schemas
    /// only for tools they intend to use.
    ///
    /// Follows AGENTS.md guidelines: limits results to 5 items with overflow indication.
    pub async fn search_tools(&self, keyword: &str, detail_level: DetailLevel) -> Result<Vec<ToolDiscoveryResult>> {
        let tools = self.mcp_client.list_mcp_tools().await?;

        debug!(keyword = keyword, count = tools.len(), "Searching tools for keyword");

        // Score by reference first. Only the truncated survivors below pay for
        // owned clones of names/descriptions/schemas; cloning every match
        // up front would discard most of that work at the 5-result cap.
        let mut scored: Vec<(&McpToolInfo, f32)> = Vec::with_capacity(tools.len() / 4);
        for tool in &tools {
            let relevance_score = self.calculate_relevance(tool, keyword);

            // Filter out tools with no relevance
            if relevance_score > 0.0 {
                scored.push((tool, relevance_score));
            }
        }

        // Sort by relevance score (highest first). Stable sort preserves the
        // original discovery order among tied scores, matching the previous
        // clone-then-sort behavior exactly.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

        // Apply AGENTS.md compliance: limit to 5 results with overflow indication
        let total_results = scored.len();
        if total_results > 5 {
            info!(
                keyword = keyword,
                matched = total_results,
                displayed = 5,
                overflow = total_results - 5,
                detail_level = detail_level.as_str(),
                "Tool search completed with overflow"
            );
            scored.truncate(5);
        } else {
            info!(
                keyword = keyword,
                matched = total_results,
                detail_level = detail_level.as_str(),
                "Tool search completed"
            );
        }

        // Materialize owned results for the survivors only.
        let mut results = Vec::with_capacity(scored.len());
        for (tool, relevance_score) in scored {
            // Only clone schemas when needed (Full detail level)
            let (input_schema, output_schema) = match detail_level {
                DetailLevel::Full => (Some(tool.input_schema.clone()), tool.output_schema.clone()),
                _ => (None, None),
            };

            results.push(ToolDiscoveryResult {
                name: tool.name.clone(),
                provider: tool.provider.clone(),
                description: tool.description.clone(),
                relevance_score,
                input_schema,
                output_schema,
            });
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
                    output_schema: tool.output_schema,
                }));
            }
        }

        Ok(None)
    }

    /// List all available tools grouped by provider.
    async fn list_tools_by_provider(&self) -> Result<Vec<(String, Vec<ToolDiscoveryResult>)>> {
        let tools = self.mcp_client.list_mcp_tools().await?;

        Ok(group_results_by_provider_preserving_order(tools.into_iter().map(|tool| ToolDiscoveryResult {
            name: tool.name,
            provider: tool.provider,
            description: tool.description,
            relevance_score: 1.0,
            input_schema: None,
            output_schema: None,
        })))
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

        // Fuzzy fallback for partial description matches
        let desc_fuzzy = self.fuzzy_score(&tool.description.to_lowercase(), &keyword_lower);
        if desc_fuzzy > 0.2 {
            return 0.3 * desc_fuzzy;
        }

        0.0
    }

    /// Sørensen-Dice bigram similarity score (0.0 to 1.0).
    ///
    /// Uses the battle-tested [`strsim`](https://docs.rs/strsim) implementation.
    /// Handles partial and fuzzy matches more accurately than simple subsequence
    /// matching for keywords in tool names and descriptions.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Sørensen-Dice is normalized to the [0, 1] range, so the f32 score remains bounded."
    )]
    fn fuzzy_score(&self, haystack: &str, needle: &str) -> f32 {
        if needle.is_empty() {
            return 1.0;
        }
        if haystack.is_empty() {
            return 0.0;
        }
        strsim::sorensen_dice(haystack, needle) as f32
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
            output_schema: None,
        }
    }

    #[test]
    fn fuzzy_score_exact_match() {
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient::default()));
        assert!((discovery.fuzzy_score("read_file", "read_file") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fuzzy_score_partial_match() {
        // Sørensen-Dice for "read_file" vs "read": 3 shared bigrams / 11 total = 0.55
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient::default()));
        let score = discovery.fuzzy_score("read_file", "read");
        assert!(score > 0.5 && score <= 1.0, "expected >0.5, got {score}");
    }

    #[test]
    fn fuzzy_score_no_match() {
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient::default()));
        assert!(discovery.fuzzy_score("read_file", "xyz").abs() < f32::EPSILON);
    }

    #[test]
    fn full_detail_json_includes_output_schema_only_when_advertised() {
        let with_schema = ToolDiscoveryResult {
            name: "ask".to_string(),
            provider: "deepwiki".to_string(),
            description: "Ask.".to_string(),
            relevance_score: 1.0,
            input_schema: Some(json!({"type": "object"})),
            output_schema: Some(json!({"type": "object"})),
        };
        assert_eq!(with_schema.to_json(DetailLevel::Full)["output_schema"], json!({"type": "object"}));

        let without_schema = ToolDiscoveryResult { output_schema: None, ..with_schema.clone() };
        let full = without_schema.to_json(DetailLevel::Full);
        assert!(full.get("output_schema").is_none(), "absent schema must stay absent");
        assert!(
            with_schema
                .to_json(DetailLevel::NameAndDescription)
                .get("output_schema")
                .is_none(),
            "compact levels must not carry schemas"
        );
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

        let grouped = discovery.list_tools_by_provider().await.expect("grouped tools");

        let providers = grouped.iter().map(|(provider, _)| provider.as_str()).collect::<Vec<_>>();
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

    #[tokio::test]
    async fn search_tools_keeps_highest_scores_despite_late_position_and_ties() {
        // Arrange: 8 tools scored through deterministic tiers only
        // (exact name = 1.0, name-contains = 0.8, description-contains = 0.6).
        // The best matches sit last, so keeping the first 5 would fail; the
        // 0.8 three-way tie checks stable discovery order; the lone 0.6 must
        // be truncated away. Fillers (`calendar`, `docs`) share at most one
        // bigram with "mail" (Dice <= 0.2), staying below every tier above.
        let discovery = ToolDiscovery::new(Arc::new(MockMcpClient {
            tools: vec![
                mock_tool("prov", "calendar", "Show the calendar."),
                mock_tool("prov", "docs", "Search the docs."),
                mock_tool("prov", "forward_mail", "Forward a message."),
                mock_tool("prov", "send_mail", "Send a message."),
                mock_tool("prov", "mail", "Mail things."),
                mock_tool("prov", "read_mail", "Read a message."),
                mock_tool("prov", "delete_mail", "Delete a message."),
                mock_tool("prov", "archive", "Archive old mail threads."),
            ],
        }));

        // Act.
        let results = discovery.search_tools("mail", DetailLevel::Full).await.expect("search tools");

        // Assert: truncation survivors in score order, ties in discovery order.
        let names = results.iter().map(|result| result.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, vec!["mail", "forward_mail", "send_mail", "read_mail", "delete_mail"]);
        let scores = results.iter().map(|result| result.relevance_score).collect::<Vec<_>>();
        assert_eq!(scores, vec![1.0, 0.8, 0.8, 0.8, 0.8]);
        assert!(results.iter().all(|result| result.input_schema.is_some()));

        // Compact levels keep the same survivors without cloning schemas.
        let compact = discovery
            .search_tools("mail", DetailLevel::NameAndDescription)
            .await
            .expect("compact search");
        let compact_names = compact.iter().map(|result| result.name.as_str()).collect::<Vec<_>>();
        assert_eq!(compact_names, names);
        assert!(
            compact
                .iter()
                .all(|result| result.input_schema.is_none() && result.output_schema.is_none())
        );
    }

    #[async_trait::async_trait]
    impl crate::McpToolExecutor for MockMcpClient {
        async fn execute_mcp_tool(&self, _tool_name: &str, _args: &Value) -> Result<Value> {
            Ok(Value::Null)
        }

        async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
            Ok(self.tools.clone())
        }

        async fn has_mcp_tool(&self, _tool_name: &str) -> Result<bool> {
            Ok(false)
        }

        fn get_status(&self) -> crate::McpClientStatus {
            crate::McpClientStatus {
                enabled: true,
                provider_count: 0,
                active_connections: 0,
                configured_providers: vec![],
            }
        }
    }
}
