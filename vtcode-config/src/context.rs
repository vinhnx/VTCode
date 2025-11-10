use crate::constants::context as context_defaults;
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

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenBudgetConfig {
    /// Enable token budget tracking
    #[serde(default = "default_token_budget_enabled")]
    pub enabled: bool,
    /// Model name for tokenizer selection
    #[serde(default = "default_token_budget_model")]
    pub model: String,
    /// Optional override for tokenizer identifier or file path
    #[serde(default)]
    pub tokenizer: Option<String>,
    /// Warning threshold (0.0-1.0)
    #[serde(default = "default_warning_threshold")]
    pub warning_threshold: f64,
    /// Alert threshold (0.0-1.0)
    #[serde(default = "default_alert_threshold")]
    pub alert_threshold: f64,
    /// Enable detailed component tracking
    #[serde(default = "default_detailed_tracking")]
    pub detailed_tracking: bool,
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {
            enabled: default_token_budget_enabled(),
            model: default_token_budget_model(),
            tokenizer: None,
            warning_threshold: default_warning_threshold(),
            alert_threshold: default_alert_threshold(),
            detailed_tracking: default_detailed_tracking(),
        }
    }
}

impl TokenBudgetConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (0.0..=1.0).contains(&self.warning_threshold),
            "Token budget warning_threshold must be between 0.0 and 1.0"
        );
        ensure!(
            (0.0..=1.0).contains(&self.alert_threshold),
            "Token budget alert_threshold must be between 0.0 and 1.0"
        );
        ensure!(
            self.warning_threshold <= self.alert_threshold,
            "Token budget warning_threshold must be less than or equal to alert_threshold"
        );

        if self.enabled {
            ensure!(
                !self.model.trim().is_empty(),
                "Token budget model must be provided when token budgeting is enabled"
            );
            if let Some(tokenizer) = &self.tokenizer {
                ensure!(
                    !tokenizer.trim().is_empty(),
                    "Token budget tokenizer override cannot be empty"
                );
            }
        }

        Ok(())
    }
}

fn default_token_budget_enabled() -> bool {
    true
}
fn default_token_budget_model() -> String {
    "gpt-5-nano".to_string()
}
fn default_warning_threshold() -> f64 {
    0.75
}
fn default_alert_threshold() -> f64 {
    0.85
}
fn default_detailed_tracking() -> bool {
    false
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextFeaturesConfig {
    #[serde(default)]
    pub ledger: LedgerConfig,
    #[serde(default)]
    pub token_budget: TokenBudgetConfig,
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
            max_context_tokens: default_max_context_tokens(),
            trim_to_percent: default_trim_to_percent(),
            preserve_recent_turns: default_preserve_recent_turns(),
        }
    }
}

impl ContextFeaturesConfig {
    pub fn validate(&self) -> Result<()> {
        self.ledger
            .validate()
            .context("Invalid ledger configuration")?;
        self.token_budget
            .validate()
            .context("Invalid token_budget configuration")?;

        ensure!(
            self.max_context_tokens > 0,
            "Context features max_context_tokens must be greater than zero"
        );
        ensure!(
            (1..=100).contains(&self.trim_to_percent),
            "Context features trim_to_percent must be between 1 and 100"
        );
        ensure!(
            self.preserve_recent_turns > 0,
            "Context features preserve_recent_turns must be greater than zero"
        );

        Ok(())
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
