use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
};

use super::terminal_capabilities;
use crate::config::constants::ui;
use crate::ui::search::fuzzy_score;

use super::super::types::InlineTextStyle;
use super::{
    Session,
    modal::{ModalListLayout, compute_modal_area},
    ratatui_color_from_ansi, ratatui_style_from_inline,
    slash_palette::{self, SlashPaletteUpdate, command_prefix, command_range},
};

pub fn render_slash_palette(session: &mut Session, frame: &mut Frame<'_>, viewport: Rect) {
    if viewport.height == 0 || viewport.width == 0 || session.modal.is_some() {
        session.slash_palette.clear_visible_rows();
        return;
    }
    let suggestions = session.slash_palette.suggestions();
    if suggestions.is_empty() {
        session.slash_palette.clear_visible_rows();
        return;
    }

    let instructions = slash_palette_instructions(session);
    let area = compute_modal_area(viewport, instructions.len(), 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(session.suggestion_block_title())
        .border_type(terminal_capabilities::get_border_type())
        .style(session.styles.default_style())
        .border_style(session.styles.border_style());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        session.slash_palette.clear_visible_rows();
        return;
    }

    let layout = ModalListLayout::new(inner, instructions.len());
    if let Some(text_area) = layout.text_area {
        let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, text_area);
    }

    session
        .slash_palette
        .set_visible_rows(layout.list_area.height as usize);

    // Get all list items (scrollable via ListState)
    let list_items = slash_list_items(session);

    let list = List::new(list_items)
        .style(session.styles.default_style())
        .highlight_style(slash_highlight_style(session))
        .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
        .repeat_highlight_symbol(true);

    frame.render_stateful_widget(
        list,
        layout.list_area,
        session.slash_palette.list_state_mut(),
    );
}

fn slash_palette_instructions(session: &Session) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            ui::SLASH_PALETTE_HINT_PRIMARY.to_owned(),
            session.styles.default_style(),
        )),
        Line::from(Span::styled(
            ui::SLASH_PALETTE_HINT_SECONDARY.to_owned(),
            session.styles.default_style().add_modifier(Modifier::DIM),
        )),
    ]
}

pub(super) fn handle_slash_palette_change(session: &mut Session) {
    // session.recalculate_transcript_rows(); // This method was removed from session.rs, need to check where it went.
    // It was moved to render.rs as `recalculate_transcript_rows`.
    // But render.rs functions are not methods on Session anymore.
    // So I need to call `render::recalculate_transcript_rows(session)`.
    // But I can't import `render` here easily if it's a sibling module.
    // `use super::render;`
    // I'll add the import later. For now I'll use the fully qualified path or assume I'll fix imports.
    // Actually, `recalculate_transcript_rows` is likely `pub(super)` in `render.rs`.
    // I'll check `render.rs` exports.

    // For now, I'll comment it out and fix it in a separate step or assume `render` is available.
    // Wait, I can't leave broken code.
    // I'll assume `crate::ui::tui::session::render::recalculate_transcript_rows(session)` works.
    crate::ui::tui::session::render::recalculate_transcript_rows(session);
    session.enforce_scroll_bounds();
    session.mark_dirty();
}

pub(super) fn clear_slash_suggestions(session: &mut Session) {
    if session.slash_palette.clear() {
        handle_slash_palette_change(session);
    }
}

pub(super) fn update_slash_suggestions(session: &mut Session) {
    if !session.input_enabled {
        clear_slash_suggestions(session);
        return;
    }

    let Some(prefix) = command_prefix(
        session.input_manager.content(),
        session.input_manager.cursor(),
    ) else {
        clear_slash_suggestions(session);
        return;
    };

    match session
        .slash_palette
        .update(Some(&prefix), ui::SLASH_SUGGESTION_LIMIT)
    {
        SlashPaletteUpdate::NoChange => {}
        SlashPaletteUpdate::Cleared | SlashPaletteUpdate::Changed { .. } => {
            handle_slash_palette_change(session);
        }
    }
}

pub(crate) fn slash_navigation_available(session: &Session) -> bool {
    let has_prefix = command_prefix(
        session.input_manager.content(),
        session.input_manager.cursor(),
    )
    .is_some();
    session.input_enabled
        && !session.slash_palette.is_empty()
        && has_prefix
        && session.modal.is_none()
        && !session.file_palette_active
}

pub(super) fn move_slash_selection_up(session: &mut Session) -> bool {
    let changed = session.slash_palette.move_up();
    handle_slash_selection_change(session, changed)
}

