use super::{PLACEHOLDER_COLOR, Session, measure_text_width, ratatui_style_from_inline};
use crate::config::constants::ui;
use crate::tools::file_ops::is_image_path;
use crate::ui::tui::types::InlineTextStyle;
use anstyle::{Color as AnsiColorEnum, Effects};
use ratatui::{
    buffer::Buffer,
    prelude::*,
    widgets::{Block, Clear, Padding, Paragraph, Wrap},
};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use tui_shimmer::shimmer_spans_with_style_at_phase;
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
            self.set_input_area(None);
            return;
        }

        self.set_input_area(Some(area));

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

        let background_style = self.styles.input_background_style();
        let block = Block::new().style(background_style).padding(Padding::new(
            ui::INLINE_INPUT_PADDING_HORIZONTAL,
            ui::INLINE_INPUT_PADDING_HORIZONTAL,
            ui::INLINE_INPUT_PADDING_VERTICAL,
            ui::INLINE_INPUT_PADDING_VERTICAL,
        ));
        let inner = block.inner(input_area);
        let input_render = self.build_input_render(inner.width, inner.height);
        let paragraph = Paragraph::new(input_render.text)
            .style(background_style)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph.block(block), input_area);

        if self.cursor_should_be_visible() && inner.width > 0 && inner.height > 0 {
            let cursor_x = input_render
                .cursor_x
                .min(inner.width.saturating_sub(1))
                .saturating_add(inner.x);
            let cursor_y = input_render
                .cursor_y
                .min(inner.height.saturating_sub(1))
                .saturating_add(inner.y);
            if self.use_fake_cursor() {
                render_fake_cursor(frame.buffer_mut(), cursor_x, cursor_y);
            } else {
                frame.set_cursor_position(Position::new(cursor_x, cursor_y));
            }
        }

        if let (Some(status_rect), Some(line)) = (status_area, status_line) {
            let paragraph = Paragraph::new(line)
                .style(self.styles.default_style())
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, status_rect);
        }
    }

    pub(crate) fn desired_input_lines(&self, inner_width: u16) -> u16 {
        if inner_width == 0 {
            return 1;
        }

        if self.input_compact_mode
            && self.input_manager.cursor() == self.input_manager.content().len()
            && self.input_compact_placeholder().is_some()
        {
            return 1;
        }

        if self.input_manager.content().is_empty() {
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
        lines
            .max(1)
            .saturating_add(ui::INLINE_INPUT_PADDING_VERTICAL.saturating_mul(2))
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

        let cursor_at_end = self.input_manager.cursor() == self.input_manager.content().len();
        if self.input_compact_mode
            && cursor_at_end
            && let Some(placeholder) = self.input_compact_placeholder()
        {
            let placeholder_style = InlineTextStyle {
                color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                bg_color: None,
                effects: Effects::DIMMED,
            };
            let style = ratatui_style_from_inline(
                &placeholder_style,
                Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
            );
            let placeholder_width = UnicodeWidthStr::width(placeholder.as_str()) as u16;
            return InputRender {
                text: Text::from(vec![Line::from(vec![
                    Span::styled(self.prompt_prefix.clone(), prompt_style),
                    Span::styled(placeholder, style),
                ])]),
                cursor_x: prompt_display_width.saturating_add(placeholder_width),
                cursor_y: 0,
            };
        }

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

    pub(super) fn input_compact_placeholder(&self) -> Option<String> {
        let content = self.input_manager.content();
        let trimmed = content.trim();
        let attachment_count = self.input_manager.attachments().len();
        if trimmed.is_empty() && attachment_count == 0 {
            return None;
        }

        if let Some(label) = compact_image_label(trimmed) {
            return Some(format!("[Image: {label}]"));
        }

        if attachment_count > 0 {
            let label = if attachment_count == 1 {
                "1 attachment".to_string()
            } else {
                format!("{attachment_count} attachments")
            };
            if trimmed.is_empty() {
                return Some(format!("[Image: {label}]"));
            }
            if let Some(compact) = compact_image_placeholders(content) {
                return Some(format!("[Image: {label}] {compact}"));
            }
            return Some(format!("[Image: {label}] {trimmed}"));
        }

        let line_count = content.split('\n').count();
        if line_count >= ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD {
            let char_count = content.chars().count();
            return Some(format!("[Pasted Content {char_count} chars]"));
        }

        if let Some(compact) = compact_image_placeholders(content) {
            return Some(compact);
        }

        None
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

        // Add left content (git status or shimmered activity)
        if let Some(left_value) = left.as_ref() {
            if status_requires_shimmer(left_value) {
                spans.extend(shimmer_spans_with_style_at_phase(
                    left_value,
                    dim_style,
                    self.shimmer_state.phase(),
                ));
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
        let loading_state = self.is_running_activity() || self.has_status_spinner();
        self.cursor_visible && (self.input_enabled || loading_state)
    }

    fn use_fake_cursor(&self) -> bool {
        self.has_status_spinner()
    }

    fn secure_prompt_active(&self) -> bool {
        self.modal
            .as_ref()
            .and_then(|modal| modal.secure_prompt.as_ref())
            .is_some()
    }

    /// Build input render data for external widgets
    pub fn build_input_widget_data(&self, width: u16, height: u16) -> InputWidgetData {
        let input_render = self.build_input_render(width, height);
        let background_style = self.styles.input_background_style();

        InputWidgetData {
            text: input_render.text,
            cursor_x: input_render.cursor_x,
            cursor_y: input_render.cursor_y,
            cursor_should_be_visible: self.cursor_should_be_visible(),
            use_fake_cursor: self.use_fake_cursor(),
            background_style,
            default_style: self.styles.default_style(),
        }
    }

    /// Build input status line for external widgets
    pub fn build_input_status_widget_data(&self, width: u16) -> Option<Vec<Span<'static>>> {
        self.render_input_status_line(width).map(|line| line.spans)
    }
}

fn compact_image_label(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(trimmed);

    if unquoted.starts_with("data:image/") {
        return Some("inline image".to_string());
    }

    let windows_drive = unquoted.as_bytes().get(1).is_some_and(|ch| *ch == b':')
        && unquoted
            .as_bytes()
            .get(2)
            .is_some_and(|ch| *ch == b'\\' || *ch == b'/');
    let starts_like_path = unquoted.starts_with('@')
        || unquoted.starts_with("file://")
        || unquoted.starts_with('/')
        || unquoted.starts_with("./")
        || unquoted.starts_with("../")
        || unquoted.starts_with("~/")
        || windows_drive;
    if !starts_like_path {
        return None;
    }

    let without_at = unquoted.strip_prefix('@').unwrap_or(unquoted);

    // Skip npm scoped package patterns like @scope/package@version
    if without_at.contains('/')
        && !without_at.starts_with('.')
        && !without_at.starts_with('/')
        && !without_at.starts_with("~/")
    {
        // Check if this looks like @scope/package (npm package)
        let parts: Vec<&str> = without_at.split('/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() {
            // Reject if it looks like a package name (no extension on second component)
            if !parts[parts.len() - 1].contains('.') {
                return None;
            }
        }
    }

    let without_scheme = without_at.strip_prefix("file://").unwrap_or(without_at);
    let path = Path::new(without_scheme);
    if !is_image_path(path) {
        return None;
    }

    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(without_scheme);
    Some(label.to_string())
}

static IMAGE_PATH_INLINE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        (?:^|[\s\(\[\{<\"'`])
        (
            @?
            (?:file://)?
            (?:
                ~/(?:[^\n/]+/)+
              | /(?:[^\n/]+/)+
              | [A-Za-z]:[\\/](?:[^\n\\\/]+[\\/])+
            )
            [^\n]*?
            \.(?:png|jpe?g|gif|bmp|webp|tiff?|svg)
        )"#,
    )
    .expect("Failed to compile inline image path regex")
});

fn compact_image_placeholders(content: &str) -> Option<String> {
    let mut matches = Vec::new();
    for capture in IMAGE_PATH_INLINE_REGEX.captures_iter(content) {
        let Some(path_match) = capture.get(1) else {
            continue;
        };
        let raw = path_match.as_str();
        let Some(label) = image_label_for_path(raw) else {
            continue;
        };
        matches.push((path_match.start(), path_match.end(), label));
    }

    if matches.is_empty() {
        return None;
    }

    let mut result = String::with_capacity(content.len());
    let mut last_end = 0usize;
    for (start, end, label) in matches {
        if start < last_end {
            continue;
        }
        result.push_str(&content[last_end..start]);
        result.push_str(&format!("[Image: {label}]"));
        last_end = end;
    }
    if last_end < content.len() {
        result.push_str(&content[last_end..]);
    }

    Some(result)
}

fn image_label_for_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim_matches(|ch: char| matches!(ch, '"' | '\'')).trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_at = trimmed.strip_prefix('@').unwrap_or(trimmed);
    let without_scheme = without_at.strip_prefix("file://").unwrap_or(without_at);
    let unescaped = unescape_whitespace(without_scheme);
    let path = Path::new(unescaped.as_str());
    if !is_image_path(path) {
        return None;
    }

    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(unescaped.as_str());
    Some(label.to_string())
}

fn unescape_whitespace(token: &str) -> String {
    let mut result = String::with_capacity(token.len());
    let mut chars = token.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(next) = chars.peek()
            && next.is_ascii_whitespace()
        {
            result.push(*next);
            chars.next();
            continue;
        }
        result.push(ch);
    }
    result
}

fn is_spinner_frame(indicator: &str) -> bool {
    matches!(
        indicator,
        "⠋" | "⠙"
            | "⠹"
            | "⠸"
            | "⠼"
            | "⠴"
            | "⠦"
            | "⠧"
            | "⠇"
            | "⠏"
            | "-"
            | "\\"
            | "|"
            | "/"
            | "."
    )
}

pub(crate) fn status_requires_shimmer(text: &str) -> bool {
    if text.contains("Running command:")
        || text.contains("Running tool:")
        || text.contains("Running:")
        || text.contains("Running ")
        || text.contains("Executing ")
        || text.contains("Press Ctrl+C to cancel")
    {
        return true;
    }
    let Some((indicator, rest)) = text.split_once(' ') else {
        return false;
    };
    if indicator.chars().count() != 1 || rest.trim().is_empty() {
        return false;
    }
    is_spinner_frame(indicator)
}

/// Data structure for input widget rendering
#[derive(Clone, Debug)]
pub struct InputWidgetData {
    pub text: Text<'static>,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_should_be_visible: bool,
    pub use_fake_cursor: bool,
    pub background_style: Style,
    pub default_style: Style,
}

fn render_fake_cursor(buf: &mut Buffer, cursor_x: u16, cursor_y: u16) {
    if let Some(cell) = buf.cell_mut((cursor_x, cursor_y)) {
        let mut style = cell.style();
        style = style.add_modifier(Modifier::REVERSED);
        cell.set_style(style);
        if cell.symbol().is_empty() {
            cell.set_symbol(" ");
        }
    }
}
