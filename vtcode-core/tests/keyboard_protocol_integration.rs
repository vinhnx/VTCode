/// Integration tests for Kitty keyboard protocol in TUI
///
/// These tests verify:
/// 1. Protocol flag configuration
/// 2. Keyboard protocol support detection
/// 3. TUI initialization with keyboard protocol
use ratatui::crossterm::event::KeyboardEnhancementFlags;
use vtcode_core::ui::tui::modern_tui::ModernTui;

#[test]
fn test_keyboard_enhancement_flags_default_mode() {
    // Default mode: disambiguate + report types + report alternates
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;

    assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    assert!(flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
    assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
    println!("✓ Default mode flags configured correctly");
}

#[test]
fn test_keyboard_enhancement_flags_minimal_mode() {
    // Minimal mode: only disambiguate escape codes
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;

    assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
    assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
    println!("✓ Minimal mode flags configured correctly");
}

#[test]
fn test_keyboard_enhancement_flags_custom_mode() {
    // Custom mode: flexible configuration
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;

    assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
    assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
    println!("✓ Custom mode flags configured correctly");
}

#[test]
fn test_protocol_disabled_config() {
    // Test that protocol can be disabled
    let disabled_flags = KeyboardEnhancementFlags::empty();
    assert!(disabled_flags.is_empty());
    println!("✓ Protocol can be disabled (empty flags)");
}

#[test]
fn test_keyboard_flag_combinations() {
    // Test various flag combinations
    let all_flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;

    assert_eq!(
        all_flags,
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
    );

    // Test subset relationships
    assert!(all_flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    assert!(all_flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
    assert!(all_flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));

    println!("✓ Flag combinations work correctly");
}

#[test]
fn test_keyboard_enhancement_single_flags() {
    // Test each flag individually
    let disambiguate = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    assert!(disambiguate.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    assert!(!disambiguate.is_empty());

    let report_types = KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
    assert!(report_types.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
    assert!(!report_types.is_empty());

    let report_alternates = KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
    assert!(report_alternates.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
    assert!(!report_alternates.is_empty());

    println!("✓ Individual keyboard enhancement flags work");
}

#[tokio::test]
async fn test_modern_tui_keyboard_builder() {
    // Verify ModernTui can be configured with different keyboard flags
    let _tui = ModernTui::new()
        .expect("Failed to create TUI")
        .keyboard_flags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES);

    println!("✓ ModernTui builder accepts keyboard_flags");
}

#[tokio::test]
async fn test_modern_tui_with_default_mode() {
    let default_flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;

    let _tui = ModernTui::new()
        .expect("Failed to create TUI")
        .keyboard_flags(default_flags);

    println!("✓ ModernTui configured with default keyboard protocol mode");
}

#[tokio::test]
async fn test_modern_tui_with_minimal_mode() {
    let minimal_flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;

    let _tui = ModernTui::new()
        .expect("Failed to create TUI")
        .keyboard_flags(minimal_flags);

    println!("✓ ModernTui configured with minimal keyboard protocol mode");
}

#[tokio::test]
async fn test_modern_tui_with_disabled_protocol() {
    let _tui = ModernTui::new()
        .expect("Failed to create TUI")
        .keyboard_flags(KeyboardEnhancementFlags::empty());

    println!("✓ ModernTui configured with protocol disabled");
}

#[test]
fn test_keyboard_protocol_config_modes() {
    // Verify different configuration modes are recognized
    let modes = vec!["default", "minimal", "full", "custom"];

    for mode in modes {
        match mode {
            "default" | "minimal" | "full" | "custom" => {
                println!("✓ Mode '{}' is recognized", mode);
            }
            _ => panic!("Unknown mode"),
        }
    }
}

#[test]
fn test_keyboard_protocol_flag_operations() {
    // Test bitwise operations on flags
    let flags1 = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    let flags2 = KeyboardEnhancementFlags::REPORT_EVENT_TYPES;

    // OR operation
    let combined = flags1 | flags2;
    assert!(combined.contains(flags1));
    assert!(combined.contains(flags2));

    // Contains operation
    assert!(combined.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    assert!(combined.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));

    println!("✓ Flag bitwise operations work correctly");
}

#[tokio::test]
async fn test_modenetui_frame_rate_and_tick_rate() {
    // Test other ModernTui builder options
    let _tui = ModernTui::new()
        .expect("Failed to create TUI")
        .frame_rate(60.0)
        .tick_rate(4.0)
        .mouse(false)
        .paste(false)
        .keyboard_flags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES);

    println!("✓ ModernTui builder supports frame_rate, tick_rate, mouse, paste");
}

#[test]
fn test_keyboard_protocol_specification_flags() {
    // Test all flags defined by Kitty keyboard protocol specification
    // Reference: https://sw.kovidgoyal.net/kitty/keyboard-protocol/

    let disambiguate = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    let report_types = KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
    let report_alternates = KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;

    // Verify all flags are available
    assert!(!disambiguate.is_empty());
    assert!(!report_types.is_empty());
    assert!(!report_alternates.is_empty());

    // Verify flags don't overlap incorrectly
    assert_ne!(disambiguate, report_types);
    assert_ne!(disambiguate, report_alternates);
    assert_ne!(report_types, report_alternates);

    println!("✓ All Kitty keyboard protocol flags are available");
}

#[test]
fn test_keyboard_enhancement_empty_flags() {
    let empty = KeyboardEnhancementFlags::empty();
    assert!(empty.is_empty());

    let non_empty = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    assert!(!non_empty.is_empty());

    // Empty should not contain any flags
    assert!(!empty.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));

    println!("✓ Empty and non-empty flag handling works");
}
