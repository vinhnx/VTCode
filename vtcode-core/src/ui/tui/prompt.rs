use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use termion::event::Key;
use unicode_width::UnicodeWidthStr;

use crate::ui::tui::{
    action::{Action, ScrollAction},
    types::{RatatuiTextStyle, RatatuiTheme},
};

const DEFAULT_PROMPT_PREFIX: &str = "> ";

#[derive(Default)]
struct InputBuffer {
    text: String,
    cursor: usize,
}

impl InputBuffer {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    fn insert(&mut self, ch: char) {
        let mut encoded = [0u8; 4];
        let slice = ch.encode_utf8(&mut encoded);
        self.text.insert_str(self.cursor, slice);
        self.cursor += slice.len();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.text.drain(start..self.cursor);
        self.cursor = start;
    }

    fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let end = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.cursor + index)
            .unwrap_or_else(|| self.text.len());
        self.text.drain(self.cursor..end);
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let new_cursor = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.cursor = new_cursor;
    }

    fn move_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let new_cursor = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.cursor + index)
            .unwrap_or_else(|| self.text.len());
        self.cursor = new_cursor;
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    fn prefix(&self) -> &str {
        &self.text[..self.cursor]
    }
}

pub struct PromptBar {
    input: InputBuffer,
    prompt_prefix: String,
    prompt_style: RatatuiTextStyle,
    placeholder_hint: Option<String>,
    placeholder_style: RatatuiTextStyle,
    theme: RatatuiTheme,
    cursor_visible: bool,
    input_enabled: bool,
}

impl PromptBar {
    pub fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Self {
        Self {
            input: InputBuffer::default(),
            prompt_prefix: DEFAULT_PROMPT_PREFIX.to_string(),
            prompt_style: RatatuiTextStyle::default(),
            placeholder_hint: placeholder,
            placeholder_style: RatatuiTextStyle::default(),
            theme,
            cursor_visible: true,
            input_enabled: true,
        }
    }

    pub fn set_theme(&mut self, theme: RatatuiTheme) {
        self.theme = theme;
    }

    pub fn set_prompt(&mut self, prefix: String, style: RatatuiTextStyle) {
        self.prompt_prefix = prefix;
        self.prompt_style = style;
    }

    pub fn set_placeholder(&mut self, hint: Option<String>, style: Option<RatatuiTextStyle>) {
        self.placeholder_hint = hint;
        if let Some(style) = style {
            self.placeholder_style = style;
        }
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    pub fn set_input_enabled(&mut self, enabled: bool) {
        self.input_enabled = enabled;
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
    }

    pub fn handle_key(&mut self, key: Key) -> Action {
        if !self.input_enabled {
            return Action::None;
        }

        match key {
            Key::Ctrl('c') | Key::Ctrl('C') => Action::Interrupt,
            Key::Ctrl('d') | Key::Ctrl('D') => Action::Exit,
            Key::Ctrl('u') | Key::Ctrl('U') => {
                self.clear_input();
                Action::Redraw
            }
            Key::Char('\n') | Key::Ctrl('m') => {
                let text = self.input.text.clone();
                self.clear_input();
                Action::Submit(text)
            }
            Key::Char('\t') => {
                self.input.insert('\t');
                Action::Redraw
            }
            Key::Char(ch) => {
                self.input.insert(ch);
                Action::Redraw
            }
            Key::Backspace | Key::Ctrl('h') => {
                self.input.backspace();
                Action::Redraw
            }
            Key::Delete => {
                self.input.delete();
                Action::Redraw
            }
            Key::Left => {
                self.input.move_left();
                Action::Redraw
            }
            Key::Right => {
                self.input.move_right();
                Action::Redraw
            }
            Key::Home => {
                self.input.move_home();
                Action::Redraw
            }
            Key::End => {
                self.input.move_end();
                Action::Redraw
            }
            Key::Up => Action::Scroll(ScrollAction::LineUp),
            Key::Down => Action::Scroll(ScrollAction::LineDown),
            Key::PageUp => Action::Scroll(ScrollAction::PageUp),
            Key::PageDown => Action::Scroll(ScrollAction::PageDown),
            Key::Esc => Action::Cancel,
            _ => Action::None,
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let mut spans = Vec::new();
        let prefix_style = self
            .prompt_style
            .clone()
            .merge_color(self.theme.primary.or(self.theme.foreground))
            .to_style(self.theme.foreground);
        spans.push(Span::styled(self.prompt_prefix.clone(), prefix_style));

        if !self.input.is_empty() {
            spans.push(Span::raw(self.input.text.clone()));
        } else if let Some(hint) = &self.placeholder_hint {
            let style = self
                .placeholder_style
                .clone()
                .merge_color(self.theme.foreground)
                .to_style(self.theme.foreground);
            spans.push(Span::styled(hint.clone(), style));
        }

        let mut paragraph = Paragraph::new(Line::from(spans));
        if let Some(bg) = self.theme.background {
            paragraph = paragraph.style(ratatui::style::Style::default().bg(bg));
        }

        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);

        if self.cursor_visible && self.input_enabled {
            let x = area.x + self.prompt_width() as u16 + self.cursor_offset() as u16;
            let y = area.y;
            frame.set_cursor_position((x, y));
        }
    }

    fn cursor_offset(&self) -> usize {
        UnicodeWidthStr::width(self.input.prefix())
    }

    fn prompt_width(&self) -> usize {
        UnicodeWidthStr::width(self.prompt_prefix.as_str())
    }
}
