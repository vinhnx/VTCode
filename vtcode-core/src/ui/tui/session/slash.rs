use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use crate::config::constants::ui;

use super::super::types::InlineTextStyle;
use super::{
    LEGACY_PROMPT_COMMAND_NAME, PROMPT_COMMAND_NAME, PROMPT_COMMAND_PREFIX, Session,
    measure_text_width,
    modal::{ModalListLayout, compute_modal_area},
    ratatui_color_from_ansi, ratatui_style_from_inline,
    slash_palette::{self, SlashPaletteUpdate, command_prefix, command_range},
};

impl Session {
    pub(super) fn render_slash_palette(&mut self, frame: &mut Frame<'_>, viewport: Rect) {
        if viewport.height == 0 || viewport.width == 0 || self.modal.is_some() {
            self.slash_palette.clear_visible_rows();
            return;
        }
        let suggestions = self.slash_palette.suggestions();
        if suggestions.is_empty() {
            self.slash_palette.clear_visible_rows();
            return;
        }

        let mut width_hint = measure_text_width(ui::SLASH_PALETTE_HINT_PRIMARY);
        width_hint = width_hint.max(measure_text_width(ui::SLASH_PALETTE_HINT_SECONDARY));
        for suggestion in suggestions.iter().take(ui::SLASH_SUGGESTION_LIMIT) {
            let label = match suggestion {
                slash_palette::SlashPaletteSuggestion::Static(cmd) => {
                    if !cmd.description.is_empty() {
                        format!("/{} {}", cmd.name, cmd.description)
                    } else {
                        format!("/{}", cmd.name)
                    }
                }
                slash_palette::SlashPaletteSuggestion::Custom(prompt) => {
                    // For custom prompts, format as /prompt:name (legacy alias /prompts:name)
                    let prompt_cmd = format!("{}:{}", PROMPT_COMMAND_NAME, prompt.name);
                    let description = prompt.description.as_deref().unwrap_or("");
                    if !description.is_empty() {
                        format!("/{} {}", prompt_cmd, description)
                    } else {
                        format!("/{}", prompt_cmd)
                    }
                }
            };
            width_hint = width_hint.max(measure_text_width(&label));
        }

        let instructions = self.slash_palette_instructions();
        let area = compute_modal_area(viewport, width_hint, instructions.len(), 0, 0, true);

        frame.render_widget(Clear, area);
        let block = Block::default()
            .title(self.suggestion_block_title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 || inner.width == 0 {
            self.slash_palette.clear_visible_rows();
            return;
        }

        let layout = ModalListLayout::new(inner, instructions.len());
        if let Some(text_area) = layout.text_area {
            let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, text_area);
        }

        self.slash_palette
            .set_visible_rows(layout.list_area.height as usize);

        // Get all list items (scrollable via ListState)
        let list_items = self.slash_list_items();

        let list = List::new(list_items)
            .style(self.default_style())
            .highlight_style(self.slash_highlight_style());

        frame.render_stateful_widget(list, layout.list_area, self.slash_palette.list_state_mut());
    }

