//! Common types and interfaces used throughout the application

use crate::core::PromptCachingConfig;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::env;
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
    /// Minimal prompt (~500 tokens base) - Pi-inspired, modern models need less guidance
    /// Best for: Power users, token-constrained contexts, fast responses
    #[default]
    Minimal,
    /// Lightweight prompt (~750 tokens base) - Essential guidance only
    /// Best for: Resource-constrained operations, simple tasks
    Lightweight,
    /// Default prompt (~900 tokens base) - Full guidance with all features
    /// Best for: General usage, comprehensive error handling
    Default,
    /// Specialized prompt (~900 tokens base) - Complex refactoring and analysis
    /// Best for: Multi-file changes, sophisticated code analysis
    Specialized,
}

impl SystemPromptMode {
    /// Return the textual representation for configuration
    fn as_str(self) -> &'static str {
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
    /// First sentence of each tool description (at most 64 chars) and no
    /// parameter descriptions - Pi-style, power users
    /// Best for: Maximum efficiency, experienced users, token-constrained contexts
    Minimal,
    /// Complete tool and parameter descriptions; only unusually long tails are
    /// trimmed at a sentence boundary (measured ~1,793 tokens for the default
    /// catalog; budgeted ≤ 2,000 in `emitted_model_tool_schema_fits_within_first_request_budget`)
    /// Best for: General usage (recommended)
    #[default]
    Progressive,
    /// Every tool and parameter description sent unmodified
    /// Best for: Debugging tool definitions, comprehensive documentation
    Full,
}

impl ToolDocumentationMode {
    /// Return the textual representation for configuration
    fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Progressive => "progressive",
            Self::Full => "full",
        }
    }

    /// Parse tool documentation mode from user configuration
    fn parse(value: &str) -> Option<Self> {
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

/// Configured shell syntax profile used for model-facing command examples.
///
/// This is prompt guidance only. Command approval, sandboxing, and allow-list
/// policy are enforced elsewhere in the runtime.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ShellPromptProfile {
    /// Detect from the current platform. Unix-like shells are used for Linux,
    /// macOS, and WSL. Native Windows uses PowerShell.
    #[default]
    Auto,
    /// Use Unix-like shell syntax in prompt examples.
    UnixLike,
    /// Use native PowerShell syntax in prompt examples.
    ///
    /// The canonical wire value is `powershell` (matching `Self::as_str`,
    /// [`Self::allowed_values`], and the docs); `snake_case` would otherwise
    /// render it as the drifting `power_shell`.
    #[serde(rename = "powershell")]
    PowerShell,
}

impl ShellPromptProfile {
    /// Return the textual representation for configuration.
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::UnixLike => "unix_like",
            Self::PowerShell => "powershell",
        }
    }

    /// Parse a shell prompt profile from user configuration.
    fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("auto") {
            Some(Self::Auto)
        } else if normalized.eq_ignore_ascii_case("unix_like")
            || normalized.eq_ignore_ascii_case("unix-like")
            || normalized.eq_ignore_ascii_case("unix")
            || normalized.eq_ignore_ascii_case("posix")
        {
            Some(Self::UnixLike)
        } else if normalized.eq_ignore_ascii_case("powershell")
            || normalized.eq_ignore_ascii_case("power_shell")
            || normalized.eq_ignore_ascii_case("windows")
        {
            Some(Self::PowerShell)
        } else {
            None
        }
    }

    /// Allowed configuration values for validation.
    pub fn allowed_values() -> &'static [&'static str] {
        &["auto", "unix_like", "powershell"]
    }

    /// Resolve this configured value against the current runtime platform.
    pub fn resolve_for_current_platform(self) -> ResolvedShellPromptProfile {
        self.resolve_for_platform(ShellProfilePlatform::current())
    }

    /// Resolve this configured value against an explicit platform.
    fn resolve_for_platform(self, platform: ShellProfilePlatform) -> ResolvedShellPromptProfile {
        match self {
            Self::Auto => match platform {
                ShellProfilePlatform::NativeWindows => ResolvedShellPromptProfile::PowerShell,
                ShellProfilePlatform::UnixLike | ShellProfilePlatform::Wsl => ResolvedShellPromptProfile::UnixLike,
            },
            Self::UnixLike => ResolvedShellPromptProfile::UnixLike,
            Self::PowerShell => ResolvedShellPromptProfile::PowerShell,
        }
    }
}

