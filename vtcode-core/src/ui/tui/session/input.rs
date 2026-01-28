use super::terminal_capabilities;
use super::{PLACEHOLDER_COLOR, Session, measure_text_width, ratatui_style_from_inline};
use crate::config::constants::ui;
use crate::ui::tui::types::{EditingMode, InlineTextStyle};
use anstyle::{Color as AnsiColorEnum, Effects};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use tui_shimmer::shimmer_spans_with_style;
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

        if area.height >= 2
            && let Some(line) = self.render_input_status_line(area.width)
        {
            let block_height = area.height.saturating_sub(1).max(1);
            input_area.height = block_height;
            status_area = Some(Rect::new(area.x, area.y + block_height, area.width, 1));
            status_line = Some(line);
        }

        // Determine border styling based on editing mode and autonomous state
        let base_border_style = self.styles.accent_style();
        let autonomous_style = ratatui::style::Style::default()
            .fg(ratatui::style::Color::Green)
            .add_modifier(ratatui::style::Modifier::BOLD);
        let border_style = match self.header_context.editing_mode {
            EditingMode::Plan => ratatui::style::Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD),
            EditingMode::Edit => {
                if self.header_context.autonomous_mode {
                    autonomous_style
                } else {
                    base_border_style
                }
            }
        };

        // Determine border type - use double borders for trust modes
        let border_type = if self.is_full_auto_trust() || self.is_tools_policy_trust() {
            ratatui::widgets::BorderType::Double
        } else {
            terminal_capabilities::get_border_type()
        };

        let block = Block::new()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_type(border_type)
            .style(self.styles.default_style())
            .border_style(border_style);
        let inner = block.inner(input_area);
        let input_render = self.build_input_render(inner.width, inner.height);
        let paragraph = Paragraph::new(input_render.text)
            .style(self.styles.default_style())
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
            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }

        if let (Some(status_rect), Some(line)) = (status_area, status_line) {
            let paragraph = Paragraph::new(line)
                .style(self.styles.default_style())
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, status_rect);
        }
    }

    pub(crate) fn desired_input_lines(&self, inner_width: u16) -> u16 {
        if inner_width == 0 || self.input_manager.content().is_empty() {
            return 1;
        }

        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(inner_width);
        let layout = self.input_layout(inner_width, prompt_display_width);
        let line_count = layout.buffers.len().max(1);
        let capped = line_count.min(ui::INLINE_INPUT_MAX_LINES.max(1));
        capped as u16
    }

    pub(crate) fn apply_input_height(&mut self, height: u16) {
        let resolved = height.max(Self::input_block_height_for_lines(1));
        if self.input_height != resolved {
            self.input_height = resolved;
            crate::ui::tui::session::render::recalculate_transcript_rows(self);
        }
    }

    pub(crate) fn input_block_height_for_lines(lines: u16) -> u16 {
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
        let input_content = self.input_manager.content();
        let cursor_pos = self.input_manager.cursor();
        let mut cursor_set = cursor_pos == 0;

        for (idx, ch) in input_content.char_indices() {
            if !cursor_set
                && cursor_pos == idx
                && let Some(current) = buffers.last()
            {
                cursor_line_idx = buffers.len() - 1;
                cursor_column = current.prefix_width + current.text_width;
                cursor_set = true;
            }

            if ch == '\n' {
                let end = idx + ch.len_utf8();
                buffers.push(InputLineBuffer::new(
                    indent_prefix.clone(),
                    prompt_display_width,
                ));
                if !cursor_set && cursor_pos == end {
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
            if !cursor_set
                && cursor_pos == end
                && let Some(current) = buffers.last()
            {
                cursor_line_idx = buffers.len() - 1;
                cursor_column = current.prefix_width + current.text_width;
                cursor_set = true;
            }
        }

        if !cursor_set && let Some(current) = buffers.last() {
            cursor_line_idx = buffers.len() - 1;
            cursor_column = current.prefix_width + current.text_width;
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

        if self.input_manager.content().is_empty() {
            let mut spans = Vec::new();
            spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

            if let Some(placeholder) = &self.placeholder {
                let placeholder_style = self.placeholder_style.clone().unwrap_or(InlineTextStyle {
                    color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                    bg_color: None,
                    effects: Effects::ITALIC,
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
            ratatui_style_from_inline(&self.styles.accent_inline_style(), self.theme.foreground);
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
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let right = self
            .input_status_right
            .as_ref()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        // Build scroll indicator if enabled
        let scroll_indicator = if ui::SCROLL_INDICATOR_ENABLED {
            Some(self.build_scroll_indicator())
        } else {
            None
        };

        if left.is_none() && right.is_none() && scroll_indicator.is_none() {
            return None;
        }

        let dim_style = self.styles.default_style().add_modifier(Modifier::DIM);
        let mut spans = Vec::new();

        // Add left content (git status)
        if let Some(left_value) = left.as_ref() {
            if let Some((indicator, rest)) = split_running_command_status(left_value) {
                let indicator_style = ratatui_style_from_inline(
                    &self.styles.tool_inline_style("run"),
                    self.theme.foreground,
                );
                spans.push(Span::styled(indicator, indicator_style));
                spans.push(Span::raw(" "));
                spans.extend(shimmer_spans_with_style(&rest, dim_style));
            } else {
                spans.extend(self.create_git_status_spans(left_value, dim_style));
            }
        }

        // Build right side spans (scroll indicator + optional right content)
        let mut right_spans: Vec<Span<'static>> = Vec::new();
        if let Some(scroll) = &scroll_indicator {
            right_spans.push(Span::styled(scroll.clone(), dim_style));
        }
        if let Some(right_value) = &right {
            if !right_spans.is_empty() {
                right_spans.push(Span::raw(" "));
            }
            right_spans.push(Span::styled(right_value.clone(), dim_style));
        }

        if !right_spans.is_empty() {
            let left_width: u16 = spans.iter().map(|s| measure_text_width(&s.content)).sum();
            let right_width: u16 = right_spans
                .iter()
                .map(|s| measure_text_width(&s.content))
                .sum();
            let padding = width.saturating_sub(left_width + right_width);

            if padding > 0 {
                spans.push(Span::raw(" ".repeat(padding as usize)));
            } else if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            spans.extend(right_spans);
        }

        if spans.is_empty() {
            return None;
        }

        Some(Line::from(spans))
    }

    /// Build scroll indicator string with percentage
    fn build_scroll_indicator(&self) -> String {
        let percent = self.scroll_manager.progress_percent();
        format!("{} {:>3}%", ui::SCROLL_INDICATOR_FORMAT, percent)
    }

    #[allow(dead_code)]
    fn create_git_status_spans(&self, text: &str, default_style: Style) -> Vec<Span<'static>> {
        if let Some((branch_part, indicator_part)) = text.rsplit_once(" | ") {
            let mut spans = Vec::new();
            let branch_trim = branch_part.trim_end();
            if !branch_trim.is_empty() {
                spans.push(Span::styled(branch_trim.to_owned(), default_style));
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
                self.styles.accent_style().add_modifier(Modifier::BOLD)
            };

            spans.push(Span::styled(indicator_trim.to_owned(), indicator_style));
            spans
        } else {
            vec![Span::styled(text.to_owned(), default_style)]
        }
    }

    fn cursor_should_be_visible(&self) -> bool {
        self.cursor_visible && self.input_enabled && !self.is_running_activity()
    }

    fn secure_prompt_active(&self) -> bool {
        self.modal
            .as_ref()
            .and_then(|modal| modal.secure_prompt.as_ref())
            .is_some()
    }

    fn is_full_auto_trust(&self) -> bool {
        self.header_context.workspace_trust.contains("full auto")
    }

    fn is_tools_policy_trust(&self) -> bool {
        self.header_context.workspace_trust.contains("tools policy")
    }

    /// Build input render data for external widgets
    pub fn build_input_widget_data(&self, width: u16, height: u16) -> InputWidgetData {
        let input_render = self.build_input_render(width, height);

        // Determine border styling based on editing mode and autonomous state
        let base_border_style = self.styles.accent_style();
        let autonomous_style = ratatui::style::Style::default()
            .fg(ratatui::style::Color::Green)
            .add_modifier(ratatui::style::Modifier::BOLD);
        let border_style = match self.header_context.editing_mode {
            EditingMode::Plan => ratatui::style::Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD),
            EditingMode::Edit => {
                if self.header_context.autonomous_mode {
                    autonomous_style
                } else {
                    base_border_style
                }
            }
        };

        InputWidgetData {
            text: input_render.text,
            cursor_x: input_render.cursor_x,
            cursor_y: input_render.cursor_y,
            is_full_auto_trust: self.is_full_auto_trust(),
            is_tools_policy_trust: self.is_tools_policy_trust(),
            cursor_should_be_visible: self.cursor_should_be_visible(),
            border_style,
            default_style: self.styles.default_style(),
        }
    }

    /// Build input status line for external widgets
    pub fn build_input_status_widget_data(&self, width: u16) -> Option<Vec<Span<'static>>> {
        self.render_input_status_line(width).map(|line| line.spans)
    }
}

fn split_running_command_status(text: &str) -> Option<(String, String)> {
    let (indicator, rest) = text.split_once(' ')?;
    if indicator.chars().count() != 1 {
        return None;
    }
    if is_spinner_frame(indicator) && !rest.trim().is_empty() {
        Some((indicator.to_string(), rest.to_string()))
    } else {
        None
    }
}

fn is_spinner_frame(indicator: &str) -> bool {
    matches!(
        indicator,
        "⠋" | "⠙" | "⠹" | "⠸" | "⠼" | "⠴" | "⠦" | "⠧" | "⠇" | "⠏"
            | "-" | "\\" | "|" | "/" | "."
    )
}

/// Data structure for input widget rendering
#[derive(Clone, Debug)]
pub struct InputWidgetData {
    pub text: Text<'static>,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub is_full_auto_trust: bool,
    pub is_tools_policy_trust: bool,
    pub cursor_should_be_visible: bool,
    pub border_style: Style,
    pub default_style: Style,
}
