//! Modal search and secure-prompt input widgets.

use super::super::state::ModalSearchState;
use crate::tui::ui::tui::types::SecurePromptConfig;
use ratatui::prelude::*;
use ratatui_cheese::input::{Input, InputState, InputStyles};

pub(super) fn render_modal_search(
    frame: &mut Frame<'_>,
    area: Rect,
    search: &ModalSearchState,
    input_styles: &InputStyles,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let input_widget = Input::new(search.label.as_str())
        .placeholder(search.placeholder.as_deref().unwrap_or("Type to filter..."))
        .prompt(">")
        .styles(input_styles.clone());

    let mut input_state = InputState::new();
    input_state.set_value(search.query.clone());
    input_state.set_focused(true);
    for _ in 0..search.query.chars().count() {
        input_state.move_right();
    }

    frame.render_stateful_widget(&input_widget, area, &mut input_state);
}

pub(super) fn render_secure_prompt(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &SecurePromptConfig,
    input: &str,
    cursor: usize,
    input_styles: &InputStyles,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let input_widget = Input::new(config.label.as_str())
        .placeholder(config.placeholder.as_deref().unwrap_or("Enter value..."))
        .password_mode(config.mask_input)
        .password_char('\u{2022}')
        .styles(input_styles.clone());

    let mut input_state = InputState::new();
    input_state.set_value(input.to_owned());
    input_state.set_focused(true);
    let char_count = input.chars().count();
    let target = cursor.min(char_count);
    for _ in 0..target {
        input_state.move_right();
    }

    frame.render_stateful_widget(&input_widget, area, &mut input_state);
}
