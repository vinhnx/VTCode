use crate::config::constants::context as context_defaults;
use serde::{Deserialize, Serialize};

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

fn default_enabled() -> bool {
    true
}
fn default_max_entries() -> usize {
    12
}
fn default_include_in_prompt() -> bool {
    true
}
fn default_preserve_in_compression() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenBudgetConfig {
    /// Enable token budget tracking
    #[serde(default = "default_token_budget_enabled")]
    pub enabled: bool,
    /// Model name for tokenizer selection
    #[serde(default = "default_token_budget_model")]
    pub model: String,
    /// Warning threshold (0.0-1.0)
    #[serde(default = "default_warning_threshold")]
    pub warning_threshold: f64,
    /// Compaction threshold (0.0-1.0)
    #[serde(default = "default_compaction_threshold")]
    pub compaction_threshold: f64,
    /// Enable detailed component tracking
    #[serde(default = "default_detailed_tracking")]
    pub detailed_tracking: bool,
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {
            enabled: default_token_budget_enabled(),
            model: default_token_budget_model(),
            warning_threshold: default_warning_threshold(),
            compaction_threshold: default_compaction_threshold(),
            detailed_tracking: default_detailed_tracking(),
        }
    }
}

fn default_token_budget_enabled() -> bool {
    true
}
fn default_token_budget_model() -> String {
    "gpt-4o-mini".to_string()
}
fn default_warning_threshold() -> f64 {
    0.75
}
fn default_compaction_threshold() -> f64 {
    0.85
}
fn default_detailed_tracking() -> bool {
    false
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextCurationConfig {
    /// Enable dynamic context curation
    #[serde(default = "default_curation_enabled")]
    pub enabled: bool,
    /// Maximum tokens per turn
    #[serde(default = "default_max_tokens_per_turn")]
    pub max_tokens_per_turn: usize,
    /// Number of recent messages to always include
    #[serde(default = "default_preserve_recent_messages")]
    pub preserve_recent_messages: usize,
    /// Maximum tool descriptions to include
    #[serde(default = "default_max_tool_descriptions")]
    pub max_tool_descriptions: usize,
    /// Include decision ledger summary
    #[serde(default = "default_include_ledger")]
    pub include_ledger: bool,
    /// Maximum ledger entries
    #[serde(default = "default_ledger_max_entries")]
    pub ledger_max_entries: usize,
    /// Include recent errors
    #[serde(default = "default_include_recent_errors")]
    pub include_recent_errors: bool,
    /// Maximum recent errors to include
    #[serde(default = "default_max_recent_errors")]
    pub max_recent_errors: usize,
}

impl Default for ContextCurationConfig {
    fn default() -> Self {
        Self {
            enabled: default_curation_enabled(),
            max_tokens_per_turn: default_max_tokens_per_turn(),
            preserve_recent_messages: default_preserve_recent_messages(),
            max_tool_descriptions: default_max_tool_descriptions(),
            include_ledger: default_include_ledger(),
            ledger_max_entries: default_ledger_max_entries(),
            include_recent_errors: default_include_recent_errors(),
            max_recent_errors: default_max_recent_errors(),
        }
    }
}

fn default_curation_enabled() -> bool {
    true
}
fn default_max_tokens_per_turn() -> usize {
    100_000
}
fn default_preserve_recent_messages() -> usize {
    5
}
fn default_max_tool_descriptions() -> usize {
    10
}
fn default_include_ledger() -> bool {
    true
}
fn default_ledger_max_entries() -> usize {
    12
}
fn default_include_recent_errors() -> bool {
    true
}
fn default_max_recent_errors() -> usize {
    3
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextFeaturesConfig {
    #[serde(default)]
    pub ledger: LedgerConfig,
    #[serde(default)]
    pub token_budget: TokenBudgetConfig,
    #[serde(default)]
    pub curation: ContextCurationConfig,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    #[serde(default = "default_trim_to_percent")]
    pub trim_to_percent: u8,
    #[serde(default = "default_preserve_recent_turns")]
    pub preserve_recent_turns: usize,
}

impl Default for ContextFeaturesConfig {
    fn default() -> Self {
        Self {
            ledger: LedgerConfig::default(),
            token_budget: TokenBudgetConfig::default(),
            curation: ContextCurationConfig::default(),
            max_context_tokens: default_max_context_tokens(),
            trim_to_percent: default_trim_to_percent(),
            preserve_recent_turns: default_preserve_recent_turns(),
        }
    }
}

fn default_max_context_tokens() -> usize {
    context_defaults::DEFAULT_MAX_TOKENS
}

fn default_trim_to_percent() -> u8 {
    context_defaults::DEFAULT_TRIM_TO_PERCENT
}

fn default_preserve_recent_turns() -> usize {
    context_defaults::DEFAULT_PRESERVE_RECENT_TURNS
}
