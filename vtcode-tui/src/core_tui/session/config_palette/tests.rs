use crate::config::loader::ConfigManager;

use super::*;

fn setup_palette() -> ConfigPalette {
    let temp_dir = std::env::temp_dir();
    let manager =
        ConfigManager::load_from_workspace(temp_dir).expect("Failed to create test config manager");
    ConfigPalette::new(manager)
}

#[test]
fn test_initialization() {
    let palette = setup_palette();
    assert!(
        !palette.items.is_empty(),
        "Palette should have items loaded"
    );
    assert_eq!(
        palette.selected(),
        Some(0),
        "First item should be selected by default"
    );
    assert!(!palette.modified, "Modified flag should be false initially");
}

#[test]
fn test_navigation() {
    let mut palette = setup_palette();
    let item_count = palette.items.len();

    // Test Down navigation
    palette.list_state.select(Some(0));
    palette.move_down();
    assert_eq!(palette.selected(), Some(1), "Should move down to index 1");

    // Test Wrap around Down
    palette.list_state.select(Some(item_count - 1));
    palette.move_down();
    assert_eq!(palette.selected(), Some(0), "Should wrap around to 0");

    // Test Up navigation
    palette.list_state.select(Some(1));
    palette.move_up();
    assert_eq!(palette.selected(), Some(0), "Should move up to 0");

    // Test Wrap around Up
    palette.list_state.select(Some(0));
    palette.move_up();
    assert_eq!(
        palette.selected(),
        Some(item_count - 1),
        "Should wrap around to last item"
    );
}

#[test]
fn test_toggle_bool() {
    let mut palette = setup_palette();

    // Find a boolean item index (e.g., ui.allow_tool_ansi)
    let index = palette
        .items
        .iter()
        .position(|i| i.key == "ui.allow_tool_ansi");
    assert!(index.is_some(), "Should have ui.allow_tool_ansi item");
    let index = index.unwrap();

    palette.list_state.select(Some(index));

    let initial_value = palette.config.ui.allow_tool_ansi;

    palette.toggle_selected();

    assert_ne!(
        palette.config.ui.allow_tool_ansi, initial_value,
        "Value should create toggled"
    );
    assert!(palette.modified, "Modified flag should be true");

    // Toggle back
    palette.toggle_selected();
    assert_eq!(
        palette.config.ui.allow_tool_ansi, initial_value,
        "Value should default back"
    );
}

#[test]
fn test_toggle_show_turn_timer() {
    let mut palette = setup_palette();
    let index = palette
        .items
        .iter()
        .position(|i| i.key == "ui.show_turn_timer")
        .expect("Should have ui.show_turn_timer item");

    palette.list_state.select(Some(index));
    let initial_value = palette.config.ui.show_turn_timer;

    palette.toggle_selected();
    assert_ne!(
        palette.config.ui.show_turn_timer, initial_value,
        "Show turn timer should toggle"
    );
    assert!(palette.modified, "Modified flag should be true");
}

#[test]
fn test_cycle_enum() {
    let mut palette = setup_palette();

    // Find enum item (e.g., ui.tool_output_mode)
    let index = palette
        .items
        .iter()
        .position(|i| i.key == "ui.tool_output_mode");
    if let Some(idx) = index {
        palette.list_state.select(Some(idx));
        let initial = palette.config.ui.tool_output_mode.clone();

        palette.toggle_selected();

        assert_ne!(
            palette.config.ui.tool_output_mode, initial,
            "Enum should cycle"
        );
        assert!(palette.modified, "Modified flag should be true");
    }
}
