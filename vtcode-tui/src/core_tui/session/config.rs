//! Configuration system for TUI session UI preferences
//!
//! Contains settings for customizable UI elements, colors, key bindings, and other preferences.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Main configuration struct for TUI session preferences
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionConfig {
    /// UI appearance settings
    pub appearance: AppearanceConfig,

    /// Key binding preferences
    pub key_bindings: KeyBindingConfig,

    /// Behavior preferences
    pub behavior: BehaviorConfig,

    /// Performance related settings
    pub performance: PerformanceConfig,

    /// Customization settings
    pub customization: CustomizationConfig,
}

/// UI mode variants for quick presets
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UiMode {
    /// Full UI with all features (sidebar, footer)
    #[default]
    Full,
    /// Minimal UI - no sidebar, no footer
    Minimal,
    /// Focused mode - transcript only, maximum content space
    Focused,
}

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

    /// Transcript bottom padding
    pub transcript_bottom_padding: u16,

    /// Whether to dim completed todo items (- [x] and ~~strikethrough~~)
    pub dim_completed_todos: bool,

    /// Number of blank lines between message blocks (0-2)
    pub message_block_spacing: u8,

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
            message_block_spacing: 0,
            customization: CustomizationConfig::default(),
        }
    }
}

impl AppearanceConfig {
    /// Create AppearanceConfig from VTCodeConfig
    pub fn from_config(config: &crate::config::loader::VTCodeConfig) -> Self {
        Self {
            theme: config.agent.theme.clone(),
            ui_mode: match config.ui.display_mode {
                crate::config::UiDisplayMode::Full => UiMode::Full,
                crate::config::UiDisplayMode::Minimal => UiMode::Minimal,
                crate::config::UiDisplayMode::Focused => UiMode::Focused,
            },
            show_sidebar: config.ui.show_sidebar,
            min_content_width: 40,
            min_navigation_width: 20,
            navigation_width_percent: 25,
            transcript_bottom_padding: 0,
            dim_completed_todos: config.ui.dim_completed_todos,
            message_block_spacing: if config.ui.message_block_spacing {
                1
            } else {
                0
            },
            customization: CustomizationConfig::default(),
        }
    }

    /// Check if sidebar should be shown based on ui_mode and show_sidebar
    pub fn should_show_sidebar(&self) -> bool {
        match self.ui_mode {
            UiMode::Full => self.show_sidebar,
            UiMode::Minimal | UiMode::Focused => false,
        }
    }

    /// Check if footer should be shown based on ui_mode
    #[allow(dead_code)]
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
    pub bindings: HashMap<String, Vec<String>>,
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
    pub max_input_lines: usize,

    /// Whether to enable command history
    pub enable_history: bool,

    /// History size limit
    pub history_size: usize,

    /// Whether to enable double-tap escape to clear input
    pub double_tap_escape_clears: bool,

    /// Delay in milliseconds for double-tap detection
    pub double_tap_delay_ms: u64,

    /// Whether to auto-scroll to bottom
    pub auto_scroll_to_bottom: bool,

    /// Whether to show queued inputs
    pub show_queued_inputs: bool,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            max_input_lines: 10,
            enable_history: true,
            history_size: 100,
            double_tap_escape_clears: true,
            double_tap_delay_ms: 300,
            auto_scroll_to_bottom: true,
            show_queued_inputs: true,
        }
    }
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Cache size for rendered elements
    pub render_cache_size: usize,

    /// Transcript cache size (number of messages to cache)
    pub transcript_cache_size: usize,

    /// Whether to enable transcript reflow caching
    pub enable_transcript_caching: bool,

    /// Size of LRU cache for expensive operations
    pub lru_cache_size: usize,

    /// Whether to enable smooth scrolling
    pub enable_smooth_scrolling: bool,
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
    pub ui_labels: HashMap<String, String>,

    /// Custom styling options
    pub custom_styles: HashMap<String, String>,

    /// Enabled UI features
    pub enabled_features: Vec<String>,
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
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads configuration from a file
    #[allow(dead_code)]
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = crate::utils::file_utils::read_file_with_context_sync(
            Path::new(path),
            "session config file",
        )
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(std::io::Error::other(err)) })?;
        let config: SessionConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Saves configuration to a file
    #[allow(dead_code)]
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        crate::utils::file_utils::write_file_with_context_sync(
            Path::new(path),
            &content,
            "session config file",
        )
        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(std::io::Error::other(err)) })?;
        Ok(())
    }

    /// Updates a specific configuration value by key
    #[allow(dead_code)]
    pub fn set_value(&mut self, key: &str, value: &str) -> Result<(), String> {
        // This is a simplified version - in a real implementation, we'd have more sophisticated
        // parsing and validation for different configuration types
        match key {
            "behavior.max_input_lines" => {
                self.behavior.max_input_lines = value
                    .parse()
                    .map_err(|_| format!("Cannot parse '{}' as number", value))?;
            }
            "performance.lru_cache_size" => {
                self.performance.lru_cache_size = value
                    .parse()
                    .map_err(|_| format!("Cannot parse '{}' as number", value))?;
            }
            _ => return Err(format!("Unknown configuration key: {}", key)),
        }
        Ok(())
    }

    /// Gets a configuration value by key
    #[allow(dead_code)]
    pub fn get_value(&self, key: &str) -> Option<String> {
        match key {
            "behavior.max_input_lines" => Some(self.behavior.max_input_lines.to_string()),
            "performance.lru_cache_size" => Some(self.performance.lru_cache_size.to_string()),
            _ => None,
        }
    }

    /// Validates the configuration to ensure all values are within acceptable ranges
    #[allow(dead_code)]
    pub fn validate(&self) -> Result<(), Vec<String>> {
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

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SessionConfig::new();
        assert_eq!(config.behavior.history_size, 100);
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

        assert!(
            config
                .set_value("behavior.max_input_lines", "not_a_number")
                .is_err()
        );
    }

    #[test]
    fn test_config_value_getting() {
        let config = SessionConfig::new();
        assert_eq!(
            config.get_value("behavior.max_input_lines"),
            Some("10".to_owned())
        );
    }

    #[test]
    fn test_config_validation() {
        let config = SessionConfig::new();
        assert!(config.validate().is_ok());

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
