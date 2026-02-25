//! Common types and interfaces used throughout the application

use crate::constants::reasoning;
use crate::core::PromptCachingConfig;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;

/// Supported reasoning effort levels configured via vtcode.toml
/// These map to different provider-specific parameters:
/// - For Gemini 3 Pro: Maps to thinking_level (low, high) - medium coming soon
/// - For other models: Maps to provider-specific reasoning parameters
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ReasoningEffortLevel {
    /// No reasoning configuration - for models that don't support configurable reasoning
    None,
    /// Minimal reasoning effort - maps to low thinking level for Gemini 3 Pro
    Minimal,
    /// Low reasoning effort - maps to low thinking level for Gemini 3 Pro
    Low,
    /// Medium reasoning effort - Note: Not fully available for Gemini 3 Pro yet, defaults to high
    #[default]
    Medium,
    /// High reasoning effort - maps to high thinking level for Gemini 3 Pro
    High,
    /// Extra high reasoning effort - for gpt-5.2-codex+ long-running tasks
    XHigh,
}

impl ReasoningEffortLevel {
    /// Return the textual representation expected by downstream APIs
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => reasoning::LOW,
            Self::Medium => reasoning::MEDIUM,
            Self::High => reasoning::HIGH,
            Self::XHigh => "xhigh",
        }
    }

    /// Attempt to parse an effort level from user configuration input
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("none") {
            Some(Self::None)
        } else if normalized.eq_ignore_ascii_case("minimal") {
            Some(Self::Minimal)
        } else if normalized.eq_ignore_ascii_case(reasoning::LOW) {
            Some(Self::Low)
        } else if normalized.eq_ignore_ascii_case(reasoning::MEDIUM) {
            Some(Self::Medium)
        } else if normalized.eq_ignore_ascii_case(reasoning::HIGH) {
            Some(Self::High)
        } else if normalized.eq_ignore_ascii_case("xhigh") {
            Some(Self::XHigh)
        } else {
            None
        }
    }

    /// Enumerate the allowed configuration values for validation and messaging
    pub fn allowed_values() -> &'static [&'static str] {
        reasoning::ALLOWED_LEVELS
    }
}

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
    Progressive,
    /// Full documentation upfront (~3,000 tokens total) - Current behavior
    /// Best for: Maximum hand-holding, comprehensive parameter documentation
    #[default]
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

/// Verbosity level for model output (GPT-5.1 and compatible models)
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

impl fmt::Display for ReasoningEffortLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReasoningEffortLevel {
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

/// Default editing mode for agent startup (Codex-inspired workflow)
///
/// Controls the initial mode when a session starts. This is a **configuration**
/// enum for `default_editing_mode` in vtcode.toml. At runtime, the mode can be
/// cycled (Edit → Plan → Edit) via Shift+Tab or /plan command.
///
/// Inspired by OpenAI Codex's emphasis on structured planning before execution,
/// but provider-agnostic (works with Gemini, Anthropic, OpenAI, xAI, DeepSeek, etc.)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum EditingMode {
    /// Full tool access - can read, write, execute commands (default)
    /// Use for: Implementation, bug fixes, feature development
    #[default]
    Edit,
    /// Read-only exploration - mutating tools blocked
    /// Use for: Planning, research, architecture analysis
    /// Agent can write plans to `.vtcode/plans/` but not modify code
    Plan,
}

impl EditingMode {
    /// Return the textual representation for configuration and display
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Plan => "plan",
        }
    }

    /// Parse editing mode from user configuration input
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("edit") {
            Some(Self::Edit)
        } else if normalized.eq_ignore_ascii_case("plan") {
            Some(Self::Plan)
        } else {
            None
        }
    }

    /// Enumerate the allowed configuration values for validation
    pub fn allowed_values() -> &'static [&'static str] {
        &["edit", "plan"]
    }

    /// Check if this mode allows file modifications
    pub fn can_modify_files(self) -> bool {
        matches!(self, Self::Edit)
    }

    /// Check if this mode allows command execution
    pub fn can_execute_commands(self) -> bool {
        matches!(self, Self::Edit)
    }

    /// Check if this is read-only planning mode
    pub fn is_read_only(self) -> bool {
        matches!(self, Self::Plan)
    }
}

