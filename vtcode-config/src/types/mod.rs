//! Common types and interfaces used throughout the application

use crate::core::PromptCachingConfig;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

// Re-export from vtcode-commons so downstream code can use `vtcode_config::types::ReasoningEffortLevel`.
pub use vtcode_commons::reasoning::ReasoningEffortLevel;

/// System prompt mode (inspired by pi-coding-agent philosophy)
/// Controls verbosity and complexity of system prompts sent to models
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SystemPromptMode {
    /// Minimal prompt (~500-800 tokens) - Pi-inspired, modern models need less guidance
    /// Best for: Power users, token-constrained contexts, fast responses
    Minimal,
    /// Lightweight prompt (~1-2k tokens) - Essential guidance only
    /// Best for: Resource-constrained operations, simple tasks
    Lightweight,
    /// Default prompt (~6-7k tokens) - Full guidance with all features
    /// Best for: General usage, comprehensive error handling
    #[default]
    Default,
    /// Specialized prompt (~7-8k tokens) - Complex refactoring and analysis
    /// Best for: Multi-file changes, sophisticated code analysis
    Specialized,
}

impl SystemPromptMode {
    /// Return the textual representation for configuration
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Lightweight => "lightweight",
            Self::Default => "default",
            Self::Specialized => "specialized",
        }
    }

    /// Parse system prompt mode from user configuration
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("minimal") {
            Some(Self::Minimal)
        } else if normalized.eq_ignore_ascii_case("lightweight") {
            Some(Self::Lightweight)
        } else if normalized.eq_ignore_ascii_case("default") {
            Some(Self::Default)
        } else if normalized.eq_ignore_ascii_case("specialized") {
            Some(Self::Specialized)
        } else {
            None
        }
    }

    /// Allowed configuration values for validation
    pub fn allowed_values() -> &'static [&'static str] {
        &["minimal", "lightweight", "default", "specialized"]
    }
}

impl fmt::Display for SystemPromptMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SystemPromptMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if let Some(parsed) = Self::parse(&raw) {
            Ok(parsed)
        } else {
            Ok(Self::default())
        }
    }
}

/// Tool documentation mode (inspired by pi-coding-agent progressive disclosure)
/// Controls how much tool documentation is loaded upfront vs on-demand
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ToolDocumentationMode {
    /// Minimal signatures only (~800 tokens total) - Pi-style, power users
    /// Best for: Maximum efficiency, experienced users, token-constrained contexts
    Minimal,
    /// Signatures + common parameters (~1,200 tokens total) - Smart hints
    /// Best for: General usage, balances overhead vs guidance (recommended)
    #[default]
    Progressive,
    /// Full documentation upfront (~3,000 tokens total) - Current behavior
    /// Best for: Maximum hand-holding, comprehensive parameter documentation
    Full,
}

impl ToolDocumentationMode {
    /// Return the textual representation for configuration
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Progressive => "progressive",
            Self::Full => "full",
        }
    }

    /// Parse tool documentation mode from user configuration
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("minimal") {
            Some(Self::Minimal)
        } else if normalized.eq_ignore_ascii_case("progressive") {
            Some(Self::Progressive)
        } else if normalized.eq_ignore_ascii_case("full") {
            Some(Self::Full)
        } else {
            None
        }
    }

    /// Allowed configuration values for validation
    pub fn allowed_values() -> &'static [&'static str] {
        &["minimal", "progressive", "full"]
    }
}

impl fmt::Display for ToolDocumentationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ToolDocumentationMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if let Some(parsed) = Self::parse(&raw) {
            Ok(parsed)
        } else {
            Ok(Self::default())
        }
    }
}

/// Verbosity level for model output (GPT-5.4-family and compatible models)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum VerbosityLevel {
    Low,
    #[default]
    Medium,
    High,
}

impl VerbosityLevel {
    /// Return the textual representation expected by downstream APIs
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Attempt to parse a verbosity level from user configuration input
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("low") {
            Some(Self::Low)
        } else if normalized.eq_ignore_ascii_case("medium") {
            Some(Self::Medium)
        } else if normalized.eq_ignore_ascii_case("high") {
            Some(Self::High)
        } else {
            None
        }
    }

    /// Enumerate the allowed configuration values
    pub fn allowed_values() -> &'static [&'static str] {
        &["low", "medium", "high"]
    }
}

