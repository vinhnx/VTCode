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
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ReasoningDisplayMode {
    Always,
    #[default]
    Toggle,
    Hidden,
}

/// Layout mode override for responsive UI
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayoutModeOverride {
    /// Auto-detect based on terminal size
    #[default]
    Auto,
    /// Force compact mode (no borders)
    Compact,
    /// Force standard mode (borders, no sidebar/footer)
    Standard,
    /// Force wide mode (sidebar + footer)
    Wide,
}

/// UI display mode variants for quick presets
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiDisplayMode {
    /// Full UI with all features (sidebar, footer)
    Full,
    /// Minimal UI - no sidebar, no footer
    #[default]
    Minimal,
    /// Focused mode - transcript only, maximum content space
    Focused,
}

/// Notification delivery mode for terminal attention events.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryMode {
    /// Terminal-native alerts only (bell/OSC).
    Terminal,
    /// Terminal alerts with desktop notifications when supported.
    #[default]
    Hybrid,
    /// Desktop notifications first; fall back to terminal alerts when unavailable.
    Desktop,
}

/// Notification preferences for terminal and desktop alerts.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiNotificationsConfig {
    /// Master toggle for all runtime notifications.
    #[serde(default = "default_notifications_enabled")]
    pub enabled: bool,

    /// Notification transport strategy.
    #[serde(default)]
    pub delivery_mode: NotificationDeliveryMode,

    /// Suppress notifications while terminal focus is active.
    #[serde(default = "default_notifications_suppress_when_focused")]
    pub suppress_when_focused: bool,

    /// Notify when a tool call fails.
    #[serde(default = "default_notifications_tool_failure")]
    pub tool_failure: bool,

    /// Notify on runtime/system errors.
    #[serde(default = "default_notifications_error")]
    pub error: bool,

    /// Notify on turn/session completion events.
    #[serde(default = "default_notifications_completion")]
    pub completion: bool,

    /// Notify when human input/approval is required.
    #[serde(default = "default_notifications_hitl")]
    pub hitl: bool,

    /// Notify on successful tool calls.
    #[serde(default = "default_notifications_tool_success")]
    pub tool_success: bool,
}

