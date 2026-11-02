use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Input accepted by the OpenAI-compatible `search` tool.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchInput {
    /// Case-insensitive substring to find in visible workspace paths or content.
    pub query: String,
}

/// Input accepted by the OpenAI-compatible `fetch` tool.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct FetchInput {
    /// Workspace-relative file ID returned by `search`.
    pub id: String,
}

/// A search result compatible with the OpenAI remote MCP contract.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema, PartialEq, Eq)]
pub struct SearchResult {
    /// Stable workspace-relative file ID.
    pub id: String,
    /// Human-readable result title.
    pub title: String,
    /// Citation URL, or an empty string when no prefix is configured.
    pub url: String,
}

/// The structured result returned by `search`.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema, PartialEq, Eq)]
pub struct SearchOutput {
    /// Matching visible workspace files.
    pub results: Vec<SearchResult>,
}

/// The structured result returned by `fetch`.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema, PartialEq)]
pub struct FetchOutput {
    /// Workspace-relative file ID.
    pub id: String,
    /// Human-readable document title.
    pub title: String,
    /// UTF-8 file contents.
    pub text: String,
    /// Citation URL, or an empty string when no prefix is configured.
    pub url: String,
    /// Optional provider-defined metadata. VT Code currently has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
}
