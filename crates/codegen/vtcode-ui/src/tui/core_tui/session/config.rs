//! Configuration system for TUI session UI preferences
//!
//! Contains settings for customizable UI elements, colors, key bindings, and other preferences.

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::path::Path;
use vtcode_commons::fs::{read_file_with_context_sync, write_file_with_context_sync};

/// Main configuration struct for TUI session preferences
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionConfig {
    /// UI appearance settings
    appearance: AppearanceConfig,

    /// Key binding preferences
    key_bindings: KeyBindingConfig,

    /// Behavior preferences
    behavior: BehaviorConfig,

    /// Performance related settings
    performance: PerformanceConfig,

    /// Customization settings
    customization: CustomizationConfig,
}

// Re-export shared enums from vtcode-commons.
pub use vtcode_commons::ui_protocol::{
    DiffPreviewMode, LayoutModeOverride, ReasoningDisplayMode, ThinkingBlockState, UiMode,
};

/// UI appearance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    /// Color theme to use
    pub theme: String,

    /// UI mode variant (full, minimal, focused)
    pub ui_mode: UiMode,

    /// Whether to show the right sidebar (queue, context, tools)
    pub show_sidebar: bool,

    /// Minimum width for content area
    pub min_content_width: u16,

    /// Minimum width for navigation area
    pub min_navigation_width: u16,

    /// Percentage of width for navigation area
    pub navigation_width_percent: u8,

    /// Transcript bottom padding.
    ///
    /// Deprecated: retained for serialized-config compatibility only (including
    /// the `vtcode-core` mirror field, which carries the same note). The live
    /// transcript uses `INLINE_TRANSCRIPT_BOTTOM_PADDING`
    /// via `effective_transcript_bottom_padding()`;
    /// this field is never read at render time.
    pub transcript_bottom_padding: u16,

    /// Whether to dim completed todo items (- \[x\] and ~~strikethrough~~)
    pub dim_completed_todos: bool,

    /// Automatically show the plan-mode TODO task tracking panel when a plan
    /// is approved or the task tracker is updated.
    #[serde(default)]
    pub show_task_panel: bool,

    /// Number of blank lines between message blocks (0-2)
    pub message_block_spacing: u8,

    /// Override responsive layout mode
    #[serde(default)]
    pub layout_mode: LayoutModeOverride,

    /// Reasoning visibility mode
    #[serde(default)]
    pub reasoning_display_mode: ReasoningDisplayMode,

    /// Default reasoning visibility when mode is "toggle"
    #[serde(default)]
    pub reasoning_visible_default: bool,

    /// Default collapse state of agent thinking/reasoning blocks
    #[serde(default)]
    pub thinking_display: ThinkingBlockState,

    /// Enable Vim-style input editing for the prompt.
    #[serde(default)]
    pub vim_mode: bool,

    /// Enable Readline-style input editing for the prompt.
    /// This adds Emacs-style keybindings like Ctrl+F/B for navigation,
    /// Ctrl+P/N for history, Alt+D for kill word forward, etc.
    #[serde(default)]
    pub readline_mode: bool,

    /// Screen reader mode (disables animation-heavy rendering paths)
    #[serde(default)]
    pub screen_reader_mode: bool,

    /// Reduce motion mode (keeps progress labels visible without animated effects)
    #[serde(default)]
    pub reduce_motion_mode: bool,

    /// Keep progress animation while reduce motion mode is enabled.
    /// Screen reader mode still disables progress animation.
    #[serde(default)]
    pub reduce_motion_keep_progress_animation: bool,

    /// Hide the full TUI header, showing only version info in a compact line
    #[serde(default)]
    pub hide_header: bool,

    /// Show the clickable/keyboard affordance on compact command rows.
    #[serde(default = "default_transcript_review_control")]
    pub show_transcript_review_hints: bool,

    /// Show the keyboard guide footer inside Transcript Review.
    #[serde(default = "default_transcript_review_control")]
    pub show_transcript_review_shortcut_guide: bool,

    /// Show the mouse-clickable close control in the Transcript Review title.
    #[serde(default = "default_transcript_review_control")]
    pub show_transcript_review_close_button: bool,

    /// Diff preview layout for file-edit approval overlays.
    #[serde(default)]
    pub diff_preview_mode: DiffPreviewMode,

    /// Syntax highlighting for the code surfaces the TUI renders itself
    /// (diagnostic evidence bodies). The code palette follows the active UI
    /// theme; this decides whether highlighting happens and which languages
    /// are eligible.
    #[serde(default)]
    pub syntax_highlighting: vtcode_config::loader::SyntaxHighlightingConfig,

    /// Customization settings
    pub customization: CustomizationConfig,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: "default".to_owned(),
            ui_mode: UiMode::Full,
            show_sidebar: true,
            min_content_width: 40,
            min_navigation_width: 20,
            navigation_width_percent: 25,
            transcript_bottom_padding: 0,
            dim_completed_todos: true,
            show_task_panel: false,
            // Default 1 provides a single blank line between blocks (unified vertical rhythm).
            // `tool_block_spacing` clamps 1..2, so tool blocks remain visually grouped even when
            // this is 0. Previously 0 produced a cramped transcript; 1 matches the approved UX.
            message_block_spacing: 1,
            layout_mode: LayoutModeOverride::Auto,
            reasoning_display_mode: ReasoningDisplayMode::Toggle,
            reasoning_visible_default: crate::tui::config::constants::ui::DEFAULT_REASONING_VISIBLE,
            thinking_display: ThinkingBlockState::Collapsed,
            vim_mode: false,
            readline_mode: false,
            screen_reader_mode: false,
            reduce_motion_mode: false,
            reduce_motion_keep_progress_animation: false,
            hide_header: true,
            show_transcript_review_hints: default_transcript_review_control(),
            show_transcript_review_shortcut_guide: default_transcript_review_control(),
            show_transcript_review_close_button: default_transcript_review_control(),
            diff_preview_mode: DiffPreviewMode::Inline,
            syntax_highlighting: vtcode_config::loader::SyntaxHighlightingConfig::default(),
            customization: CustomizationConfig::default(),
        }
    }
}