impl Default for UiNotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: default_notifications_enabled(),
            delivery_mode: NotificationDeliveryMode::default(),
            suppress_when_focused: default_notifications_suppress_when_focused(),
            tool_failure: default_notifications_tool_failure(),
            error: default_notifications_error(),
            completion: default_notifications_completion(),
            hitl: default_notifications_hitl(),
            tool_success: default_notifications_tool_success(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    /// Tool output display mode ("compact" or "full")
    #[serde(default = "default_tool_output_mode")]
    pub tool_output_mode: ToolOutputMode,

    /// Maximum number of lines to display in tool output (prevents transcript flooding)
    #[serde(default = "default_tool_output_max_lines")]
    pub tool_output_max_lines: usize,

    /// Maximum bytes of output to display before auto-spooling to disk
    #[serde(default = "default_tool_output_spool_bytes")]
    pub tool_output_spool_bytes: usize,

    /// Optional custom directory for spooled tool output logs
    #[serde(default)]
    pub tool_output_spool_dir: Option<String>,

    /// Allow ANSI escape sequences in tool output (enables colors but may cause layout issues)
    #[serde(default = "default_allow_tool_ansi")]
    pub allow_tool_ansi: bool,

    /// Number of rows to allocate for inline UI viewport
    #[serde(default = "default_inline_viewport_rows")]
    pub inline_viewport_rows: u16,

    /// Reasoning display mode for chat UI ("always", "toggle", or "hidden")
    #[serde(default = "default_reasoning_display_mode")]
    pub reasoning_display_mode: ReasoningDisplayMode,

    /// Default visibility for reasoning when display mode is "toggle"
    #[serde(default = "default_reasoning_visible_default")]
    pub reasoning_visible_default: bool,

    /// Status line configuration settings
    #[serde(default)]
    pub status_line: StatusLineConfig,

    /// Keyboard protocol enhancements for modern terminals (e.g. Kitty protocol)
    #[serde(default)]
    pub keyboard_protocol: KeyboardProtocolConfig,

    /// Override the responsive layout mode
    #[serde(default)]
    pub layout_mode: LayoutModeOverride,

    /// UI display mode preset (full, minimal, focused)
    #[serde(default)]
    pub display_mode: UiDisplayMode,

    /// Show the right sidebar (queue, context, tools)
    #[serde(default = "default_show_sidebar")]
    pub show_sidebar: bool,

    /// Dim completed todo items (- [x]) in agent output
    #[serde(default = "default_dim_completed_todos")]
    pub dim_completed_todos: bool,

    /// Add spacing between message blocks
    #[serde(default = "default_message_block_spacing")]
    pub message_block_spacing: bool,

    /// Show per-turn elapsed timer line after completed turns
    #[serde(default = "default_show_turn_timer")]
    pub show_turn_timer: bool,

    /// Show warning/error/fatal diagnostic lines in the TUI transcript and log panel.
    /// Also controls whether ERROR-level tracing logs appear in the TUI session log.
    /// Errors are always captured in the session archive JSON regardless of this setting.
    #[serde(default = "default_show_diagnostics_in_transcript")]
    pub show_diagnostics_in_transcript: bool,

    // === Color Accessibility Configuration ===
    // Based on NO_COLOR standard, Ghostty minimum-contrast, and terminal color portability research
    // See: https://no-color.org/, https://ghostty.org/docs/config/reference#minimum-contrast
    /// Minimum contrast ratio for text against background (WCAG 2.1 standard)
    /// - 4.5: WCAG AA (default, suitable for most users)
    /// - 7.0: WCAG AAA (enhanced, for low-vision users)
    /// - 3.0: Large text minimum
    /// - 1.0: Disable contrast enforcement
    #[serde(default = "default_minimum_contrast")]
    pub minimum_contrast: f64,

    /// Compatibility mode for legacy terminals that map bold to bright colors.
    /// When enabled, avoids using bold styling on text that would become bright colors,
    /// preventing visibility issues in terminals with "bold is bright" behavior.
    #[serde(default = "default_bold_is_bright")]
    pub bold_is_bright: bool,

    /// Restrict color palette to the 11 "safe" ANSI colors portable across common themes.
    /// Safe colors: red, green, yellow, blue, magenta, cyan + brred, brgreen, brmagenta, brcyan
    /// Problematic colors avoided: brblack (invisible in Solarized Dark), bryellow (light themes),
    /// white/brwhite (light themes), brblue (Basic Dark).
    /// See: https://blog.xoria.org/terminal-colors/
    #[serde(default = "default_safe_colors_only")]
    pub safe_colors_only: bool,

    /// Color scheme mode for automatic light/dark theme switching.
    /// - "auto": Detect from terminal (via OSC 11 or COLORFGBG env var)
    /// - "light": Force light mode theme selection
    /// - "dark": Force dark mode theme selection
    #[serde(default = "default_color_scheme_mode")]
    pub color_scheme_mode: ColorSchemeMode,

    /// Notification preferences for attention events.
    #[serde(default)]
    pub notifications: UiNotificationsConfig,

    /// Screen reader mode: disables animations, uses plain text indicators,
    /// and optimizes output for assistive technology compatibility.
    /// Can also be enabled via VTCODE_SCREEN_READER=1 environment variable.
    #[serde(default = "default_screen_reader_mode")]
    pub screen_reader_mode: bool,

    /// Reduce motion mode: minimizes shimmer/flashing animations.
    /// Can also be enabled via VTCODE_REDUCE_MOTION=1 environment variable.
    #[serde(default = "default_reduce_motion_mode")]
    pub reduce_motion_mode: bool,

    /// Keep animated progress indicators while reduce_motion_mode is enabled.
    #[serde(default = "default_reduce_motion_keep_progress_animation")]
    pub reduce_motion_keep_progress_animation: bool,
}

/// Color scheme mode for theme selection
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorSchemeMode {
    /// Detect from terminal environment (OSC 11 query or COLORFGBG)
    #[default]
    Auto,
    /// Force light color scheme
    Light,
    /// Force dark color scheme
    Dark,
}

fn default_minimum_contrast() -> f64 {
    crate::constants::ui::THEME_MIN_CONTRAST_RATIO
}

fn default_bold_is_bright() -> bool {
    false
}

fn default_safe_colors_only() -> bool {
    false
}

fn default_color_scheme_mode() -> ColorSchemeMode {
    ColorSchemeMode::Auto
}

fn default_show_sidebar() -> bool {
    true
}

fn default_dim_completed_todos() -> bool {
    true
}

fn default_message_block_spacing() -> bool {
    true
}

fn default_show_turn_timer() -> bool {
    true
}

fn default_show_diagnostics_in_transcript() -> bool {
    false
}

fn default_notifications_enabled() -> bool {
    true
}

fn default_notifications_suppress_when_focused() -> bool {
    true
}

fn default_notifications_tool_failure() -> bool {
    true
}

fn default_notifications_error() -> bool {
    true
}

fn default_notifications_completion() -> bool {
    true
}

fn default_notifications_hitl() -> bool {
    true
}

fn default_notifications_tool_success() -> bool {
    false
}

