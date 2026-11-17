/// TUI configuration constants split into sub-modules for defaults and UI tuning.
pub mod constants {
    pub mod defaults;
    pub mod ui;
}

/// Configuration type definitions re-exported for convenience.
pub mod types;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use vtcode_commons::terminal_detection::is_ghostty_terminal;

pub use types::{ReasoningEffortLevel, SystemPromptMode, ToolDocumentationMode, UiSurfacePreference, VerbosityLevel};

/// Configuration for the terminal keyboard protocol (Kitty progressive enhancement).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyboardProtocolConfig {
    pub(crate) enabled: bool,
    pub(crate) mode: String,
    pub(crate) disambiguate_escape_codes: bool,
    pub(crate) report_event_types: bool,
    pub(crate) report_alternate_keys: bool,
    pub(crate) report_all_keys: bool,
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
    /// Validate the keyboard protocol configuration.
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

/// Convert KeyboardProtocolConfig to crossterm keyboard enhancement flags.
pub fn keyboard_protocol_to_flags(config: &KeyboardProtocolConfig) -> crossterm::event::KeyboardEnhancementFlags {
    keyboard_protocol_to_flags_for_terminal(
        config,
        cfg!(target_os = "macos"),
        std::env::var("TERM_PROGRAM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    )
}

fn keyboard_protocol_to_flags_for_terminal(
    config: &KeyboardProtocolConfig,
    is_macos: bool,
    term_program: Option<&str>,
    term: Option<&str>,
) -> crossterm::event::KeyboardEnhancementFlags {
    use ratatui::crossterm::event::KeyboardEnhancementFlags;

    if !config.enabled {
        return KeyboardEnhancementFlags::empty();
    }

    let mut flags = match config.mode.as_str() {
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
    };

    if should_force_report_all_keys(config.mode.as_str(), is_macos, term_program, term) {
        flags |= KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
    }

    flags
}

fn should_force_report_all_keys(mode: &str, is_macos: bool, term_program: Option<&str>, term: Option<&str>) -> bool {
    if !is_macos || !matches!(mode, "default") {
        return false;
    }

    // Ghostty on macOS needs "report all keys" enabled so bare Command presses
    // surface as modifier-key events that transcript link clicks can merge in.
    is_ghostty_terminal(term_program, term)
}

#[cfg(test)]
mod keyboard_protocol_tests {
    use super::*;
    use ratatui::crossterm::event::KeyboardEnhancementFlags;

    fn default_keyboard_protocol_config() -> KeyboardProtocolConfig {
        KeyboardProtocolConfig {
            enabled: true,
            mode: "default".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        }
    }

    #[test]
    fn keyboard_protocol_defaults_keep_standard_flags() {
        let flags = keyboard_protocol_to_flags_for_terminal(
            &default_keyboard_protocol_config(),
            false,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn keyboard_protocol_defaults_enable_all_keys_for_ghostty_on_macos() {
        let flags = keyboard_protocol_to_flags_for_terminal(
            &default_keyboard_protocol_config(),
            true,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn keyboard_protocol_custom_mode_respects_explicit_report_all_keys_setting() {
        let flags = keyboard_protocol_to_flags_for_terminal(
            &KeyboardProtocolConfig {
                enabled: true,
                mode: "custom".to_string(),
                disambiguate_escape_codes: true,
                report_event_types: true,
                report_alternate_keys: true,
                report_all_keys: false,
            },
            true,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }
}
