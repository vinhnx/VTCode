use serde::{Deserialize, Serialize};

use crate::status_line::StatusLineConfig;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ToolOutputMode {
    #[default]
    Compact,
    Full,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    #[serde(default = "default_tool_output_mode")]
    pub tool_output_mode: ToolOutputMode,
    #[serde(default = "default_tool_output_max_lines")]
    pub tool_output_max_lines: usize,
    #[serde(default = "default_tool_output_spool_bytes")]
    pub tool_output_spool_bytes: usize,
    #[serde(default)]
    pub tool_output_spool_dir: Option<String>,
    #[serde(default = "default_allow_tool_ansi")]
    pub allow_tool_ansi: bool,
    #[serde(default = "default_inline_viewport_rows")]
    pub inline_viewport_rows: u16,
    #[serde(default = "default_show_timeline_pane")]
    pub show_timeline_pane: bool,
    #[serde(default)]
    pub status_line: StatusLineConfig,
    #[serde(default)]
    pub keyboard_protocol: KeyboardProtocolConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            tool_output_mode: default_tool_output_mode(),
            tool_output_max_lines: default_tool_output_max_lines(),
            tool_output_spool_bytes: default_tool_output_spool_bytes(),
            tool_output_spool_dir: None,
            allow_tool_ansi: default_allow_tool_ansi(),
            inline_viewport_rows: default_inline_viewport_rows(),
            show_timeline_pane: default_show_timeline_pane(),
            status_line: StatusLineConfig::default(),
            keyboard_protocol: KeyboardProtocolConfig::default(),
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

    /// Maximum bytes of output to retain per PTY session (prevents memory explosion)
    #[serde(default = "default_max_scrollback_bytes")]
    pub max_scrollback_bytes: usize,

    /// Threshold (KB) at which to auto-spool large outputs to disk
    #[serde(default = "default_large_output_threshold_kb")]
    pub large_output_threshold_kb: usize,

    /// Preferred shell program for PTY sessions (falls back to environment when unset)
    #[serde(default)]
    pub preferred_shell: Option<String>,
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
            max_scrollback_bytes: default_max_scrollback_bytes(),
            large_output_threshold_kb: default_large_output_threshold_kb(),
            preferred_shell: None,
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

fn default_max_scrollback_bytes() -> usize {
    // Reduced from 50MB to 25MB for memory-constrained development environments
    // Can be overridden in vtcode.toml with: pty.max_scrollback_bytes = 52428800
    25_000_000 // 25MB max to prevent memory explosion
}

fn default_large_output_threshold_kb() -> usize {
    5_000 // 5MB threshold for auto-spooling
}

fn default_tool_output_mode() -> ToolOutputMode {
    ToolOutputMode::Compact
}

fn default_tool_output_max_lines() -> usize {
    600
}

fn default_tool_output_spool_bytes() -> usize {
    200_000
}

fn default_allow_tool_ansi() -> bool {
    false
}

fn default_inline_viewport_rows() -> u16 {
    crate::constants::ui::DEFAULT_INLINE_VIEWPORT_ROWS
}

fn default_show_timeline_pane() -> bool {
    crate::constants::ui::INLINE_SHOW_TIMELINE_PANE
}

/// Kitty keyboard protocol configuration
/// Reference: https://sw.kovidgoyal.net/kitty/keyboard-protocol/
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyboardProtocolConfig {
    /// Enable keyboard protocol enhancements (master toggle)
    #[serde(default = "default_keyboard_protocol_enabled")]
    pub enabled: bool,

    /// Preset mode: "default", "full", "minimal", "custom"
    #[serde(default = "default_keyboard_protocol_mode")]
    pub mode: String,

    /// Individual flag controls (used when mode = "custom")
    /// Resolve Esc key ambiguity (recommended)
    #[serde(default = "default_disambiguate_escape_codes")]
    pub disambiguate_escape_codes: bool,

    /// Report press/release/repeat events
    #[serde(default = "default_report_event_types")]
    pub report_event_types: bool,

    /// Report alternate key layouts
    #[serde(default = "default_report_alternate_keys")]
    pub report_alternate_keys: bool,

    /// Report modifier-only keys (Shift, Ctrl, Alt alone)
    #[serde(default = "default_report_all_keys")]
    pub report_all_keys: bool,
}

impl Default for KeyboardProtocolConfig {
    fn default() -> Self {
        Self {
            enabled: default_keyboard_protocol_enabled(),
            mode: default_keyboard_protocol_mode(),
            disambiguate_escape_codes: default_disambiguate_escape_codes(),
            report_event_types: default_report_event_types(),
            report_alternate_keys: default_report_alternate_keys(),
            report_all_keys: default_report_all_keys(),
        }
    }
}

impl KeyboardProtocolConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        match self.mode.as_str() {
            "default" | "full" | "minimal" | "custom" => Ok(()),
            _ => anyhow::bail!(
                "Invalid keyboard protocol mode '{}'. Must be: default, full, minimal, or custom",
                self.mode
            ),
        }
    }
}

fn default_keyboard_protocol_enabled() -> bool {
    std::env::var("VTCODE_KEYBOARD_PROTOCOL_ENABLED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true)
}

fn default_keyboard_protocol_mode() -> String {
    std::env::var("VTCODE_KEYBOARD_PROTOCOL_MODE").unwrap_or_else(|_| "default".to_string())
}

fn default_disambiguate_escape_codes() -> bool {
    true
}

fn default_report_event_types() -> bool {
    true
}

fn default_report_alternate_keys() -> bool {
    true
}

fn default_report_all_keys() -> bool {
    false
}