fn default_transcript_review_control() -> bool {
    true
}

impl AppearanceConfig {
    /// Check if sidebar should be shown based on ui_mode and show_sidebar
    pub(crate) fn should_show_sidebar(&self) -> bool {
        match self.ui_mode {
            UiMode::Full => self.show_sidebar,
            UiMode::Minimal | UiMode::Focused => false,
        }
    }

    /// Whether the plan-mode TODO task tracking panel may auto-show.
    pub(crate) fn should_show_task_panel(&self) -> bool {
        self.show_task_panel
    }

    fn reasoning_visible(&self) -> bool {
        match self.reasoning_display_mode {
            ReasoningDisplayMode::Always => true,
            ReasoningDisplayMode::Hidden => false,
            ReasoningDisplayMode::Toggle => self.reasoning_visible_default,
        }
    }

    /// Whether thinking/reasoning blocks should render collapsed by default.
    pub(crate) fn thinking_collapsed_by_default(&self) -> bool {
        self.thinking_display == ThinkingBlockState::Collapsed
    }

    /// Whether progress effects animate; screen reader mode always suppresses them.
    pub(crate) fn should_animate_progress_status(&self) -> bool {
        !self.screen_reader_mode && (!self.reduce_motion_mode || self.reduce_motion_keep_progress_animation)
    }

    /// Check if footer should be shown based on ui_mode
    pub fn should_show_footer(&self) -> bool {
        match self.ui_mode {
            UiMode::Full => true,
            UiMode::Minimal => false,
            UiMode::Focused => false,
        }
    }
}

/// Key binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindingConfig {
    /// Map of action to key sequences
    bindings: HashMap<String, Vec<String>>,
}

impl Default for KeyBindingConfig {
    fn default() -> Self {
        let mut bindings = HashMap::new();

        // Navigation
        bindings.insert("scroll_up".to_owned(), vec!["up".to_owned()]);
        bindings.insert("scroll_down".to_owned(), vec!["down".to_owned()]);
        bindings.insert("page_up".to_owned(), vec!["pageup".to_owned()]);
        bindings.insert("page_down".to_owned(), vec!["pagedown".to_owned()]);

        // Input
        bindings.insert("submit".to_owned(), vec!["enter".to_owned()]);
        bindings.insert("submit_queue".to_owned(), vec!["tab".to_owned()]);
        bindings.insert("cancel".to_owned(), vec!["esc".to_owned()]);
        bindings.insert("interrupt".to_owned(), vec!["ctrl+c".to_owned()]);

        Self { bindings }
    }
}

/// Behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorConfig {
    /// Maximum lines for input area
    max_input_lines: usize,

    /// Whether to enable command history
    enable_history: bool,

    /// History size limit
    history_size: usize,

    /// Whether to show queued inputs
    show_queued_inputs: bool,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            max_input_lines: 10,
            enable_history: true,
            history_size: 100,
            show_queued_inputs: true,
        }
    }
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Cache size for rendered elements
    render_cache_size: usize,

    /// Transcript cache size (number of messages to cache)
    transcript_cache_size: usize,

    /// Whether to enable transcript reflow caching
    enable_transcript_caching: bool,

    /// Size of LRU cache for expensive operations
    lru_cache_size: usize,

    /// Whether to enable smooth scrolling
    enable_smooth_scrolling: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            render_cache_size: 1000,
            transcript_cache_size: 500,
            enable_transcript_caching: true,
            lru_cache_size: 128,
            enable_smooth_scrolling: false,
        }
    }
}

/// Customization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomizationConfig {
    /// User-defined UI labels
    ui_labels: HashMap<String, String>,

    /// Custom styling options
    custom_styles: HashMap<String, String>,

    /// Enabled UI features
    enabled_features: Vec<String>,
}

