use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

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
}

impl Default for ContextFeaturesConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: default_max_context_tokens(),
            trim_to_percent: default_trim_to_percent(),
            preserve_recent_turns: default_preserve_recent_turns(),
            ledger: LedgerConfig::default(),
        }
    }
}

impl ContextFeaturesConfig {
    pub fn validate(&self) -> Result<()> {
        self.ledger
            .validate()
            .context("Invalid ledger configuration")?;
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
fn default_max_context_tokens() -> usize {
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