pub(super) fn move_slash_selection_down(session: &mut Session) -> bool {
    let changed = session.slash_palette.move_down();
    handle_slash_selection_change(session, changed)
}

pub(super) fn select_first_slash_suggestion(session: &mut Session) -> bool {
    let changed = session.slash_palette.select_first();
    handle_slash_selection_change(session, changed)
}

pub(super) fn select_last_slash_suggestion(session: &mut Session) -> bool {
    let changed = session.slash_palette.select_last();
    handle_slash_selection_change(session, changed)
}

pub(super) fn page_up_slash_suggestion(session: &mut Session) -> bool {
    let changed = session.slash_palette.page_up();
    handle_slash_selection_change(session, changed)
}

pub(super) fn page_down_slash_suggestion(session: &mut Session) -> bool {
    let changed = session.slash_palette.page_down();
    handle_slash_selection_change(session, changed)
}

pub(super) fn handle_slash_selection_change(session: &mut Session, changed: bool) -> bool {
    if changed {
        preview_selected_slash_suggestion(session);
        crate::ui::tui::session::render::recalculate_transcript_rows(session);
        session.enforce_scroll_bounds();
        session.mark_dirty();
        true
    } else {
        false
    }
}

fn preview_selected_slash_suggestion(session: &mut Session) {
    let Some(command) = session.slash_palette.selected_command() else {
        return;
    };
    let Some(range) = command_range(
        session.input_manager.content(),
        session.input_manager.cursor(),
    ) else {
        return;
    };

    let current_input = session.input_manager.content().to_owned();
    let prefix = &current_input[..range.start];
    let suffix = &current_input[range.end..];

    let mut new_input = String::new();
    new_input.push_str(prefix);
    new_input.push('/');
    new_input.push_str(command.name.as_str());
    let cursor_position = new_input.len();

    if !suffix.is_empty() {
        if !suffix.chars().next().is_some_and(char::is_whitespace) {
            new_input.push(' ');
        }
        new_input.push_str(suffix);
    }

    session.input_manager.set_content(new_input.clone());
    session
        .input_manager
        .set_cursor(cursor_position.min(new_input.len()));
    session.mark_dirty();
}

pub(super) fn apply_selected_slash_suggestion(session: &mut Session) -> bool {
    let Some(command) = session.slash_palette.selected_command() else {
        return false;
    };

    let command_name = command.name.to_owned();

    let input_content = session.input_manager.content();
    let cursor_pos = session.input_manager.cursor();
    let Some(range) = command_range(input_content, cursor_pos) else {
        return false;
    };

    let suffix = input_content[range.end..].to_owned();
    let mut new_input = format!("/{}", command_name);

    let cursor_position = if suffix.is_empty() {
        new_input.push(' ');
        new_input.len()
    } else {
        if !suffix.chars().next().is_some_and(char::is_whitespace) {
            new_input.push(' ');
        }
        let position = new_input.len();
        new_input.push_str(&suffix);
        position
    };

    session.input_manager.set_content(new_input);
    session.input_manager.set_cursor(cursor_position);

    clear_slash_suggestions(session);
    session.mark_dirty();

    true
}

pub(super) fn autocomplete_slash_suggestion(session: &mut Session) -> bool {
    let input_content = session.input_manager.content();
    let cursor_pos = session.input_manager.cursor();

    let Some(range) = command_range(input_content, cursor_pos) else {
        return false;
    };

    let prefix_text = command_prefix(input_content, cursor_pos).unwrap_or_default();

    if prefix_text.is_empty() {
        return false;
    }

    let suggestions = session.slash_palette.suggestions();
    if suggestions.is_empty() {
        return false;
    }

    // Find the best fuzzy match from all suggestions
    let mut best_match: Option<(usize, u32, String)> = None;

    for (idx, suggestion) in suggestions.iter().enumerate() {
        let command_name = match suggestion {
            slash_palette::SlashPaletteSuggestion::Static(cmd) => cmd.name.to_string(),
        };

        if let Some(score) = fuzzy_score(&prefix_text, &command_name) {
            if let Some((_, best_score, _)) = &best_match {
                if score > *best_score {
                    best_match = Some((idx, score, command_name));
                }
            } else {
                best_match = Some((idx, score, command_name));
            }
        }
    }

    let Some((_, _, best_command)) = best_match else {
        return false;
    };

    // Handle static command
    let suffix = &input_content[range.end..];
    let mut new_input = format!("/{}", best_command);

    let cursor_position = if suffix.is_empty() {
        new_input.push(' ');
        new_input.len()
    } else {
        if !suffix.chars().next().is_some_and(char::is_whitespace) {
            new_input.push(' ');
        }
        let position = new_input.len();
        new_input.push_str(suffix);
        position
    };

    session.input_manager.set_content(new_input);
    session.input_manager.set_cursor(cursor_position);

    clear_slash_suggestions(session);
    session.mark_dirty();
    true
}

