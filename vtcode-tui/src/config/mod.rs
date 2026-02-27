pub mod constants {
    pub mod defaults;
    pub mod ui;
}

pub mod loader;
pub mod types;

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub use types::{
    ReasoningEffortLevel, SystemPromptMode, ToolDocumentationMode, UiSurfacePreference,
    VerbosityLevel,
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutputMode {
    #[default]
    Compact,
    Full,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiDisplayMode {
    Full,
    #[default]
    Minimal,
    Focused,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryMode {
    Terminal,
    #[default]
    Hybrid,
    Desktop,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentClientProtocolZedWorkspaceTrustMode {
    FullAuto,
    #[default]
    ToolsPolicy,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    Allow,
    #[default]
    Prompt,
    Deny,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyboardProtocolConfig {
    pub enabled: bool,
    pub mode: String,
    pub disambiguate_escape_codes: bool,
    pub report_event_types: bool,
    pub report_alternate_keys: bool,
    pub report_all_keys: bool,
}

impl Default for KeyboardProtocolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "default".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        }
    }
}

impl KeyboardProtocolConfig {
    pub fn validate(&self) -> Result<()> {
        match self.mode.as_str() {
            "default" | "full" | "minimal" | "custom" => Ok(()),
            _ => anyhow::bail!(
                "Invalid keyboard protocol mode '{}'. Must be: default, full, minimal, or custom",
                self.mode
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiNotificationsConfig {
    pub enabled: bool,
    pub delivery_mode: NotificationDeliveryMode,
    pub suppress_when_focused: bool,
    pub tool_failure: bool,
    pub error: bool,
    pub completion: bool,
    pub hitl: bool,
    pub tool_success: bool,
}

impl Default for UiNotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            delivery_mode: NotificationDeliveryMode::Hybrid,
            suppress_when_focused: true,
            tool_failure: true,
            error: true,
            completion: true,
            hitl: true,
            tool_success: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    pub tool_output_mode: ToolOutputMode,
    pub allow_tool_ansi: bool,
    pub inline_viewport_rows: u16,
    pub keyboard_protocol: KeyboardProtocolConfig,
    pub display_mode: UiDisplayMode,
    pub show_sidebar: bool,
    pub dim_completed_todos: bool,
    pub message_block_spacing: bool,
    pub show_turn_timer: bool,
    pub notifications: UiNotificationsConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            tool_output_mode: ToolOutputMode::Compact,
            allow_tool_ansi: false,
            inline_viewport_rows: constants::ui::DEFAULT_INLINE_VIEWPORT_ROWS,
            keyboard_protocol: KeyboardProtocolConfig::default(),
            display_mode: UiDisplayMode::Minimal,
            show_sidebar: true,
            dim_completed_todos: true,
            message_block_spacing: false,
            show_turn_timer: false,
            notifications: UiNotificationsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentCheckpointingConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentSmallModelConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentVibeCodingConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub default_model: String,
    pub theme: String,
    pub reasoning_effort: ReasoningEffortLevel,
    pub system_prompt_mode: SystemPromptMode,
    pub tool_documentation_mode: ToolDocumentationMode,
    pub verbosity: VerbosityLevel,
    pub todo_planning_mode: bool,
    pub checkpointing: AgentCheckpointingConfig,
    pub small_model: AgentSmallModelConfig,
    pub vibe_coding: AgentVibeCodingConfig,
    pub max_conversation_turns: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            default_model: "gpt-5-mini".to_string(),
            theme: "default".to_string(),
            reasoning_effort: ReasoningEffortLevel::Medium,
            system_prompt_mode: SystemPromptMode::Default,
            tool_documentation_mode: ToolDocumentationMode::Full,
            verbosity: VerbosityLevel::Medium,
            todo_planning_mode: false,
            checkpointing: AgentCheckpointingConfig::default(),
            small_model: AgentSmallModelConfig::default(),
            vibe_coding: AgentVibeCodingConfig::default(),
            max_conversation_turns: 100,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptCacheConfig {
    pub enabled: bool,
}

impl Default for PromptCacheConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpConfig {
    pub enabled: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AcpZedConfig {
    pub workspace_trust: AgentClientProtocolZedWorkspaceTrustMode,
}

impl Default for AcpZedConfig {
    fn default() -> Self {
        Self {
            workspace_trust: AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AcpConfig {
    pub zed: AcpZedConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FullAutoConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AutomationConfig {
    pub full_auto: FullAutoConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolsConfig {
    pub default_policy: ToolPolicy,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            default_policy: ToolPolicy::Prompt,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    pub human_in_the_loop: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            human_in_the_loop: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextConfig {
    pub max_context_tokens: usize,
    pub trim_to_percent: u8,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 128_000,
            trim_to_percent: 80,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyntaxHighlightingConfig {
    pub enabled: bool,
    pub theme: String,
    pub cache_themes: bool,
    pub max_file_size_mb: usize,
    pub enabled_languages: Vec<String>,
    pub highlight_timeout_ms: u64,
}

impl Default for SyntaxHighlightingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            theme: "base16-ocean.dark".to_string(),
            cache_themes: true,
            max_file_size_mb: 5,
            enabled_languages: vec![
                "rust".to_string(),
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
                "go".to_string(),
                "bash".to_string(),
                "json".to_string(),
                "yaml".to_string(),
                "toml".to_string(),
                "markdown".to_string(),
            ],
            highlight_timeout_ms: 300,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PtyConfig {
    pub enabled: bool,
    pub default_rows: u16,
    pub default_cols: u16,
    pub command_timeout_seconds: u64,
}

impl Default for PtyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_rows: 24,
            default_cols: 80,
            command_timeout_seconds: 300,
        }
    }
}

/// Convert KeyboardProtocolConfig to crossterm keyboard enhancement flags.
pub fn keyboard_protocol_to_flags(
    config: &KeyboardProtocolConfig,
) -> ratatui::crossterm::event::KeyboardEnhancementFlags {
    use ratatui::crossterm::event::KeyboardEnhancementFlags;

    if !config.enabled {
        return KeyboardEnhancementFlags::empty();
    }

    match config.mode.as_str() {
        "default" => {
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        }
        "full" => {
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        }
        "minimal" => KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        "custom" => {
            let mut flags = KeyboardEnhancementFlags::empty();
            if config.disambiguate_escape_codes {
                flags |= KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
            }
            if config.report_event_types {
                flags |= KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
            }
            if config.report_alternate_keys {
                flags |= KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
            }
            if config.report_all_keys {
                flags |= KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
            }
            flags
        }
        _ => {
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        }
    }
}