impl Default for CustomizationConfig {
    fn default() -> Self {
        Self {
            ui_labels: HashMap::new(),
            custom_styles: HashMap::new(),
            enabled_features: vec![
                "slash_commands".to_owned(),
                "file_palette".to_owned(),
                "modal_dialogs".to_owned(),
            ],
        }
    }
}

impl SessionConfig {
    /// Creates a new default configuration
    fn new() -> Self {
        Self::default()
    }

    /// Loads configuration from a file
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = read_file_with_context_sync(Path::new(path), "session config file")
            .map_err(|err| -> Box<dyn std::error::Error> { Box::new(std::io::Error::other(err)) })?;
        let config: SessionConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Saves configuration to a file
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        write_file_with_context_sync(Path::new(path), &content, "session config file")
            .map_err(|err| -> Box<dyn std::error::Error> { Box::new(std::io::Error::other(err)) })?;
        Ok(())
    }

    /// Updates a specific configuration value by key
    fn set_value(&mut self, key: &str, value: &str) -> Result<(), String> {
        // This is a simplified version - in a real implementation, we'd have more sophisticated
        // parsing and validation for different configuration types
        match key {
            "behavior.max_input_lines" => {
                self.behavior.max_input_lines =
                    value.parse().map_err(|_| format!("Cannot parse '{value}' as number"))?;
            }
            "performance.lru_cache_size" => {
                self.performance.lru_cache_size =
                    value.parse().map_err(|_| format!("Cannot parse '{value}' as number"))?;
            }
            _ => return Err(format!("Unknown configuration key: {key}")),
        }
        Ok(())
    }

    /// Gets a configuration value by key
    fn get_value(&self, key: &str) -> Option<String> {
        match key {
            "behavior.max_input_lines" => Some(self.behavior.max_input_lines.to_string()),
            "performance.lru_cache_size" => Some(self.performance.lru_cache_size.to_string()),
            _ => None,
        }
    }

    /// Validates the configuration to ensure all values are within acceptable ranges
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.behavior.history_size == 0 {
            errors.push("history_size must be greater than 0".to_owned());
        }

        if self.performance.lru_cache_size == 0 {
            errors.push("lru_cache_size must be greater than 0".to_owned());
        }

        if self.appearance.navigation_width_percent > 100 {
            errors.push("navigation_width_percent must be between 0 and 100".to_owned());
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SessionConfig::new();
        assert_eq!(config.behavior.history_size, 100);
        assert_eq!(config.appearance.reasoning_display_mode, ReasoningDisplayMode::Toggle);
        assert!(config.appearance.reasoning_visible_default);
        assert!(config.appearance.reasoning_visible());
        assert_eq!(config.appearance.thinking_display, ThinkingBlockState::Collapsed);
        assert!(config.appearance.thinking_collapsed_by_default());
    }

    #[test]
    fn test_task_panel_default_hidden() {
        let mut config = SessionConfig::new();
        assert!(!config.appearance.show_task_panel);
        assert!(!config.appearance.should_show_task_panel());

        config.appearance.show_task_panel = true;
        assert!(config.appearance.should_show_task_panel());
    }

    #[test]
    fn test_thinking_display_default_collapsed() {
        let config = SessionConfig::new();
        assert_eq!(config.appearance.thinking_display, ThinkingBlockState::Collapsed);
        assert!(config.appearance.thinking_collapsed_by_default());
    }

    #[test]
    fn test_thinking_display_override_extended() {
        let mut config = SessionConfig::new();
        config.appearance.thinking_display = ThinkingBlockState::Extended;
        assert!(!config.appearance.thinking_collapsed_by_default());
    }

    #[test]
    fn test_config_serialization() {
        let config = SessionConfig::new();
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(serialized.contains("theme"));
    }

    #[test]
    fn test_config_value_setting() {
        let mut config = SessionConfig::new();

        config.set_value("behavior.max_input_lines", "15").unwrap();
        assert_eq!(config.behavior.max_input_lines, 15);

        assert!(config.set_value("behavior.max_input_lines", "not_a_number").is_err());
    }

    #[test]
    fn test_config_value_getting() {
        let config = SessionConfig::new();
        assert_eq!(config.get_value("behavior.max_input_lines"), Some("10".to_owned()));
    }

    #[test]
    fn test_config_validation() {
        let config = SessionConfig::new();
        config.validate().unwrap();

        // Test invalid history size
        let mut invalid_config = config.clone();
        invalid_config.behavior.history_size = 0;
        assert!(invalid_config.validate().is_err());

        // Test invalid cache size
        let mut invalid_config2 = config.clone();
        invalid_config2.performance.lru_cache_size = 0;
        assert!(invalid_config2.validate().is_err());
    }

    #[test]
    fn test_config_with_custom_values() {
        let mut config = SessionConfig::new();

        // Test setting custom values
        config.behavior.max_input_lines = 20;
        config.performance.lru_cache_size = 256;

        assert_eq!(config.behavior.max_input_lines, 20);
        assert_eq!(config.performance.lru_cache_size, 256);
    }
}
