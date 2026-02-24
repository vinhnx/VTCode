use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::constants::{defaults, tools};
use crate::core::plugins::PluginRuntimeConfig;

/// Tools configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolsConfig {
    /// Default policy for tools not explicitly listed
    #[serde(default = "default_tool_policy")]
    pub default_policy: ToolPolicy,

    /// Specific tool policies
    #[serde(default)]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeMap<String, ToolPolicy>")
    )]
    pub policies: IndexMap<String, ToolPolicy>,

    /// Maximum inner tool-call loops per user turn
    ///
    /// Prevents infinite tool-calling cycles in interactive chat. This limits how
    /// many back-and-forths the agent will perform executing tools and
    /// re-asking the model before returning a final answer.
    ///
    #[serde(default = "default_max_tool_loops")]
    pub max_tool_loops: usize,

    /// Maximum number of times the same tool invocation can be retried with the
    /// identical arguments within a single turn.
    #[serde(default = "default_max_repeated_tool_calls")]
    pub max_repeated_tool_calls: usize,

    /// Maximum consecutive blocked tool calls allowed per turn before forcing a
    /// turn break. This prevents long blocked-call churn from consuming CPU.
    #[serde(default = "default_max_consecutive_blocked_tool_calls_per_turn")]
    pub max_consecutive_blocked_tool_calls_per_turn: usize,

    /// Optional per-second rate limit for tool calls to smooth bursty retries.
    /// When unset, the runtime defaults apply.
    #[serde(default = "default_max_tool_rate_per_second")]
    pub max_tool_rate_per_second: Option<usize>,

    /// Maximum sequential spool-chunk `read_file` calls allowed per turn before
    /// nudging the agent to switch to targeted extraction/summarization.
    #[serde(default = "default_max_sequential_spool_chunk_reads")]
    pub max_sequential_spool_chunk_reads: usize,

    /// Web Fetch tool security configuration
    #[serde(default)]
    pub web_fetch: WebFetchConfig,

    /// Dynamic plugin runtime configuration
    #[serde(default)]
    pub plugins: PluginRuntimeConfig,

    /// Tool-specific loop thresholds (Adaptive Loop Detection)
    /// Allows setting higher loop limits for read-only tools (e.g., ls, grep)
    /// and lower limits for mutating tools.
    #[serde(default)]
    pub loop_thresholds: IndexMap<String, usize>,
}

/// Web Fetch tool security configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebFetchConfig {
    /// Security mode: "restricted" (blocklist) or "whitelist" (allowlist)
    #[serde(default = "default_web_fetch_mode")]
    pub mode: String,

    /// Enable dynamic blocklist loading from external file
    #[serde(default)]
    pub dynamic_blocklist_enabled: bool,

    /// Path to dynamic blocklist file
    #[serde(default)]
    pub dynamic_blocklist_path: String,

    /// Enable dynamic whitelist loading from external file
    #[serde(default)]
    pub dynamic_whitelist_enabled: bool,

    /// Path to dynamic whitelist file
    #[serde(default)]
    pub dynamic_whitelist_path: String,

    /// Inline blocklist - Additional domains to block
    #[serde(default)]
    pub blocked_domains: Vec<String>,

    /// Inline whitelist - Domains to allow in restricted mode
    #[serde(default)]
    pub allowed_domains: Vec<String>,

    /// Additional blocked patterns
    #[serde(default)]
    pub blocked_patterns: Vec<String>,

    /// Enable audit logging of URL validation decisions
    #[serde(default)]
    pub enable_audit_logging: bool,

    /// Path to audit log file
    #[serde(default)]
    pub audit_log_path: String,

    /// Strict HTTPS-only mode
    #[serde(default = "default_strict_https")]
    pub strict_https_only: bool,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        let policies = DEFAULT_TOOL_POLICIES
            .iter()
            .map(|(tool, policy)| ((*tool).into(), *policy))
            .collect::<IndexMap<_, _>>();
        Self {
            default_policy: default_tool_policy(),
            policies,
            max_tool_loops: default_max_tool_loops(),
            max_repeated_tool_calls: default_max_repeated_tool_calls(),
            max_consecutive_blocked_tool_calls_per_turn:
                default_max_consecutive_blocked_tool_calls_per_turn(),
            max_tool_rate_per_second: default_max_tool_rate_per_second(),
            max_sequential_spool_chunk_reads: default_max_sequential_spool_chunk_reads(),
            web_fetch: WebFetchConfig::default(),
            plugins: PluginRuntimeConfig::default(),
            loop_thresholds: IndexMap::new(),
        }
    }
}