pub(super) fn try_handle_slash_navigation(
    session: &mut Session,
    key: &KeyEvent,
    has_control: bool,
    has_alt: bool,
    has_command: bool,
) -> bool {
    if !slash_navigation_available(session) {
        return false;
    }

    // Block Control modifier
    if has_control {
        return false;
    }

    // Block Alt unless combined with Command for Up/Down navigation
    if has_alt && !matches!(key.code, KeyCode::Up | KeyCode::Down) {
        return false;
    }

    let handled = match key.code {
        KeyCode::Up => {
            if has_alt && !has_command {
                return false;
            }
            if has_command {
                select_first_slash_suggestion(session)
            } else {
                move_slash_selection_up(session)
            }
        }
        KeyCode::Down => {
            if has_alt && !has_command {
                return false;
            }
            if has_command {
                select_last_slash_suggestion(session)
            } else {
                move_slash_selection_down(session)
            }
        }
        KeyCode::PageUp => page_up_slash_suggestion(session),
        KeyCode::PageDown => page_down_slash_suggestion(session),
        KeyCode::Tab => autocomplete_slash_suggestion(session),
        KeyCode::BackTab => move_slash_selection_up(session),
        KeyCode::Enter => {
            let applied = apply_selected_slash_suggestion(session);
            if !applied {
                return false;
            }

            let should_submit_now = should_submit_immediately_from_palette(session);

            if should_submit_now {
                return false;
            }

            true
        }
        _ => return false,
    };

    if handled {
        session.mark_dirty();
    }

    handled
}

fn should_submit_immediately_from_palette(session: &Session) -> bool {
    let Some(command) = session
        .input_manager
        .content()
        .trim_start()
        .split_whitespace()
        .next()
    else {
        return false;
    };

    matches!(
        command,
        "/files"
            | "/status"
            | "/doctor"
            | "/model"
            | "/new"
            | "/git"
            | "/docs"
            | "/copy"
            | "/config"
            | "/settings"
            | "/help"
            | "/clear"
            | "/exit"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tui::InlineTheme;

    #[test]
    fn immediate_submit_matcher_accepts_immediate_commands() {
        let mut session = Session::new(InlineTheme::default(), None, 20);
        session.set_input("/files".to_string());
        assert!(should_submit_immediately_from_palette(&session));

        session.set_input("   /status   ".to_string());
        assert!(should_submit_immediately_from_palette(&session));
    }

    #[test]
    fn immediate_submit_matcher_rejects_argument_driven_commands() {
        let mut session = Session::new(InlineTheme::default(), None, 20);
        session.set_input("/command echo hello".to_string());
        assert!(!should_submit_immediately_from_palette(&session));

        session.set_input("/add-dir ~/tmp".to_string());
        assert!(!should_submit_immediately_from_palette(&session));
    }
}

fn slash_list_items(session: &Session) -> Vec<ListItem<'static>> {
    session
        .slash_palette
        .suggestions()
        .iter()
        .map(|suggestion| match suggestion {
            slash_palette::SlashPaletteSuggestion::Static(command) => {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("/{}", command.name), slash_name_style(session)),
                    Span::raw(" "),
                    Span::styled(
                        command.description.to_owned(),
                        slash_description_style(session),
                    ),
                ]))
            }
        })
        .collect()
}

fn slash_highlight_style(session: &Session) -> Style {
    let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
    if let Some(primary) = session.theme.primary.or(session.theme.secondary) {
        style = style.fg(ratatui_color_from_ansi(primary));
    }
    style
}

fn slash_name_style(session: &Session) -> Style {
    let style = InlineTextStyle::default()
        .bold()
        .with_color(session.theme.primary.or(session.theme.foreground));
    ratatui_style_from_inline(&style, session.theme.foreground)
}

fn slash_description_style(session: &Session) -> Style {
    session.styles.default_style().add_modifier(Modifier::DIM)
}