impl fmt::Display for ShellPromptProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ShellPromptProfile {
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

/// Platform category used to resolve [`ShellPromptProfile::Auto`].
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellProfilePlatform {
    UnixLike,
    Wsl,
    NativeWindows,
}

impl ShellProfilePlatform {
    fn current() -> Self {
        if cfg!(windows) {
            Self::NativeWindows
        } else if is_wsl_environment() {
            Self::Wsl
        } else {
            Self::UnixLike
        }
    }
}

/// Prompt profile after automatic platform resolution.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedShellPromptProfile {
    UnixLike,
    /// Canonical wire value is `powershell` (matching `Self::as_str`); the
    /// derived `snake_case` form would otherwise be the drifting `power_shell`.
    #[serde(rename = "powershell")]
    PowerShell,
}

impl ResolvedShellPromptProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::UnixLike => "unix_like",
            Self::PowerShell => "powershell",
        }
    }
}

impl fmt::Display for ResolvedShellPromptProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn is_wsl_environment() -> bool {
    if env::var_os("WSL_DISTRO_NAME").is_some() || env::var_os("WSL_INTEROP").is_some() {
        return true;
    }

    for path in ["/proc/sys/kernel/osrelease", "/proc/version"] {
        if let Ok(contents) = std::fs::read_to_string(path)
            && contents.to_ascii_lowercase().contains("microsoft")
        {
            return true;
        }
    }

    false
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
    Auto,
    Alternate,
    #[default]
    Inline,
}

