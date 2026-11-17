//! Keyboard-protocol flag mapping tests (moved with the TUI runtime).

use vtcode_core::config::{KeyboardProtocolConfig, KeyboardProtocolMode};

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

    let mut flags = match config.mode {
        KeyboardProtocolMode::Default => {
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        }
        KeyboardProtocolMode::Full => {
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        }
        KeyboardProtocolMode::Minimal => KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        KeyboardProtocolMode::Custom => {
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
    };

    if should_force_report_all_keys(config.mode, is_macos, term_program, term) {
        flags |= KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
    }

    flags
}

fn should_force_report_all_keys(
    mode: KeyboardProtocolMode,
    is_macos: bool,
    term_program: Option<&str>,
    term: Option<&str>,
) -> bool {
    if !is_macos || mode != KeyboardProtocolMode::Default {
        return false;
    }

    // Ghostty on macOS needs "report all keys" enabled so bare Command presses
    // surface as modifier-key events that transcript link clicks can merge in.
    vtcode_commons::terminal_detection::is_ghostty_terminal(term_program, term)
}

mod keyboard_protocol_tests {
    use super::*;
    use ratatui::crossterm::event::KeyboardEnhancementFlags;

    fn default_keyboard_protocol_config() -> KeyboardProtocolConfig {
        KeyboardProtocolConfig {
            enabled: true,
            mode: KeyboardProtocolMode::Default,
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        }
    }

    #[test]
    fn test_keyboard_protocol_defaults() {
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
    fn test_keyboard_protocol_defaults_enable_all_keys_for_ghostty_on_macos() {
        let flags = keyboard_protocol_to_flags_for_terminal(
            &default_keyboard_protocol_config(),
            true,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn test_keyboard_protocol_minimal_mode() {
        let config = KeyboardProtocolConfig {
            enabled: true,
            mode: KeyboardProtocolMode::Minimal,
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let flags = keyboard_protocol_to_flags_for_terminal(&config, true, Some("Ghostty"), Some("xterm-ghostty"));

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn test_keyboard_protocol_disabled() {
        let config = KeyboardProtocolConfig {
            enabled: false,
            mode: KeyboardProtocolMode::Default,
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let flags = keyboard_protocol_to_flags_for_terminal(&config, true, Some("Ghostty"), Some("xterm-ghostty"));
        assert!(flags.is_empty());
    }

    #[test]
    fn test_keyboard_protocol_custom_mode() {
        let config = KeyboardProtocolConfig {
            enabled: true,
            mode: KeyboardProtocolMode::Custom,
            disambiguate_escape_codes: true,
            report_event_types: false,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let flags = keyboard_protocol_to_flags_for_terminal(&config, true, Some("Ghostty"), Some("xterm-ghostty"));

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn test_keyboard_protocol_validation() {
        let config = KeyboardProtocolConfig {
            enabled: true,
            mode: KeyboardProtocolMode::Default,
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        // All enum variants are valid; validation always succeeds.
        config.validate().unwrap();
    }
}
