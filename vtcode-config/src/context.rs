use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Configuration for dynamic context discovery
///
/// This implements Cursor-style dynamic context discovery patterns where
/// large outputs are written to files instead of being truncated, allowing
/// agents to retrieve them on demand via read_file/grep_file.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicContextConfig {
    /// Enable dynamic context discovery features
    #[serde(default = "default_dynamic_enabled")]
    pub enabled: bool,

    /// Threshold in bytes above which tool outputs are spooled to files
    #[serde(default = "default_tool_output_threshold")]
    pub tool_output_threshold: usize,

    /// Enable syncing terminal sessions to .vtcode/terminals/ files
    #[serde(default = "default_sync_terminals")]
    pub sync_terminals: bool,

    /// Enable persisting conversation history during summarization
    #[serde(default = "default_persist_history")]
    pub persist_history: bool,

    /// Enable syncing MCP tool descriptions to .vtcode/mcp/tools/
    #[serde(default = "default_sync_mcp_tools")]
    pub sync_mcp_tools: bool,

    /// Enable generating skill index in .agents/skills/INDEX.md
    #[serde(default = "default_sync_skills")]
    pub sync_skills: bool,

    /// Maximum age in seconds for spooled tool output files before cleanup
    #[serde(default = "default_spool_max_age_secs")]
    pub spool_max_age_secs: u64,

    /// Maximum number of spooled files to keep
    #[serde(default = "default_max_spooled_files")]
    pub max_spooled_files: usize,
}

impl Default for DynamicContextConfig {
    fn default() -> Self {
        Self {
            enabled: default_dynamic_enabled(),
            tool_output_threshold: default_tool_output_threshold(),
            sync_terminals: default_sync_terminals(),
            persist_history: default_persist_history(),
            sync_mcp_tools: default_sync_mcp_tools(),
            sync_skills: default_sync_skills(),
            spool_max_age_secs: default_spool_max_age_secs(),
            max_spooled_files: default_max_spooled_files(),
        }
    }
}

impl DynamicContextConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.tool_output_threshold >= 1024,
            "Tool output threshold must be at least 1024 bytes"
        );
        ensure!(
            self.max_spooled_files > 0,
            "Max spooled files must be greater than zero"
        );
        Ok(())
    }
}

fn default_dynamic_enabled() -> bool {
    true
}

fn default_tool_output_threshold() -> usize {
    8192 // 8KB
}

fn default_sync_terminals() -> bool {
    true
}

fn default_persist_history() -> bool {
    true
}

fn default_sync_mcp_tools() -> bool {
    true
}

fn default_sync_skills() -> bool {
    true
}

fn default_spool_max_age_secs() -> u64 {
    3600 // 1 hour
}

fn default_max_spooled_files() -> usize {
    100
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LedgerConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    /// Inject ledger into the system prompt each turn
    #[serde(default = "default_include_in_prompt")]
    pub include_in_prompt: bool,
    /// Preserve ledger entries during context compression
    #[serde(default = "default_preserve_in_compression")]
    pub preserve_in_compression: bool,
}

impl Default for LedgerConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_entries: default_max_entries(),
            include_in_prompt: default_include_in_prompt(),
            preserve_in_compression: default_preserve_in_compression(),
        }
    }
}

impl LedgerConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.max_entries > 0,
            "Ledger max_entries must be greater than zero"
        );
        Ok(())
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextFeaturesConfig {
    /// Maximum tokens to keep in context (affects model cost and performance)
    /// Higher values preserve more context but cost more and may hit token limits
    /// This field is maintained for compatibility but no longer used for trimming
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,

    /// Percentage to trim context to when it gets too large
    /// This field is maintained for compatibility but no longer used for trimming
    #[serde(default = "default_trim_to_percent")]
    pub trim_to_percent: u8,

    /// Preserve recent turns during context management
    /// This field is maintained for compatibility but no longer used for trimming
    #[serde(default = "default_preserve_recent_turns")]
    pub preserve_recent_turns: usize,

    #[serde(default)]
    pub ledger: LedgerConfig,

    /// Dynamic context discovery settings (Cursor-style)
    #[serde(default)]
    pub dynamic: DynamicContextConfig,
}

impl Default for ContextFeaturesConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: default_max_context_tokens(),
            trim_to_percent: default_trim_to_percent(),
            preserve_recent_turns: default_preserve_recent_turns(),
            ledger: LedgerConfig::default(),
            dynamic: DynamicContextConfig::default(),
        }
    }
}

impl ContextFeaturesConfig {
    pub fn validate(&self) -> Result<()> {
        self.ledger
            .validate()
            .context("Invalid ledger configuration")?;
        self.dynamic
            .validate()
            .context("Invalid dynamic context configuration")?;
        Ok(())
    }
}

fn default_enabled() -> bool {
    true
}
fn default_max_entries() -> usize {
    12
}
fn default_include_in_prompt() -> bool {
    true
}
pub fn default_max_context_tokens() -> usize {
    90000
}

fn default_trim_to_percent() -> u8 {
    60
}

fn default_preserve_recent_turns() -> usize {
    10
}

fn default_preserve_in_compression() -> bool {
    true
}