    fn slash_palette_instructions(&self) -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled(
                ui::SLASH_PALETTE_HINT_PRIMARY.to_string(),
                self.default_style(),
            )),
            Line::from(Span::styled(
                ui::SLASH_PALETTE_HINT_SECONDARY.to_string(),
                self.default_style().add_modifier(Modifier::DIM),
            )),
        ]
    }

    pub(super) fn handle_slash_palette_change(&mut self) {
        self.recalculate_transcript_rows();
        self.enforce_scroll_bounds();
        self.mark_dirty();
    }

    pub(super) fn clear_slash_suggestions(&mut self) {
        if self.slash_palette.clear() {
            self.handle_slash_palette_change();
        }
    }

    pub(super) fn update_slash_suggestions(&mut self) {
        if !self.input_enabled {
            self.clear_slash_suggestions();
            return;
        }

        let Some(prefix) = command_prefix(&self.input, self.cursor) else {
            self.clear_slash_suggestions();
            return;
        };

        // Update slash palette with custom prompts if available
        if let Some(ref custom_prompts) = self.custom_prompts {
            self.slash_palette
                .set_custom_prompts(custom_prompts.clone());
        }

        match self
            .slash_palette
            .update(Some(&prefix), ui::SLASH_SUGGESTION_LIMIT)
        {
            SlashPaletteUpdate::NoChange => {}
            SlashPaletteUpdate::Cleared | SlashPaletteUpdate::Changed { .. } => {
                self.handle_slash_palette_change();
            }
        }
    }

    pub(super) fn slash_navigation_available(&self) -> bool {
        self.input_enabled && !self.slash_palette.is_empty()
    }

    pub(super) fn move_slash_selection_up(&mut self) -> bool {
        let changed = self.slash_palette.move_up();
        self.handle_slash_selection_change(changed)
    }

    pub(super) fn move_slash_selection_down(&mut self) -> bool {
        let changed = self.slash_palette.move_down();
        self.handle_slash_selection_change(changed)
    }

    pub(super) fn select_first_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.select_first();
        self.handle_slash_selection_change(changed)
    }

    pub(super) fn select_last_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.select_last();
        self.handle_slash_selection_change(changed)
    }

    pub(super) fn page_up_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.page_up();
        self.handle_slash_selection_change(changed)
    }

    pub(super) fn page_down_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.page_down();
        self.handle_slash_selection_change(changed)
    }

    pub(super) fn handle_slash_selection_change(&mut self, changed: bool) -> bool {
        if changed {
            self.preview_selected_slash_suggestion();
            self.recalculate_transcript_rows();
            self.enforce_scroll_bounds();
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    fn preview_selected_slash_suggestion(&mut self) {
        let Some(command) = self.slash_palette.selected_command() else {
            return;
        };
        let Some(range) = command_range(&self.input, self.cursor) else {
            return;
        };

        let current_input = self.input.clone();
        let prefix = &current_input[..range.start];
        let suffix = &current_input[range.end..];

        let mut new_input = String::new();
        new_input.push_str(prefix);
        new_input.push('/');
        new_input.push_str(command.name);
        let cursor_position = new_input.len();

        if !suffix.is_empty() {
            if !suffix.chars().next().map_or(false, char::is_whitespace) {
                new_input.push(' ');
            }
            new_input.push_str(suffix);
        }

        self.input = new_input;
        self.cursor = cursor_position.min(self.input.len());
        self.mark_dirty();
    }

    pub(super) fn apply_selected_slash_suggestion(&mut self) -> bool {
        if let Some(custom_prompt) = self.slash_palette.selected_custom_prompt() {
            let Some(range) = command_range(&self.input, self.cursor) else {
                return false;
            };

            let mut new_input = String::from(PROMPT_COMMAND_PREFIX);
            new_input.push_str(&custom_prompt.name);

            let suffix = &self.input[range.end..];
            if !suffix.is_empty() {
                if !suffix.chars().next().map_or(false, char::is_whitespace) {
                    new_input.push(' ');
                }
                new_input.push_str(suffix);
            } else {
                new_input.push(' ');
            }

            let cursor_position = new_input.len();

            self.input = new_input;
            self.cursor = cursor_position;
            self.update_slash_suggestions();
            self.mark_dirty();
            return true;
        }

        let Some(command) = self.slash_palette.selected_command() else {
            return false;
        };

        let command_name = command.name.to_string();

        let Some(range) = command_range(&self.input, self.cursor) else {
            return false;
        };

        let suffix = self.input[range.end..].to_string();
        let mut new_input = format!("/{}", command_name);

        let cursor_position = if suffix.is_empty() {
            new_input.push(' ');
            new_input.len()
        } else {
            if !suffix.chars().next().map_or(false, char::is_whitespace) {
                new_input.push(' ');
            }
            let position = new_input.len();
            new_input.push_str(&suffix);
            position
        };

        self.input = new_input;
        self.cursor = cursor_position;

        if command_name == "files" {
            self.clear_slash_suggestions();
            self.mark_dirty();
            self.deferred_file_browser_trigger = true;
        } else if command_name == PROMPT_COMMAND_NAME || command_name == LEGACY_PROMPT_COMMAND_NAME
        {
            self.clear_slash_suggestions();
            self.mark_dirty();
            self.deferred_prompt_browser_trigger = true;
        } else {
            self.update_slash_suggestions();
            self.mark_dirty();
        }

        true
    }

    pub(super) fn try_handle_slash_navigation(
        &mut self,
        key: &KeyEvent,
        has_control: bool,
        has_alt: bool,
        has_command: bool,
    ) -> bool {
        if !self.slash_navigation_available() || has_control || has_alt {
            return false;
        }

        let handled = match key.code {
            KeyCode::Up => {
                if has_command {
                    self.select_first_slash_suggestion()
                } else {
                    self.move_slash_selection_up()
                }
            }
            KeyCode::Down => {
                if has_command {
                    self.select_last_slash_suggestion()
                } else {
                    self.move_slash_selection_down()
                }
            }
            KeyCode::PageUp => self.page_up_slash_suggestion(),
            KeyCode::PageDown => self.page_down_slash_suggestion(),
            KeyCode::Tab => self.move_slash_selection_down(),
            KeyCode::BackTab => self.move_slash_selection_up(),
            KeyCode::Enter => self.apply_selected_slash_suggestion(),
            _ => return false,
        };

        if handled {
            self.mark_dirty();
        }

        handled
    }

    fn slash_list_items(&self) -> Vec<ListItem<'static>> {
        self.slash_palette
            .suggestions()
            .iter()
            .map(|suggestion| match suggestion {
                slash_palette::SlashPaletteSuggestion::Static(command) => {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("/{}", command.name), self.slash_name_style()),
                        Span::raw(" "),
                        Span::styled(command.description.to_string(), self.slash_description_style()),
                    ]))
                }
                slash_palette::SlashPaletteSuggestion::Custom(prompt) => {
                    let display_name = format!("/{}:{}", PROMPT_COMMAND_NAME, prompt.name);
                    let description = prompt.description.clone().unwrap_or_default();
                    ListItem::new(Line::from(vec![
                        Span::styled(display_name, self.slash_name_style()),
                        Span::raw(" "),
                        Span::styled(description, self.slash_description_style()),
                    ]))
                }
            })
            .collect()
    }



    fn slash_highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.secondary) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    fn slash_name_style(&self) -> Style {
        let mut style = InlineTextStyle::default();
        style.bold = true;
        style.color = self.theme.primary.or(self.theme.foreground);
        ratatui_style_from_inline(&style, self.theme.foreground)
    }

    fn slash_description_style(&self) -> Style {
        self.default_style().add_modifier(Modifier::DIM)
    }
}
