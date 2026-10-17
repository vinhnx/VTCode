use anyhow::Result;
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use super::{McpPolicyStore, ToolPolicy};

/// Decision result for tool execution
#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecutionDecision {
    /// The tool is allowed to execute.
    Allowed,
    /// The tool execution is denied.
    Denied,
    /// The tool execution is denied with a feedback message.
    DeniedWithFeedback(String),
}

impl ToolExecutionDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Tool policy configuration stored in the canonical user config directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicyConfig {
    /// Configuration version for future compatibility
    pub version: u32,
    /// Available tools at time of last update
    pub available_tools: Vec<String>,
    /// Policy for each tool
    pub policies: IndexMap<String, ToolPolicy>,
    /// Optional per-tool constraints to scope permissions and enforce safety
    #[serde(default)]
    pub constraints: IndexMap<String, ToolConstraints>,
    /// MCP-specific policy configuration
    #[serde(default)]
    pub mcp: McpPolicyStore,
    /// Explicit remembered approvals for future prompts in this workspace
    #[serde(default)]
    pub approval_cache: ApprovalCacheConfig,
}

impl Default for ToolPolicyConfig {
    fn default() -> Self {
        Self {
            version: 1,
            available_tools: Vec::new(),
            policies: IndexMap::new(),
            constraints: IndexMap::new(),
            mcp: McpPolicyStore::default(),
            approval_cache: ApprovalCacheConfig::default(),
        }
    }
}

/// Persisted approval cache stored alongside tool policies
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalCacheConfig {
    /// Stable approval keys that should bypass future prompts
    #[serde(default)]
    pub allowed: IndexSet<String>,
    /// Shell command prefixes that should bypass future prompts in the same scope
    #[serde(default)]
    pub prefixes: IndexSet<String>,
    /// Regex patterns matched against approval keys for advanced manual policy tuning
    #[serde(default)]
    pub regexes: IndexSet<String>,
}

/// Maximum remembered approval entries per cache collection. Unbounded growth
/// turned `tool-policy.json` into a multi-hundred-kilobyte file after a few
/// long sessions (see session-vtcode-20260925T234343Z: 294 KB). Oldest
/// insertions are dropped first so recent approvals survive.
pub const APPROVAL_CACHE_MAX_ENTRIES: usize = 256;

impl ApprovalCacheConfig {
    /// Drop the oldest entries so each collection stays within
    /// [`APPROVAL_CACHE_MAX_ENTRIES`]. Returns `true` when anything was trimmed.
    pub fn trim_to_budget(&mut self) -> bool {
        let mut trimmed = false;
        for collection in [&mut self.allowed, &mut self.prefixes, &mut self.regexes] {
            while collection.len() > APPROVAL_CACHE_MAX_ENTRIES {
                let _ = collection.shift_remove_index(0);
                trimmed = true;
            }
        }
        trimmed
    }
}

/// Handler for tool permission prompts
///
/// This trait allows different UI modes (CLI, TUI) to provide their own
/// implementation for prompting users about tool execution.
pub trait PermissionPromptHandler: Send + Sync {
    /// Prompt the user for tool execution permission
    fn prompt_tool_permission(&mut self, tool_name: &str) -> Result<ToolExecutionDecision>;
}

/// Scoped, optional constraints for a tool to align with safe defaults
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolConstraints {
    /// Whitelisted modes for tools that support modes (e.g., 'terminal')
    #[serde(default)]
    pub allowed_modes: Option<Vec<String>>,
    /// Cap on results for list/search-like tools
    #[serde(default)]
    pub max_results_per_call: Option<usize>,
    /// Cap on items scanned for file listing
    #[serde(default)]
    pub max_items_per_call: Option<usize>,
    /// Default response format if unspecified by caller
    #[serde(default)]
    pub default_response_format: Option<String>,
    /// Cap maximum bytes when reading files
    #[serde(default)]
    pub max_bytes_per_read: Option<usize>,
    /// Cap maximum bytes when fetching over the network
    #[serde(default)]
    pub max_response_bytes: Option<usize>,
    /// Allowed URL schemes for network tools
    #[serde(default)]
    pub allowed_url_schemes: Option<Vec<String>>,
    /// Denied URL hosts or suffixes for network tools
    #[serde(default)]
    pub denied_url_hosts: Option<Vec<String>>,
}
