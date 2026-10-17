use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::ToolConstraints;

/// Alternative tool policy configuration format (user's format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeToolPolicyConfig {
    /// Configuration version for future compatibility
    pub version: u32,
    /// Default policy settings
    pub default: AlternativeDefaultPolicy,
    /// Tool-specific policies
    pub tools: IndexMap<String, AlternativeToolPolicy>,
    /// Optional per-tool constraints (ignored if absent)
    #[serde(default)]
    pub constraints: IndexMap<String, ToolConstraints>,
}

/// Default policy in alternative format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeDefaultPolicy {
    /// Whether to allow by default
    pub allow: bool,
    /// Rate limit per run
    pub rate_limit_per_run: u32,
    /// Max concurrent executions
    pub max_concurrent: u32,
    /// Allow filesystem writes
    pub fs_write: bool,
    /// Allow network access
    pub network: bool,
}

/// Tool policy in alternative format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeToolPolicy {
    /// Whether to allow this tool
    pub allow: bool,
    /// Allow filesystem writes (optional)
    #[serde(default)]
    pub fs_write: bool,
    /// Allow network access (optional)
    #[serde(default)]
    pub network: bool,
    /// Arguments policy (optional)
    #[serde(default)]
    pub args_policy: Option<AlternativeArgsPolicy>,
}

/// Arguments policy in alternative format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeArgsPolicy {
    /// Substrings to deny
    pub deny_substrings: Vec<String>,
}
