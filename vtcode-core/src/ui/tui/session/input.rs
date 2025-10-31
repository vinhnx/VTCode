use super::{PLACEHOLDER_COLOR, Session, measure_text_width, ratatui_style_from_inline};
use crate::config::constants::ui;
use crate::ui::tui::types::InlineTextStyle;
use anstyle::Color as AnsiColorEnum;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

struct InputRender {
    text: Text<'static>,
    cursor_x: u16,
    cursor_y: u16,
}

#[derive(Default)]
struct InputLineBuffer {
    prefix: String,
    text: String,
    prefix_width: u16,
    text_width: u16,
}

impl InputLineBuffer {
    fn new(prefix: String, prefix_width: u16) -> Self {
        Self {
            prefix,
            text: String::new(),
            prefix_width,
            text_width: 0,
        }
    }
}

struct InputLayout {
    buffers: Vec<InputLineBuffer>,
    cursor_line_idx: usize,
    cursor_column: u16,
}

impl Session {
    pub(super) fn render_input(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 {
            return;
        }

        let mut input_area = area;
        let mut status_area = None;
        let mut status_line = None;

        if area.height >= 2 {
            if let Some(line) = self.render_input_status_line(area.width) {
                let block_height = area.height.saturating_sub(1).max(1);
                input_area.height = block_height;
                status_area = Some(Rect::new(area.x, area.y + block_height, area.width, 1));
                status_line = Some(line);
            }
        }

        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.accent_style());
        let inner = block.inner(input_area);
        let input_render = self.build_input_render(inner.width, inner.height);
        let paragraph = Paragraph::new(input_render.text)
            .style(self.default_style())
            .wrap(Wrap { trim: false })
            .block(block);
        frame.render_widget(paragraph, input_area);