fn env_bool_var(name: &str) -> Option<bool> {
    std::env::var(name).ok().and_then(|v| {
        let normalized = v.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

fn default_screen_reader_mode() -> bool {
    env_bool_var("VTCODE_SCREEN_READER").unwrap_or(false)
}

fn default_reduce_motion_mode() -> bool {
    env_bool_var("VTCODE_REDUCE_MOTION").unwrap_or(false)
}

fn default_reduce_motion_keep_progress_animation() -> bool {
    false
}

fn default_ask_questions_enabled() -> bool {
    true
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
            reasoning_display_mode: default_reasoning_display_mode(),
            reasoning_visible_default: default_reasoning_visible_default(),
            status_line: StatusLineConfig::default(),
            keyboard_protocol: KeyboardProtocolConfig::default(),
            layout_mode: LayoutModeOverride::default(),
            display_mode: UiDisplayMode::default(),
            show_sidebar: default_show_sidebar(),
            dim_completed_todos: default_dim_completed_todos(),
            message_block_spacing: default_message_block_spacing(),
            show_turn_timer: default_show_turn_timer(),
            show_diagnostics_in_transcript: default_show_diagnostics_in_transcript(),
            // Color accessibility defaults
            minimum_contrast: default_minimum_contrast(),
            bold_is_bright: default_bold_is_bright(),
            safe_colors_only: default_safe_colors_only(),
            color_scheme_mode: default_color_scheme_mode(),
            notifications: UiNotificationsConfig::default(),
            screen_reader_mode: default_screen_reader_mode(),
            reduce_motion_mode: default_reduce_motion_mode(),
            reduce_motion_keep_progress_animation: default_reduce_motion_keep_progress_animation(),
        }
    }
}

/// Chat configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ChatConfig {
    /// Ask Questions tool configuration (chat.askQuestions.*)
    #[serde(default, rename = "askQuestions", alias = "ask_questions")]
    pub ask_questions: AskQuestionsConfig,
}

/// Ask Questions tool configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AskQuestionsConfig {
    /// Enable the Ask Questions tool in interactive chat
    #[serde(default = "default_ask_questions_enabled")]
    pub enabled: bool,
}

impl Default for AskQuestionsConfig {
    fn default() -> Self {
        Self {
            enabled: default_ask_questions_enabled(),
        }
    }
}

/// PTY configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PtyConfig {
    /// Enable PTY support for interactive commands
    #[serde(default = "default_pty_enabled")]
    pub enabled: bool,

    /// Default terminal rows for PTY sessions
    #[serde(default = "default_pty_rows")]
    pub default_rows: u16,

    /// Default terminal columns for PTY sessions
    #[serde(default = "default_pty_cols")]
    pub default_cols: u16,

    /// Maximum number of concurrent PTY sessions
    #[serde(default = "default_max_pty_sessions")]
    pub max_sessions: usize,

    /// Command timeout in seconds (prevents hanging commands)
    #[serde(default = "default_pty_timeout")]
    pub command_timeout_seconds: u64,

    /// Number of recent PTY output lines to display in the chat transcript
    #[serde(default = "default_stdout_tail_lines")]
    pub stdout_tail_lines: usize,

    /// Total scrollback buffer size (lines) retained per PTY session
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: usize,

    /// Maximum bytes of output to retain per PTY session (prevents memory explosion)
    #[serde(default = "default_max_scrollback_bytes")]
    pub max_scrollback_bytes: usize,

    /// Threshold (KB) at which to auto-spool large outputs to disk instead of memory
    #[serde(default = "default_large_output_threshold_kb")]
    pub large_output_threshold_kb: usize,

    /// Preferred shell program for PTY sessions (e.g. "zsh", "bash"); falls back to $SHELL
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

fn default_reasoning_display_mode() -> ReasoningDisplayMode {
    ReasoningDisplayMode::Toggle
}

fn default_reasoning_visible_default() -> bool {
    crate::constants::ui::DEFAULT_REASONING_VISIBLE
}

/// Kitty keyboard protocol configuration
/// Reference: https://sw.kovidgoyal.net/kitty/keyboard-protocol/
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyboardProtocolConfig {
    /// Enable keyboard protocol enhancements (master toggle)
    #[serde(default = "default_keyboard_protocol_enabled")]
    pub enabled: bool,

    /// Preset mode: "default", "full", "minimal", or "custom"
    #[serde(default = "default_keyboard_protocol_mode")]
    pub mode: String,

    /// Resolve Esc key ambiguity (recommended for performance)
    #[serde(default = "default_disambiguate_escape_codes")]
    pub disambiguate_escape_codes: bool,

    /// Report press, release, and repeat events
    #[serde(default = "default_report_event_types")]
    pub report_event_types: bool,

    /// Report alternate key layouts (e.g. for non-US keyboards)
    #[serde(default = "default_report_alternate_keys")]
    pub report_alternate_keys: bool,

    /// Report all keys, including modifier-only keys (Shift, Ctrl)
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
