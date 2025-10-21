use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutputMode {
    Compact,
    Full,
}

impl Default for ToolOutputMode {
    fn default() -> Self {
        Self::Compact
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum StatusLineMode {
    #[default]
    Auto,
    Command,
    Hidden,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusLineConfig {
    #[serde(default = "default_status_line_mode")]
    pub mode: StatusLineMode,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default = "default_status_line_refresh_interval_ms")]
    pub refresh_interval_ms: u64,
    #[serde(default = "default_status_line_command_timeout_ms")]
    pub command_timeout_ms: u64,
}

impl Default for StatusLineConfig {
    fn default() -> Self {
        Self {
            mode: default_status_line_mode(),
            command: None,
            refresh_interval_ms: default_status_line_refresh_interval_ms(),
            command_timeout_ms: default_status_line_command_timeout_ms(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    #[serde(default = "default_tool_output_mode")]
    pub tool_output_mode: ToolOutputMode,
    #[serde(default = "default_inline_viewport_rows")]
    pub inline_viewport_rows: u16,
    #[serde(default = "default_show_timeline_pane")]
    pub show_timeline_pane: bool,
    #[serde(default)]
    pub status_line: StatusLineConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            tool_output_mode: default_tool_output_mode(),
            inline_viewport_rows: default_inline_viewport_rows(),
            show_timeline_pane: default_show_timeline_pane(),
            status_line: StatusLineConfig::default(),
        }
    }
}

/// PTY configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PtyConfig {
    /// Enable PTY functionality
    #[serde(default = "default_pty_enabled")]
    pub enabled: bool,

    /// Default terminal rows
    #[serde(default = "default_pty_rows")]
    pub default_rows: u16,

    /// Default terminal columns
    #[serde(default = "default_pty_cols")]
    pub default_cols: u16,

    /// Maximum number of concurrent PTY sessions
    #[serde(default = "default_max_pty_sessions")]
    pub max_sessions: usize,

    /// Command timeout in seconds
    #[serde(default = "default_pty_timeout")]
    pub command_timeout_seconds: u64,

    /// Number of PTY stdout lines to display in chat output
    #[serde(default = "default_stdout_tail_lines")]
    pub stdout_tail_lines: usize,

    /// Maximum number of scrollback lines retained per PTY session
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: usize,
}

impl Default for PtyConfig {
    fn default() -> Self {
        Self {
            enabled: default_pty_enabled(),
            default_rows: default_pty_rows(),
            default_cols: default_pty_cols(),
            max_sessions: default_max_pty_sessions(),
            command_timeout_seconds: default_pty_timeout(),
            stdout_tail_lines: default_stdout_tail_lines(),
            scrollback_lines: default_scrollback_lines(),
        }
    }
}

fn default_pty_enabled() -> bool {
    true
}

fn default_pty_rows() -> u16 {
    24
}

fn default_pty_cols() -> u16 {
    80
}

fn default_max_pty_sessions() -> usize {
    10
}

fn default_pty_timeout() -> u64 {
    300
}

fn default_stdout_tail_lines() -> usize {
    crate::constants::defaults::DEFAULT_PTY_STDOUT_TAIL_LINES
}

fn default_scrollback_lines() -> usize {
    crate::constants::defaults::DEFAULT_PTY_SCROLLBACK_LINES
}

fn default_tool_output_mode() -> ToolOutputMode {
    ToolOutputMode::Compact
}

fn default_inline_viewport_rows() -> u16 {
    crate::constants::ui::DEFAULT_INLINE_VIEWPORT_ROWS
}

fn default_show_timeline_pane() -> bool {
    crate::constants::ui::INLINE_SHOW_TIMELINE_PANE
}

fn default_status_line_mode() -> StatusLineMode {
    StatusLineMode::Auto
}

fn default_status_line_refresh_interval_ms() -> u64 {
    crate::constants::ui::STATUS_LINE_REFRESH_INTERVAL_MS
}

fn default_status_line_command_timeout_ms() -> u64 {
    crate::constants::ui::STATUS_LINE_COMMAND_TIMEOUT_MS
}