impl UiSurfacePreference {
    /// String representation used in configuration and logging
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Alternate => "alternate",
            Self::Inline => "inline",
        }
    }

    /// Parse a surface preference from configuration input
    fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.eq_ignore_ascii_case("auto") {
            Some(Self::Auto)
        } else if normalized.eq_ignore_ascii_case("alternate") || normalized.eq_ignore_ascii_case("alt") {
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
    error_type: String,
    message: String,
    turn_number: usize,
    recoverable: bool,
    timestamp: u64,
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
        assert_eq!(ReasoningEffortLevel::parse("max"), Some(ReasoningEffortLevel::Max));
        assert_eq!(ReasoningEffortLevel::Max.as_str(), "max");
        assert!(ReasoningEffortLevel::allowed_values().contains(&"max"));
    }

    #[test]
    fn shell_prompt_profile_parses_aliases() {
        assert_eq!(ShellPromptProfile::parse("auto"), Some(ShellPromptProfile::Auto));
        assert_eq!(ShellPromptProfile::parse("unix-like"), Some(ShellPromptProfile::UnixLike));
        assert_eq!(ShellPromptProfile::parse("posix"), Some(ShellPromptProfile::UnixLike));
        assert_eq!(ShellPromptProfile::parse("PowerShell"), Some(ShellPromptProfile::PowerShell));
        assert_eq!(ShellPromptProfile::parse("unknown"), None);
    }

    #[test]
    fn ui_surface_defaults_to_inline() {
        assert_eq!(UiSurfacePreference::default(), UiSurfacePreference::Inline);
    }

    #[test]
    fn auto_shell_prompt_profile_selects_platform_defaults() {
        let profile = ShellPromptProfile::Auto;

        assert_eq!(profile.resolve_for_platform(ShellProfilePlatform::UnixLike), ResolvedShellPromptProfile::UnixLike);
        assert_eq!(profile.resolve_for_platform(ShellProfilePlatform::Wsl), ResolvedShellPromptProfile::UnixLike);
        assert_eq!(
            profile.resolve_for_platform(ShellProfilePlatform::NativeWindows),
            ResolvedShellPromptProfile::PowerShell
        );
    }

    #[test]
    fn explicit_shell_prompt_profiles_override_platform_defaults() {
        assert_eq!(
            ShellPromptProfile::UnixLike.resolve_for_platform(ShellProfilePlatform::NativeWindows),
            ResolvedShellPromptProfile::UnixLike
        );
        assert_eq!(
            ShellPromptProfile::PowerShell.resolve_for_platform(ShellProfilePlatform::Wsl),
            ResolvedShellPromptProfile::PowerShell
        );
    }

    /// Every `as_str()`/`parse()`/serde triangle in this module stays in
    /// lockstep; see [`crate::test_support::assert_string_enum_lockstep`].
    #[test]
    fn string_enums_keep_as_str_and_serde_in_lockstep() {
        use crate::test_support::assert_string_enum_lockstep;

        assert_string_enum_lockstep!(
            SystemPromptMode,
            [
                SystemPromptMode::Minimal,
                SystemPromptMode::Lightweight,
                SystemPromptMode::Default,
                SystemPromptMode::Specialized,
            ]
        );
        assert_string_enum_lockstep!(
            ToolDocumentationMode,
            [
                ToolDocumentationMode::Minimal,
                ToolDocumentationMode::Progressive,
                ToolDocumentationMode::Full,
            ]
        );
        assert_string_enum_lockstep!(
            ShellPromptProfile,
            [
                ShellPromptProfile::Auto,
                ShellPromptProfile::UnixLike,
                ShellPromptProfile::PowerShell,
            ]
        );
        assert_string_enum_lockstep!(
            VerbosityLevel,
            [VerbosityLevel::Low, VerbosityLevel::Medium, VerbosityLevel::High]
        );
        assert_string_enum_lockstep!(
            UiSurfacePreference,
            [
                UiSurfacePreference::Auto,
                UiSurfacePreference::Alternate,
                UiSurfacePreference::Inline,
            ]
        );

        // The shared `allowed_values()` lists are the model-facing validation
        // surface; pin them against `parse()` so a rename cannot silently drop
        // a value from the accepted set.
        for value in SystemPromptMode::allowed_values() {
            assert!(SystemPromptMode::parse(value).is_some(), "allowed value {value:?} must parse");
        }
        for value in ToolDocumentationMode::allowed_values() {
            assert!(ToolDocumentationMode::parse(value).is_some(), "allowed value {value:?} must parse");
        }
        for value in ShellPromptProfile::allowed_values() {
            assert!(ShellPromptProfile::parse(value).is_some(), "allowed value {value:?} must parse");
        }
        for value in VerbosityLevel::allowed_values() {
            assert!(VerbosityLevel::parse(value).is_some(), "allowed value {value:?} must parse");
        }
        for value in UiSurfacePreference::allowed_values() {
            assert!(UiSurfacePreference::parse(value).is_some(), "allowed value {value:?} must parse");
        }
    }

    /// `ResolvedShellPromptProfile` has no `parse()`, so it is not covered by
    /// [`crate::test_support::assert_string_enum_lockstep`]. It still exposes the
    /// same value through `Display`/`as_str()` and the derived `snake_case`
    /// serde form — and it carries a schema derive — so pin those together.
    #[test]
    fn resolved_shell_prompt_profile_matches_display_and_serde() {
        for profile in [
            ResolvedShellPromptProfile::UnixLike,
            ResolvedShellPromptProfile::PowerShell,
        ] {
            let expected = profile.to_string();
            assert_eq!(
                serde_json::to_value(profile).expect("resolved profile serializes"),
                serde_json::json!(expected),
                "serde wire form drifted from Display for {profile:?}"
            );
        }
    }
}