impl fmt::Display for EditingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EditingMode {
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

/// Configuration for the agent
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: String,
    pub api_key: String,
    pub provider: String,
    pub api_key_env: String,
    pub workspace: std::path::PathBuf,
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

/// Conversation turn information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub turn_number: usize,
    pub timestamp: u64,
    pub user_input: Option<String>,
    pub agent_response: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
    pub decision: Option<DecisionInfo>,
}

/// Tool call information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub name: String,
    pub args: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: Option<u64>,
}

/// Decision information for tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionInfo {
    pub turn_number: usize,
    pub action_type: String,
    pub description: String,
    pub reasoning: String,
    pub outcome: Option<String>,
    pub confidence_score: Option<f64>,
    pub timestamp: u64,
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

/// Task information for project workflows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub task_type: String,
    pub description: String,
    pub completed: bool,
    pub success: bool,
    pub duration_seconds: Option<u64>,
    pub tools_used: Vec<String>,
    pub dependencies: Vec<String>,
}

/// Project creation specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSpec {
    pub name: String,
    pub features: Vec<String>,
    pub template: Option<String>,
    pub dependencies: HashMap<String, String>,
}

/// Workspace analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceAnalysis {
    pub root_path: String,
    pub project_type: Option<String>,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub config_files: Vec<String>,
    pub source_files: Vec<String>,
    pub test_files: Vec<String>,
    pub documentation_files: Vec<String>,
    pub total_files: usize,
    pub total_size_bytes: u64,
}

/// Command execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub command: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub execution_time_ms: u64,
}

/// File operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOperationResult {
    pub operation: String,
    pub path: String,
    pub success: bool,
    pub details: HashMap<String, Value>,
    pub error: Option<String>,
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

/// Quality metrics for agent actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    pub decision_confidence_avg: f64,
    pub tool_success_rate: f64,
    pub error_recovery_rate: f64,
    pub context_preservation_rate: f64,
    pub user_satisfaction_score: Option<f64>,
}

/// Configuration for tool behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub enable_validation: bool,
    pub max_execution_time_seconds: u64,
    pub allow_file_creation: bool,
    pub allow_file_deletion: bool,
    pub working_directory: Option<String>,
}

/// Context management settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    pub max_context_length: usize,
    pub compression_threshold: usize,
    pub summarization_interval: usize,
    pub preservation_priority: Vec<String>,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file_logging: bool,
    pub log_directory: Option<String>,
    pub max_log_files: usize,
    pub max_log_size_mb: usize,
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

/// Compression level for context compression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionLevel {
    Light,
    Medium,
    Aggressive,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editing_mode_parse() {
        assert_eq!(EditingMode::parse("edit"), Some(EditingMode::Edit));
        assert_eq!(EditingMode::parse("EDIT"), Some(EditingMode::Edit));
        assert_eq!(EditingMode::parse("Edit"), Some(EditingMode::Edit));
        assert_eq!(EditingMode::parse("plan"), Some(EditingMode::Plan));
        assert_eq!(EditingMode::parse("PLAN"), Some(EditingMode::Plan));
        assert_eq!(EditingMode::parse("Plan"), Some(EditingMode::Plan));
        assert_eq!(EditingMode::parse("agent"), None);
        assert_eq!(EditingMode::parse("invalid"), None);
        assert_eq!(EditingMode::parse(""), None);
    }

    #[test]
    fn test_editing_mode_as_str() {
        assert_eq!(EditingMode::Edit.as_str(), "edit");
        assert_eq!(EditingMode::Plan.as_str(), "plan");
    }

    #[test]
    fn test_editing_mode_capabilities() {
        // Edit mode: full access
        assert!(EditingMode::Edit.can_modify_files());
        assert!(EditingMode::Edit.can_execute_commands());
        assert!(!EditingMode::Edit.is_read_only());

        // Plan mode: read-only
        assert!(!EditingMode::Plan.can_modify_files());
        assert!(!EditingMode::Plan.can_execute_commands());
        assert!(EditingMode::Plan.is_read_only());
    }

    #[test]
    fn test_editing_mode_default() {
        assert_eq!(EditingMode::default(), EditingMode::Edit);
    }

    #[test]
    fn test_editing_mode_display() {
        assert_eq!(format!("{}", EditingMode::Edit), "edit");
        assert_eq!(format!("{}", EditingMode::Plan), "plan");
    }

    #[test]
    fn test_editing_mode_allowed_values() {
        let values = EditingMode::allowed_values();
        assert_eq!(values, &["edit", "plan"]);
    }
}