const DEFAULT_BLOCKLIST_PATH: &str = "~/.vtcode/web_fetch_blocklist.json";
const DEFAULT_WHITELIST_PATH: &str = "~/.vtcode/web_fetch_whitelist.json";
const DEFAULT_AUDIT_LOG_PATH: &str = "~/.vtcode/web_fetch_audit.log";

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            mode: default_web_fetch_mode(),
            dynamic_blocklist_enabled: false,
            dynamic_blocklist_path: DEFAULT_BLOCKLIST_PATH.into(),
            dynamic_whitelist_enabled: false,
            dynamic_whitelist_path: DEFAULT_WHITELIST_PATH.into(),
            blocked_domains: Vec::new(),
            allowed_domains: Vec::new(),
            blocked_patterns: Vec::new(),
            enable_audit_logging: false,
            audit_log_path: DEFAULT_AUDIT_LOG_PATH.into(),
            strict_https_only: true,
        }
    }
}

/// Tool execution policy
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    /// Allow execution without confirmation
    Allow,
    /// Prompt user for confirmation
    Prompt,
    /// Deny execution
    Deny,
}

#[inline]
const fn default_tool_policy() -> ToolPolicy {
    ToolPolicy::Prompt
}

#[inline]
const fn default_max_tool_loops() -> usize {
    defaults::DEFAULT_MAX_TOOL_LOOPS
}

#[inline]
const fn default_max_repeated_tool_calls() -> usize {
    defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS
}

#[inline]
const fn default_max_consecutive_blocked_tool_calls_per_turn() -> usize {
    defaults::DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN
}

#[inline]
const fn default_max_tool_rate_per_second() -> Option<usize> {
    None
}

#[inline]
const fn default_max_sequential_spool_chunk_reads() -> usize {
    defaults::DEFAULT_MAX_SEQUENTIAL_SPOOL_CHUNK_READS_PER_TURN
}

#[inline]
fn default_web_fetch_mode() -> String {
    "restricted".into()
}

fn default_strict_https() -> bool {
    true
}

const DEFAULT_TOOL_POLICIES: &[(&str, ToolPolicy)] = &[
    // File operations (non-destructive)
    (tools::LIST_FILES, ToolPolicy::Allow),
    (tools::GREP_FILE, ToolPolicy::Allow),
    (tools::READ_FILE, ToolPolicy::Allow),
    // File operations (write/create)
    (tools::WRITE_FILE, ToolPolicy::Allow),
    (tools::EDIT_FILE, ToolPolicy::Allow),
    (tools::CREATE_FILE, ToolPolicy::Allow),
    // File operations (destructive - require confirmation)
    (tools::DELETE_FILE, ToolPolicy::Prompt),
    (tools::APPLY_PATCH, ToolPolicy::Prompt),
    (tools::SEARCH_REPLACE, ToolPolicy::Prompt),
    // PTY/Terminal operations
    (tools::RUN_PTY_CMD, ToolPolicy::Prompt),
    (tools::CREATE_PTY_SESSION, ToolPolicy::Allow),
    (tools::READ_PTY_SESSION, ToolPolicy::Allow),
    (tools::LIST_PTY_SESSIONS, ToolPolicy::Allow),
    (tools::RESIZE_PTY_SESSION, ToolPolicy::Allow),
    (tools::SEND_PTY_INPUT, ToolPolicy::Prompt),
    (tools::CLOSE_PTY_SESSION, ToolPolicy::Allow),
    // Code execution (requires confirmation)
    (tools::EXECUTE_CODE, ToolPolicy::Prompt),
    // Planning and meta tools
    (tools::SEARCH_TOOLS, ToolPolicy::Allow),
    (tools::SKILL, ToolPolicy::Allow),
    // Diagnostic and introspection tools
    // Web operations (requires confirmation)
    (tools::WEB_FETCH, ToolPolicy::Prompt),
];