        if self.cursor_should_be_visible() && inner.width > 0 && inner.height > 0 {
            let cursor_x = input_render
                .cursor_x
                .min(inner.width.saturating_sub(1))
                .saturating_add(inner.x);
            let cursor_y = input_render
                .cursor_y
                .min(inner.height.saturating_sub(1))
                .saturating_add(inner.y);
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        if let (Some(status_rect), Some(line)) = (status_area, status_line) {
            let paragraph = Paragraph::new(line)
                .style(self.default_style())
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, status_rect);
        }
    }

    pub(super) fn desired_input_lines(&self, inner_width: u16) -> u16 {
        if inner_width == 0 || self.input.is_empty() {
            return 1;
        }

        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(inner_width);
        let layout = self.input_layout(inner_width, prompt_display_width);
        let line_count = layout.buffers.len().max(1);
        let capped = line_count.min(ui::INLINE_INPUT_MAX_LINES.max(1));
        capped as u16
    }

    pub(super) fn apply_input_height(&mut self, height: u16) {
        let resolved = height.max(Self::input_block_height_for_lines(1));
        if self.input_height != resolved {
            self.input_height = resolved;
            self.recalculate_transcript_rows();
        }
    }

    pub(super) fn input_block_height_for_lines(lines: u16) -> u16 {
        lines.max(1).saturating_add(2)
    }

    fn input_layout(&self, width: u16, prompt_display_width: u16) -> InputLayout {
        let indent_prefix = " ".repeat(prompt_display_width as usize);
        let mut buffers = vec![InputLineBuffer::new(
            self.prompt_prefix.clone(),
            prompt_display_width,
        )];
        let secure_prompt_active = self.secure_prompt_active();
        let mut cursor_line_idx = 0usize;
        let mut cursor_column = prompt_display_width;
        let mut cursor_set = self.cursor == 0;

        for (idx, ch) in self.input.char_indices() {
            if !cursor_set && self.cursor == idx {
                if let Some(current) = buffers.last() {
                    cursor_line_idx = buffers.len() - 1;
                    cursor_column = current.prefix_width + current.text_width;
                    cursor_set = true;
                }
            }

            if ch == '\n' {
                let end = idx + ch.len_utf8();
                buffers.push(InputLineBuffer::new(
                    indent_prefix.clone(),
                    prompt_display_width,
                ));
                if !cursor_set && self.cursor == end {
                    cursor_line_idx = buffers.len() - 1;
                    cursor_column = prompt_display_width;
                    cursor_set = true;
                }
                continue;
            }

            let display_ch = if secure_prompt_active { '•' } else { ch };
            let char_width = UnicodeWidthChar::width(display_ch).unwrap_or(0) as u16;

            if let Some(current) = buffers.last_mut() {
                let capacity = width.saturating_sub(current.prefix_width);
                if capacity > 0
                    && current.text_width + char_width > capacity
                    && !current.text.is_empty()
                {
                    buffers.push(InputLineBuffer::new(
                        indent_prefix.clone(),
                        prompt_display_width,
                    ));
                }
            }

            if let Some(current) = buffers.last_mut() {
                current.text.push(display_ch);
                current.text_width = current.text_width.saturating_add(char_width);
            }

            let end = idx + ch.len_utf8();
            if !cursor_set && self.cursor == end {
                if let Some(current) = buffers.last() {
                    cursor_line_idx = buffers.len() - 1;
                    cursor_column = current.prefix_width + current.text_width;
                    cursor_set = true;
                }
            }
        }

        if !cursor_set {
            if let Some(current) = buffers.last() {
                cursor_line_idx = buffers.len() - 1;
                cursor_column = current.prefix_width + current.text_width;
            }
        }

        InputLayout {
            buffers,
            cursor_line_idx,
            cursor_column,
        }
    }

    fn build_input_render(&self, width: u16, height: u16) -> InputRender {
        if width == 0 || height == 0 {
            return InputRender {
                text: Text::default(),
                cursor_x: 0,
                cursor_y: 0,
            };
        }

        let max_visible_lines = height.max(1).min(ui::INLINE_INPUT_MAX_LINES as u16) as usize;

        let mut prompt_style = self.prompt_style.clone();
        if prompt_style.color.is_none() {
            prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
        let prompt_style = ratatui_style_from_inline(&prompt_style, self.theme.foreground);
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(width);

        if self.input.is_empty() {
            let mut spans = Vec::new();
            spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

            if let Some(placeholder) = &self.placeholder {
                let placeholder_style =
                    self.placeholder_style
                        .clone()
                        .unwrap_or_else(|| InlineTextStyle {
                            color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                            italic: true,
                            ..InlineTextStyle::default()
                        });
                let style = ratatui_style_from_inline(
                    &placeholder_style,
                    Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                );
                spans.push(Span::styled(placeholder.clone(), style));
            }

            return InputRender {
                text: Text::from(vec![Line::from(spans)]),
                cursor_x: prompt_display_width,
                cursor_y: 0,
            };
        }

        let accent_style =
            ratatui_style_from_inline(&self.accent_inline_style(), self.theme.foreground);
        let layout = self.input_layout(width, prompt_display_width);
        let total_lines = layout.buffers.len();
        let visible_limit = max_visible_lines.max(1);
        let mut start = total_lines.saturating_sub(visible_limit);
        if layout.cursor_line_idx < start {
            start = layout.cursor_line_idx.saturating_sub(visible_limit - 1);
        }
        let end = (start + visible_limit).min(total_lines);
        let cursor_y = layout.cursor_line_idx.saturating_sub(start) as u16;

        let mut lines = Vec::new();
        for buffer in &layout.buffers[start..end] {
            let mut spans = Vec::new();
            spans.push(Span::styled(buffer.prefix.clone(), prompt_style));
            if !buffer.text.is_empty() {
                spans.push(Span::styled(buffer.text.clone(), accent_style));
            }
            lines.push(Line::from(spans));
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                self.prompt_prefix.clone(),
                prompt_style,
            )]));
        }

        InputRender {
            text: Text::from(lines),
            cursor_x: layout.cursor_column,
            cursor_y,
        }
    }

    fn render_input_status_line(&self, width: u16) -> Option<Line<'static>> {
        if width == 0 {
            return None;
        }

        let left = self
            .input_status_left
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let right = self
            .input_status_right
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if left.is_none() && right.is_none() {
            return None;
        }

        let style = self.default_style().add_modifier(Modifier::DIM);
        let mut spans = Vec::new();

        match (left, right) {
            (Some(left_value), Some(right_value)) => {
                let left_width = measure_text_width(&left_value);
                let right_width = measure_text_width(&right_value);
                let padding = width.saturating_sub(left_width + right_width);

                spans.extend(self.create_git_status_spans(&left_value, style));

                if padding > 0 {
                    spans.push(Span::raw(" ".repeat(padding as usize)));
                } else {
                    spans.push(Span::raw(" ".to_string()));
                }
                spans.push(Span::styled(right_value, style));
            }
            (Some(left_value), None) => {
                spans.extend(self.create_git_status_spans(&left_value, style));
            }
            (None, Some(right_value)) => {
                let right_width = measure_text_width(&right_value);
                if width > right_width {
                    spans.push(Span::raw(" ".repeat((width - right_width) as usize)));
                }
                spans.push(Span::styled(right_value, style));
            }
            (None, None) => return None,
        }

        Some(Line::from(spans))
    }

    #[allow(dead_code)]
    fn create_git_status_spans(&self, text: &str, default_style: Style) -> Vec<Span<'static>> {
        if let Some((branch_part, indicator_part)) = text.rsplit_once(" | ") {
            let mut spans = Vec::new();
            let branch_trim = branch_part.trim_end();
            if !branch_trim.is_empty() {
                spans.push(Span::styled(branch_trim.to_string(), default_style));
            }
            spans.push(Span::raw(" "));

            let indicator_trim = indicator_part.trim();
            let indicator_style = if indicator_trim == ui::HEADER_GIT_DIRTY_SUFFIX {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else if indicator_trim == ui::HEADER_GIT_CLEAN_SUFFIX {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                self.accent_style().add_modifier(Modifier::BOLD)
            };

            spans.push(Span::styled(indicator_trim.to_string(), indicator_style));
            spans
        } else {
            vec![Span::styled(text.to_string(), default_style)]
        }
    }

    fn cursor_should_be_visible(&self) -> bool {
        self.cursor_visible && self.input_enabled
    }

    fn secure_prompt_active(&self) -> bool {
        self.modal
            .as_ref()
            .and_then(|modal| modal.secure_prompt.as_ref())
            .is_some()
    }
}