impl fmt::Display for VerbosityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for VerbosityLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if let Some(parsed) = Self::parse(&raw) {
            Ok(parsed)
        } else {
            Ok(Self::default())
        }
    }
}

/// Preferred rendering surface for the interactive chat UI
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum UiSurfacePreference {
    #[default]
    Auto,
    Alternate,
    Inline,
}

impl UiSurfacePreference {
    /// String representation used in configuration and logging
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Alternate => "alternate",
            Self::Inline => "inline",
        }
    }

    /// Parse a surface preference from configuration input
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("auto") {
            Some(Self::Auto)
        } else if normalized.eq_ignore_ascii_case("alternate")
            || normalized.eq_ignore_ascii_case("alt")
        {
            Some(Self::Alternate)
        } else if normalized.eq_ignore_ascii_case("inline") {
            Some(Self::Inline)
        } else {
            None
        }
    }

    /// Enumerate the accepted configuration values for validation messaging
    pub fn allowed_values() -> &'static [&'static str] {
        &["auto", "alternate", "inline"]
    }
}

impl fmt::Display for UiSurfacePreference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for UiSurfacePreference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if let Some(parsed) = Self::parse(&raw) {
            Ok(parsed)
        } else {
            Ok(Self::default())
        }
    }
}

/// Source describing how the active model was selected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelSelectionSource {
    /// Model provided by workspace configuration
    #[default]
    WorkspaceConfig,
    /// Model provided by CLI override
    CliOverride,
}

/// Configuration for the agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: String,
    pub api_key: String,
    pub provider: String,
    pub openai_chatgpt_auth: Option<crate::auth::OpenAIChatGptAuthHandle>,
    pub api_key_env: String,
    pub workspace: PathBuf,
    pub verbose: bool,
    pub quiet: bool,
    pub theme: String,
    pub reasoning_effort: ReasoningEffortLevel,
    pub ui_surface: UiSurfacePreference,
    pub prompt_cache: PromptCachingConfig,
    pub model_source: ModelSelectionSource,
    pub custom_api_keys: BTreeMap<String, String>,
    pub checkpointing_enabled: bool,
    pub checkpointing_storage_dir: Option<PathBuf>,
    pub checkpointing_max_snapshots: usize,
    pub checkpointing_max_age_days: Option<u64>,
    pub max_conversation_turns: usize,
    pub model_behavior: Option<crate::core::ModelConfig>,
}

/// Workshop agent capability levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapabilityLevel {
    /// Basic chat only
    Basic,
    /// Can read files
    FileReading,
    /// Can read files and list directories
    FileListing,
    /// Can read files, list directories, and run bash commands
    Bash,
    /// Can read files, list directories, run bash commands, and edit files
    Editing,
    /// Full capabilities including code search
    CodeSearch,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub start_time: u64,
    pub total_turns: usize,
    pub total_decisions: usize,
    pub error_count: usize,
}

/// Error information for tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub error_type: String,
    pub message: String,
    pub turn_number: usize,
    pub recoverable: bool,
    pub timestamp: u64,
}

/// Performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub session_duration_seconds: u64,
    pub total_api_calls: usize,
    pub total_tokens_used: Option<usize>,
    pub average_response_time_ms: f64,
    pub tool_execution_count: usize,
    pub error_count: usize,
    pub recovery_success_rate: f64,
}

/// Analysis depth for workspace analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisDepth {
    Basic,
    Standard,
    Deep,
}

/// Output format for commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFormat {
    Text,
    Json,
    Html,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reasoning_effort_parse_and_allowed_values_include_max() {
        assert_eq!(
            ReasoningEffortLevel::parse("max"),
            Some(ReasoningEffortLevel::Max)
        );
        assert_eq!(ReasoningEffortLevel::Max.as_str(), "max");
        assert!(ReasoningEffortLevel::allowed_values().contains(&"max"));
    }
}
