use super::{
    Action, PLACEHOLDER_COLOR, Session, measure_text_width, ratatui_color_from_ansi, ratatui_style_from_inline,
};
use crate::tui::config::constants::ui;
use crate::tui::ui::tui::types::InlineTextStyle;
use anstyle::{Color as AnsiColorEnum, Effects};
use ratatui::{
    buffer::Buffer,
    prelude::*,
    widgets::{Block, Padding, Paragraph, Wrap},
};

mod helpers;
mod tokenizer;

pub(crate) use helpers::{InputWidgetData, should_track_compact_paste, status_requires_shimmer};
use helpers::{
    byte_index_to_char_index, char_index_to_byte_index, compact_image_label, compact_image_placeholders,
    compact_paste_range_preview, display_width_for_char_range, generic_large_input_preview,
    inline_prompt_suggestion_suffix, render_fake_cursor,
};

use tokenizer::{InputToken, InputTokenKind, tokenize_input};

/// Paint pre-wrapped lines into `area` without Paragraph wrapping.
/// `base` is the Paragraph base style; span styles patch on top.
fn paint_pre_wrapped_text(text: &Text<'static>, area: Rect, buf: &mut Buffer, base: Style) {
    for (row, line) in text.lines.iter().take(usize::from(area.height)).enumerate() {
        let y = area.y + row as u16;
        let mut x = area.x;
        for span in &line.spans {
            if x >= area.right() {
                break;
            }
            let merged = base.patch(span.style);
            let (end_x, _) =
                buf.set_stringn(x, y, span.content.as_ref(), area.right().saturating_sub(x) as usize, merged);
            x = end_x;
        }
    }
}

fn paint_pre_wrapped_line(line: &Line<'static>, area: Rect, buf: &mut Buffer, base: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut x = area.x;
    let y = area.y;
    for span in &line.spans {
        if x >= area.right() {
            break;
        }
        let merged = base.patch(span.style);
        let (end_x, _) = buf.set_stringn(x, y, span.content.as_ref(), area.right().saturating_sub(x) as usize, merged);
        x = end_x;
    }
}
use tui_shimmer::shimmer_spans_with_style_at_phase;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::utils::line_truncation::truncate_line_with_ellipsis_if_overflow;

pub(super) struct InputRender {
    pub(super) text: Text<'static>,
    cursor_x: u16,
    cursor_y: u16,
}

#[derive(Default)]
struct InputStatusLine {
    line: Line<'static>,
    background_hits: Vec<(u16, u16)>,
    progress_start: u16,
    progress_columns: u16,
}

struct CompactInputPreview {
    before: String,
    placeholder: String,
    after_lines: Vec<String>,
}

impl CompactInputPreview {
    fn line_count(&self) -> usize {
        self.after_lines.len().max(1)
    }
}

#[derive(Default)]
struct InputLineBuffer {
    prefix: String,
    text: String,
    prefix_width: u16,
    text_width: u16,
    /// Character index in the original input where this buffer's text starts.
    char_start: usize,
}

impl InputLineBuffer {
    fn new(prefix: String, prefix_width: u16, char_start: usize) -> Self {
        Self {
            prefix,
            text: String::new(),
            prefix_width,
            text_width: 0,
            char_start,
        }
    }
}

struct InputLayout {
    buffers: Vec<InputLineBuffer>,
    cursor_line_idx: usize,
    cursor_column: u16,
}

/// Visual-row geometry of the composer input.
///
/// Dimension key: `total_rows` is the soft-wrapped row count (at least 1);
/// `cursor_row` is the 0-based row holding the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InputVisualGeometry {
    pub(crate) total_rows: usize,
    pub(crate) cursor_row: usize,
}

const SHELL_MODE_BORDER_TITLE: &str = " ! Shell mode ";
const SHELL_MODE_STATUS_HINT: &str = "Shell mode (!): direct command execution";

impl Session {
    /// Composer-area acknowledgement for queued follow-ups while a turn runs.
    ///
    /// The transcript queue overlay can be scrolled away (or covered by a live
    /// progress row) while the agent is working, so the composer's own status
    /// line also reports the pending count. Hidden while idle (the overlay
    /// already shows an idle queue); superseded by a copy notification or the
    /// active turn's own live phase status, and placed ahead of configured
    /// context and the generic "Thinking" label because a queued entry is the
    /// freshest user action.
    fn queued_input_status_text(&self) -> Option<String> {
        let pending = self.queued_inputs.len();
        if pending == 0 || !self.is_running_activity() || self.copy_notification_text().is_some() {
            return None;
        }
        // One queued item renders the truncated text so the user gets direct
        // confirmation of what was captured; several fall back to a count.
        if pending == 1 {
            const PREVIEW_CHARS: usize = 40;
            let entry = &self.queued_inputs[0];
            let flattened = entry.replace(['\r', '\n'], " ");
            let trimmed = flattened.trim();
            let preview: String = trimmed.chars().take(PREVIEW_CHARS).collect();
            let ellipsis = if trimmed.chars().count() > PREVIEW_CHARS {
                "…"
            } else {
                ""
            };
            return Some(format!("Queued: {preview}{ellipsis} — runs after this turn"));
        }
        Some(format!("{pending} turns queued — running after this turn"))
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(crate) fn render_input(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.height == 0 {
            self.set_input_area(None);
            self.set_background_indicator_hits(Vec::new());
            return;
        }

        let mut input_area = area;
        let mut status_area = None;
        if area.height > ui::INLINE_INPUT_STATUS_HEIGHT {
            let block_height = area.height.saturating_sub(ui::INLINE_INPUT_STATUS_HEIGHT);
            input_area.height = block_height.max(1);
            status_area = Some(Rect::new(area.x, area.y + block_height, area.width, ui::INLINE_INPUT_STATUS_HEIGHT));
        }

        let background_style = self.styles.input_background_style();
        let shell_mode_title = self.shell_mode_border_title();
        let active_subagent_title = self.active_subagent_input_title();
        let active_subagent_border_style = self.active_subagent_input_border_style();
        let mut block = if shell_mode_title.is_some() || active_subagent_title.is_some() {
            Block::bordered()
        } else {
            Block::new()
        };
        block = block.style(background_style).padding(self.input_block_padding());
        if shell_mode_title.is_some() || active_subagent_title.is_some() {
            block = block.border_type(super::terminal_capabilities::get_border_type()).border_style(
                active_subagent_border_style.unwrap_or_else(|| self.styles.accent_style().add_modifier(Modifier::BOLD)),
            );
        }
        if let Some(title) = shell_mode_title {
            block = block.title_top(Line::from(title).left_aligned());
        }
        if let Some(title) = active_subagent_title {
            block = block.title_top(title);
        }
        let inner = block.inner(input_area);
        self.set_input_area(Some(inner));
        let input_render = self.build_input_render(inner.width, inner.height);
        // Input rows are already soft-wrapped to `inner.width` by `input_layout`.
        // Paint via set_span — avoid Paragraph wrap every frame (hotpath ~5KB).
        frame.render_widget(block, input_area);
        {
            let buf = frame.buffer_mut();
            buf.set_style(inner, background_style);
            paint_pre_wrapped_text(&input_render.text, inner, buf, background_style);
        }
        self.apply_input_selection_highlight(frame.buffer_mut(), inner);
        // Auto-copy on select only when enabled; otherwise the selection stays
        // until the user copies manually with Ctrl+C (or Cmd+C).
        if self.fullscreen.interaction.copy_on_select && self.input_manager.selection_needs_copy() {
            let _ = self.copy_input_selection_to_clipboard();
        }

        if self.cursor_should_be_visible() && inner.width > 0 && inner.height > 0 {
            let cursor_x = input_render.cursor_x.min(inner.width.saturating_sub(1)).saturating_add(inner.x);
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

        if let Some(status_area) = status_area {
            let status = self.build_input_status_line(status_area.width).unwrap_or_default();
            {
                let buf = frame.buffer_mut();
                buf.set_style(status_area, self.styles.default_style());
                paint_pre_wrapped_line(&status.line, status_area, buf, self.styles.default_style());
            }
            if status.progress_columns > 0 {
                let feedback_area =
                    Rect::new(status_area.x + status.progress_start, status_area.y, status.progress_columns, 1);
                self.set_progress_feedback_area(feedback_area.intersection(frame.area()));
            }
            let hits = status
                .background_hits
                .into_iter()
                .map(|(start, end)| {
                    Rect::new(status_area.x.saturating_add(start), status_area.y, end.saturating_sub(start), 1)
                })
                .collect();
            self.set_background_indicator_hits(hits);
        } else {
            self.set_background_indicator_hits(Vec::new());
        }
    }

    pub(crate) fn desired_input_lines(&self, inner_width: u16) -> u16 {
        if inner_width == 0 {
            return 1;
        }

        if self.input_compact_mode && self.input_manager.cursor() == self.input_manager.content().len() {
            if let Some(preview) = self.input_compact_preview() {
                return preview.line_count().min(ui::INLINE_INPUT_MAX_LINES.max(1)) as u16;
            }
            if self.input_compact_placeholder().is_some() {
                return 1;
            }
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
            self.recalculate_transcript_rows();
        }
    }

    pub(crate) fn input_block_height_for_lines(lines: u16) -> u16 {
        lines.max(1).saturating_add(ui::INLINE_INPUT_PADDING_VERTICAL.saturating_mul(2))
    }

    pub(crate) fn input_block_extra_height(&self) -> u16 {
        if self.active_subagent_input_title().is_some() && !self.input_uses_shell_prefix() {
            2
        } else {
            0
        }
    }

    fn input_layout(&self, width: u16, prompt_display_width: u16) -> InputLayout {
        let indent_prefix = " ".repeat(prompt_display_width as usize);
        let mut buffers = vec![InputLineBuffer::new(
            self.prompt_prefix.clone(),
            prompt_display_width,
            0,
        )];
        let secure_prompt_active = self.secure_prompt_active();
        let mut cursor_line_idx = 0usize;
        let mut cursor_column = prompt_display_width;
        let input_content = self.input_manager.content();
        let cursor_pos = self.input_manager.cursor();
        let mut cursor_set = cursor_pos == 0;
        let mut char_idx: usize = 0;

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
                char_idx += 1;
                buffers.push(InputLineBuffer::new(indent_prefix.clone(), prompt_display_width, char_idx));
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
                if capacity > 0 && current.text_width + char_width > capacity && !current.text.is_empty() {
                    buffers.push(InputLineBuffer::new(indent_prefix.clone(), prompt_display_width, char_idx));
                }
            }

            if let Some(current) = buffers.last_mut() {
                current.text.push(display_ch);
                current.text_width = current.text_width.saturating_add(char_width);
            }

            char_idx += 1;

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

        InputLayout { buffers, cursor_line_idx, cursor_column }
    }

    fn visible_input_window(&self, width: u16, height: u16) -> (InputLayout, usize, usize) {
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(width);
        let layout = self.input_layout(width, prompt_display_width);
        let total_lines = layout.buffers.len();
        let visible_limit = height.max(1).min(ui::INLINE_INPUT_MAX_LINES as u16) as usize;
        let mut start = total_lines.saturating_sub(visible_limit);
        if layout.cursor_line_idx < start {
            start = layout.cursor_line_idx.saturating_sub(visible_limit - 1);
        }
        let end = (start + visible_limit).min(total_lines);
        (layout, start, end)
    }

    /// Visual-row geometry of the composer.
    ///
    /// Rows are soft-wrapped visual rows from [`Session::input_layout`], not
    /// logical `\n` lines: a long single-line draft that wraps on screen
    /// spans multiple visual rows. Returns `None` before the first render
    /// (no input area yet) or for a zero-width area.
    pub(crate) fn input_visual_geometry(&self) -> Option<InputVisualGeometry> {
        let area = self.input_area()?;
        if area.width == 0 {
            return None;
        }
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let layout = self.input_layout(area.width, prompt_width.min(area.width));
        Some(InputVisualGeometry {
            total_rows: layout.buffers.len().max(1),
            cursor_row: layout.cursor_line_idx,
        })
    }

    /// Whether the composer spans more than one visual row.
    ///
    /// Falls back to logical lines before the first render (no input area).
    pub(crate) fn is_multi_row_composer(&self) -> bool {
        match self.input_visual_geometry() {
            Some(geometry) => geometry.total_rows > 1,
            None => !self.input_manager.is_single_line(),
        }
    }

    /// Move the cursor up one visual (soft-wrapped) row.
    ///
    /// Returns `true` when the cursor moved, `false` at the first row or
    /// when the input area is unknown.
    pub(crate) fn move_cursor_up_within_visual(&mut self) -> bool {
        self.move_cursor_visual_rows(-1)
    }

    /// Move the cursor down one visual (soft-wrapped) row.
    ///
    /// Returns `true` when the cursor moved, `false` at the last row or
    /// when the input area is unknown.
    pub(crate) fn move_cursor_down_within_visual(&mut self) -> bool {
        self.move_cursor_visual_rows(1)
    }

    fn move_cursor_visual_rows(&mut self, delta: isize) -> bool {
        let area = match self.input_area() {
            Some(area) if area.width > 0 => area,
            _ => return false,
        };
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let layout = self.input_layout(area.width, prompt_width.min(area.width));
        let total = layout.buffers.len();
        if total == 0 {
            return false;
        }
        let current = layout.cursor_line_idx.min(total - 1);
        let Some(target_idx) = current.checked_add_signed(delta) else {
            return false;
        };
        if target_idx >= total || target_idx == current {
            return false;
        }
        let desired = layout.cursor_column.saturating_sub(layout.buffers[current].prefix_width);
        let target = &layout.buffers[target_idx];
        let mut acc = 0u16;
        let mut offset = 0usize;
        for ch in target.text.chars() {
            if acc >= desired {
                break;
            }
            acc = acc.saturating_add(UnicodeWidthChar::width(ch).unwrap_or(0) as u16);
            offset += 1;
        }
        let char_index = target.char_start.saturating_add(offset);
        self.input_manager
            .set_cursor(char_index_to_byte_index(self.input_manager.content(), char_index));
        true
    }

    /// Test-only entry that exercises the fingerprint cache.
    #[cfg(test)]
    pub(super) fn build_input_render_for_test(&mut self, width: u16, height: u16) -> InputRender {
        self.build_input_render(width, height)
    }

    /// Build (or reuse) the input paragraph model for the current input state.
    fn build_input_render(&mut self, width: u16, height: u16) -> InputRender {
        if width == 0 || height == 0 {
            return InputRender { text: Text::default(), cursor_x: 0, cursor_y: 0 };
        }

        // Hash content (not just length) so same-length edits cannot serve a
        // stale cached paragraph. Cursor and flags cover layout/placeholder.
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(self.input_manager.content(), &mut hasher);
        std::hash::Hash::hash(&self.prompt_prefix, &mut hasher);
        let content_hash = std::hash::Hasher::finish(&hasher);
        let cursor = self.input_manager.cursor();
        let compact = self.input_compact_mode;
        let suggested = self.suggested_prompt_state.active;
        let key = (width, height, content_hash, cursor, compact, suggested);

        if let Some((w, h, hash, cur, cmp, sug, cached)) = self.input_render_cache.take() {
            if (w, h, hash, cur, cmp, sug) == key {
                let render = InputRender {
                    text: cached.text.clone(),
                    cursor_x: cached.cursor_x,
                    cursor_y: cached.cursor_y,
                };
                self.input_render_cache = Some((w, h, hash, cur, cmp, sug, cached));
                return render;
            }
            // Stale — drop before rebuild.
        }

        let render = self.build_input_render_uncached(width, height);
        self.input_render_cache = Some((
            width,
            height,
            content_hash,
            cursor,
            compact,
            suggested,
            InputRender {
                text: render.text.clone(),
                cursor_x: render.cursor_x,
                cursor_y: render.cursor_y,
            },
        ));
        render
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn build_input_render_uncached(&self, width: u16, height: u16) -> InputRender {
        if width == 0 || height == 0 {
            return InputRender { text: Text::default(), cursor_x: 0, cursor_y: 0 };
        }

        let max_visible_lines = height.max(1).min(ui::INLINE_INPUT_MAX_LINES as u16) as usize;

        let mut prompt_style = self.prompt_style.clone();
        if prompt_style.color.is_none() {
            prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
        if self.suggested_prompt_state.active {
            prompt_style.color = self
                .theme
                .tool_accent
                .or(self.theme.secondary)
                .or(self.theme.primary)
                .or(self.theme.foreground);
            prompt_style.effects |= Effects::BOLD;
        }
        let prompt_style = ratatui_style_from_inline(&prompt_style, self.theme.foreground);
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(width);
        let accent_style = ratatui_style_from_inline(&self.styles.accent_inline_style(), self.theme.foreground);

        let cursor_at_end = self.input_manager.cursor() == self.input_manager.content().len();
        if self.input_compact_mode
            && cursor_at_end
            && let Some(preview) = self.input_compact_preview().or_else(|| {
                self.input_compact_placeholder().map(|placeholder| CompactInputPreview {
                    before: String::new(),
                    placeholder,
                    after_lines: Vec::new(),
                })
            })
        {
            let placeholder_style = InlineTextStyle {
                color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                bg_color: None,
                effects: Effects::DIMMED,
            };
            let style = ratatui_style_from_inline(&placeholder_style, Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)));
            let mut spans = Vec::new();
            spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));
            if !preview.before.is_empty() {
                let needs_space = !preview.before.ends_with(char::is_whitespace);
                spans.push(Span::styled(preview.before, accent_style));
                if needs_space {
                    spans.push(Span::styled(" ".to_string(), accent_style));
                }
            }
            let first_after = preview.after_lines.first().map(String::as_str).unwrap_or_default();
            let needs_after_space = !first_after.is_empty() && !first_after.starts_with(char::is_whitespace);
            spans.push(Span::styled(preview.placeholder, style));
            if needs_after_space {
                spans.push(Span::styled(" ".to_string(), accent_style));
            }
            if !first_after.is_empty() {
                spans.push(Span::styled(first_after.to_string(), accent_style));
            }
            let indent_prefix = " ".repeat(prompt_display_width as usize);
            let mut lines = vec![Line::from(spans)];
            for line in preview.after_lines.iter().skip(1) {
                let mut spans = vec![Span::styled(indent_prefix.clone(), prompt_style)];
                if !line.is_empty() {
                    spans.push(Span::styled(line.clone(), accent_style));
                }
                lines.push(Line::from(spans));
            }
            let cursor_y = lines.len().saturating_sub(1) as u16;
            let cursor_x = if cursor_y == 0 {
                lines[0]
                    .spans
                    .iter()
                    .skip(1)
                    .map(|span| UnicodeWidthStr::width(span.content.as_ref()) as u16)
                    .fold(prompt_display_width, u16::saturating_add)
            } else {
                let last_line = preview.after_lines.last().map(String::as_str).unwrap_or_default();
                prompt_display_width.saturating_add(UnicodeWidthStr::width(last_line) as u16)
            };
            return InputRender { text: Text::from(lines), cursor_x, cursor_y };
        }

        if self.input_manager.content().is_empty() {
            let mut spans = Vec::new();
            spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

            if let Some(suffix) = self.visible_inline_prompt_suggestion_suffix() {
                let ghost_style = ratatui_style_from_inline(
                    &InlineTextStyle {
                        color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                        bg_color: None,
                        effects: Effects::DIMMED | Effects::ITALIC,
                    },
                    Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                );
                spans.push(Span::styled(suffix, ghost_style));
            } else if let Some(placeholder) = &self.placeholder {
                let placeholder_style = self.placeholder_style.clone().unwrap_or(InlineTextStyle {
                    color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                    bg_color: None,
                    effects: Effects::ITALIC,
                });
                let style = ratatui_style_from_inline(&placeholder_style, Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)));
                spans.push(Span::styled(placeholder.clone(), style));
            }

            return InputRender {
                text: Text::from(vec![Line::from(spans)]),
                cursor_x: prompt_display_width,
                cursor_y: 0,
            };
        }

        let slash_style = accent_style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
        let file_ref_style = accent_style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
        let code_style = accent_style.fg(Color::Green).add_modifier(Modifier::BOLD);

        let (layout, start, end) = self.visible_input_window(width, max_visible_lines as u16);
        let tokens = tokenize_input(self.input_manager.content());
        let cursor_y = layout.cursor_line_idx.saturating_sub(start) as u16;

        let vis_count = end.saturating_sub(start);
        let mut lines = Vec::with_capacity(vis_count);
        for buffer in &layout.buffers[start..end] {
            let mut spans = Vec::with_capacity(4);
            spans.push(Span::styled(buffer.prefix.clone(), prompt_style));
            if !buffer.text.is_empty() {
                let buf_chars: Vec<char> = buffer.text.chars().collect();
                let buf_len = buf_chars.len();
                let buf_start = buffer.char_start;
                let buf_end = buf_start + buf_len;

                let mut pos = 0usize;
                for token in &tokens {
                    if token.end <= buf_start || token.start >= buf_end {
                        continue;
                    }
                    let seg_start = token.start.max(buf_start).saturating_sub(buf_start);
                    let seg_end = token.end.min(buf_end).saturating_sub(buf_start);
                    if seg_start > pos {
                        let text: String = buf_chars[pos..seg_start].iter().collect();
                        spans.push(Span::styled(text, accent_style));
                    }
                    let text: String = buf_chars[seg_start..seg_end].iter().collect();
                    let style = match token.kind {
                        InputTokenKind::SlashCommand => slash_style,
                        InputTokenKind::AgentReference | InputTokenKind::FileReference => file_ref_style,
                        InputTokenKind::InlineCode => code_style,
                        InputTokenKind::Normal => accent_style,
                    };
                    spans.push(Span::styled(text, style));
                    pos = seg_end;
                }
                if pos < buf_len {
                    let text: String = buf_chars[pos..].iter().collect();
                    spans.push(Span::styled(text, accent_style));
                }
            }
            lines.push(Line::from(spans));
        }

        if let Some(suffix) = self.visible_inline_prompt_suggestion_suffix() {
            let ghost_style = ratatui_style_from_inline(
                &InlineTextStyle {
                    color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                    bg_color: None,
                    effects: Effects::DIMMED | Effects::ITALIC,
                },
                Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
            );
            if let Some(line) = lines.get_mut(cursor_y as usize) {
                line.spans.push(Span::styled(suffix, ghost_style));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(self.prompt_prefix.clone(), prompt_style)]));
        }

        InputRender {
            text: Text::from(lines),
            cursor_x: layout.cursor_column,
            cursor_y,
        }
    }

    fn apply_input_selection_highlight(&self, buf: &mut Buffer, area: Rect) {
        let Some((selection_start, selection_end)) = self.input_manager.selection_range() else {
            return;
        };
        if area.width == 0 || area.height == 0 || selection_start == selection_end {
            return;
        }

        let (layout, start, end) = self.visible_input_window(area.width, area.height);
        let selection_start_char = byte_index_to_char_index(self.input_manager.content(), selection_start);
        let selection_end_char = byte_index_to_char_index(self.input_manager.content(), selection_end);

        for (row_offset, buffer) in layout.buffers[start..end].iter().enumerate() {
            let line_char_start = buffer.char_start;
            let line_char_end = buffer.char_start + buffer.text.chars().count();
            let highlight_start = selection_start_char.max(line_char_start);
            let highlight_end = selection_end_char.min(line_char_end);
            if highlight_start >= highlight_end {
                continue;
            }

            let local_start = highlight_start.saturating_sub(line_char_start);
            let local_end = highlight_end.saturating_sub(line_char_start);
            let start_x = area
                .x
                .saturating_add(buffer.prefix_width)
                .saturating_add(display_width_for_char_range(&buffer.text, local_start));
            let end_x = area
                .x
                .saturating_add(buffer.prefix_width)
                .saturating_add(display_width_for_char_range(&buffer.text, local_end));
            let y = area.y.saturating_add(row_offset as u16);

            for x in start_x..end_x.min(area.x.saturating_add(area.width)) {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    let mut style = cell.style();
                    style = style.add_modifier(Modifier::REVERSED);
                    cell.set_style(style);
                    if cell.symbol().is_empty() {
                        cell.set_symbol(" ");
                    }
                }
            }
        }
    }

    pub(crate) fn cursor_index_for_input_point(&self, column: u16, row: u16) -> Option<usize> {
        let area = self.input_area()?;
        if row < area.y
            || row >= area.y.saturating_add(area.height)
            || column < area.x
            || column >= area.x.saturating_add(area.width)
        {
            return None;
        }

        if self.input_compact_mode
            && self.input_manager.cursor() == self.input_manager.content().len()
            && self.input_compact_placeholder().is_some()
        {
            return Some(self.input_manager.content().len());
        }

        let relative_row = row.saturating_sub(area.y);
        let relative_column = column.saturating_sub(area.x);
        let (layout, start, end) = self.visible_input_window(area.width, area.height);
        if start >= end {
            return Some(0);
        }

        let line_index = (start + usize::from(relative_row)).min(end.saturating_sub(1));
        let buffer = layout.buffers.get(line_index)?;
        if relative_column <= buffer.prefix_width {
            return Some(char_index_to_byte_index(self.input_manager.content(), buffer.char_start));
        }

        let target_width = relative_column.saturating_sub(buffer.prefix_width);
        let mut consumed_width = 0u16;
        let mut char_offset = 0usize;
        for ch in buffer.text.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
            let next_width = consumed_width.saturating_add(ch_width);
            if target_width < next_width {
                break;
            }
            consumed_width = next_width;
            char_offset += 1;
        }

        let char_index = buffer.char_start.saturating_add(char_offset);
        Some(char_index_to_byte_index(self.input_manager.content(), char_index))
    }

    pub(crate) fn input_compact_placeholder(&self) -> Option<String> {
        let content = self.input_manager.content();
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Some(label) = compact_image_label(trimmed) {
            return Some(format!("[Image: {label}]"));
        }

        if let Some(preview) = self.input_compact_preview() {
            return Some(preview.placeholder);
        }

        if let Some(compact) = compact_image_placeholders(content) {
            // `compact_image_placeholders` returns the full content with image
            // paths substituted. Never render it unbounded: large inputs are
            // already collapsed by `input_compact_preview` above, so this is
            // only a safety net for small embeds.
            if compact.chars().count() >= ui::INLINE_INPUT_COMPACT_CHAR_THRESHOLD {
                let char_count = content.chars().count();
                return Some(format!("[Pasted Content {char_count} chars]"));
            }
            return Some(compact);
        }

        None
    }

    fn input_compact_preview(&self) -> Option<CompactInputPreview> {
        let content = self.input_manager.content();
        if let Some(preview) = compact_paste_range_preview(content, self.input_manager.compact_paste_range()) {
            return Some(preview);
        }

        generic_large_input_preview(content, self.input_manager.attachments().len())
    }

    pub(crate) fn visible_inline_prompt_suggestion_suffix(&self) -> Option<String> {
        if !self.input_enabled
            || self.has_active_overlay()
            || self.input_compact_mode
            || self.input_manager.cursor() != self.input_manager.content().len()
        {
            return None;
        }

        let suggestion = self.inline_prompt_suggestion.suggestion.as_deref()?;
        inline_prompt_suggestion_suffix(self.input_manager.content(), suggestion)
    }

    pub(crate) fn render_input_status_line(&self, width: u16) -> Option<Line<'static>> {
        self.render_input_status_line_with_hit(width).map(|(line, _hits)| line)
    }

    /// Status line plus column ranges (relative to the status area) of the
    /// clickable background indicator spans: the activity text and the
    /// `{key} background` hint only.
    pub(crate) fn render_input_status_line_with_hit(&self, width: u16) -> Option<(Line<'static>, Vec<(u16, u16)>)> {
        self.build_input_status_line(width)
            .map(|status| (status.line, status.background_hits))
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn build_input_status_line(&self, width: u16) -> Option<InputStatusLine> {
        if width == 0 {
            return None;
        }

        let copy_notification = self.copy_notification_text();
        let showing_progress = copy_notification.is_none() && self.progress.is_active() && !self.progress_row_visible();
        // Queued follow-ups are the freshest user action, so they take the
        // composer line ahead of configured context and the generic
        // "Thinking" label — but never ahead of a live phase status.
        //
        // While the transcript owns foreground progress, runtime/stage status
        // text stays suppressed so the line never repeats or reflows the
        // loading row between phases; a queued follow-up is a direct user
        // action rather than background churn, so it still surfaces (and is
        // often the only acknowledgement visible once the loading row has
        // scrolled the queue overlay away).
        let left = copy_notification.or_else(|| {
            self.progress_footer_status_text().map(str::to_owned).or_else(|| {
                (!self.progress.is_active() && !self.footer_context_configured)
                    .then(|| self.status_left_text().map(str::to_owned))
                    .flatten()
            })
        });
        let thinking_spinner_line = || {
            (self.thinking_spinner.is_active && !self.progress.is_active()).then(|| {
                if self.appearance.should_animate_progress_status() {
                    format!("{} Thinking", self.thinking_spinner.current_frame())
                } else {
                    "Thinking".to_owned()
                }
            })
        };
        let left = left.or_else(|| self.queued_input_status_text()).or_else(thinking_spinner_line);
        let configured_right = if self.footer_context_configured {
            self.footer_context_right.as_deref()
        } else {
            self.status_right_text()
        };
        let right = match (configured_right, self.vim_state.status_label()) {
            (Some(existing), Some(vim_label)) => Some(format!("{vim_label} · {existing}")),
            (None, Some(vim_label)) => Some(vim_label.to_string()),
            (existing, None) => existing.map(str::to_owned),
        };
        let mode_pill = self.primary_mode_pill();
        // Bottom-line background affordances are idle-only. While anything is
        // busy — the transcript loading row, a foreground command, or an
        // in-flight turn — the composer line keeps only configured context so
        // it never reflows as progress phases or the PTY counter toggle
        // underneath (the `· Ctrl+B background` flicker after the branch
        // status). The live count moves to the transcript loading row plus a
        // static header badge, and `Ctrl+B` discovery moves to the header
        // suggestions line; the `Ctrl+B`, `Alt+S`, `/jobs`, and empty-Enter
        // entry points keep working while busy. When idle the copy is
        // width-deterministic (budget decides) rather than state-gated, so a
        // fitting count stays put across tool gaps.
        let busy = self.progress.is_active() || self.has_active_foreground_pty() || self.is_running_activity();
        let background_hint = if busy {
            None
        } else {
            self.local_agents_input_status_hint()
        };
        let background_status = if self.progress.is_active() {
            None
        } else {
            self.background_activity_status_text()
        };
        let dim_style = {
            let mut style = self.styles.default_style().add_modifier(Modifier::DIM);
            if let Some(secondary) = self.theme.secondary.or(self.theme.foreground) {
                style = style.fg(ratatui_color_from_ansi(secondary));
            }
            style
        };
        let key_style = {
            let mut style = self.styles.default_style().add_modifier(Modifier::BOLD);
            if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
                style = style.fg(ratatui_color_from_ansi(primary));
            }
            style
        };
        let label_style = {
            let mut style = self.styles.default_style();
            if let Some(secondary) = self.theme.secondary.or(self.theme.foreground) {
                style = style.fg(ratatui_color_from_ansi(secondary));
            }
            style
        };
        // Allocate persistent regions first. Mode has first claim, then the
        // configured right side, then context, then optional activity/hints.
        let mut right_spans = Vec::new();
        if let Some((label, style)) = mode_pill {
            right_spans =
                truncate_line_with_ellipsis_if_overflow(Line::from(Span::styled(label, style)), usize::from(width))
                    .spans;
        }
        let mode_width = Line::from(right_spans.clone()).width() as u16;
        if let Some(value) = right {
            let separator = u16::from(mode_width > 0);
            let budget = width.saturating_sub(mode_width.saturating_add(separator));
            if budget > 0 {
                if separator > 0 {
                    right_spans.push(Span::raw(" "));
                }
                right_spans.extend(
                    truncate_line_with_ellipsis_if_overflow(
                        Line::from(Span::styled(value, dim_style)),
                        usize::from(budget),
                    )
                    .spans,
                );
            }
        }
        let right_width = Line::from(right_spans.clone()).width() as u16;
        let left_budget = width.saturating_sub(right_width.saturating_add(u16::from(right_width > 0)));
        let mut spans = left
            .as_ref()
            .map(|text| {
                if !self.footer_context_configured
                    && status_requires_shimmer(text)
                    && self.appearance.should_animate_progress_status()
                {
                    shimmer_spans_with_style_at_phase(
                        text,
                        self.styles.accent_style().add_modifier(Modifier::DIM),
                        self.shimmer_state.phase(),
                    )
                } else {
                    self.create_git_status_spans(text, dim_style)
                }
            })
            .unwrap_or_default();
        spans = if left_budget == 0 {
            Vec::new()
        } else {
            truncate_line_with_ellipsis_if_overflow(Line::from(spans), usize::from(left_budget)).spans
        };
        let mut background_hits = Vec::new();
        let mut progress_start = 0;
        let mut progress_columns = 0;

        if showing_progress && let Some(text) = self.progress.text() {
            let used = Line::from(spans.clone()).width() as u16;
            let separator = u16::from(used > 0);
            let budget = left_budget.saturating_sub(used + separator).min(24);
            if budget > 1 {
                if separator > 0 {
                    spans.push(Span::raw(" "));
                }
                progress_start = used + separator;
                let truncated = measure_text_width(&text) > budget;
                let style = self.styles.accent_style().add_modifier(Modifier::DIM);
                let progress_spans = if self.progress.is_animated() && self.appearance.should_animate_progress_status()
                {
                    shimmer_spans_with_style_at_phase(&text, style, self.shimmer_state.phase())
                } else {
                    vec![Span::styled(text, dim_style)]
                };
                let line = truncate_line_with_ellipsis_if_overflow(Line::from(progress_spans), usize::from(budget));
                let actual = line.width() as u16;
                progress_columns = actual.saturating_sub(u16::from(truncated));
                spans.extend(line.spans);
                spans.push(Span::raw(" ".repeat(usize::from(budget.saturating_sub(actual)))));
            }
        }
        if let Some(status) = background_status {
            let used = Line::from(spans.clone()).width() as u16;
            let separator = u16::from(used > 0);
            let budget = left_budget.saturating_sub(used + separator * 3);
            if measure_text_width(&status) <= budget {
                let start = used + separator * 3;
                if separator > 0 {
                    spans.push(Span::raw(" · "));
                }
                background_hits.push((start, start + measure_text_width(&status)));
                if self.appearance.should_animate_progress_status() {
                    spans.extend(shimmer_spans_with_style_at_phase(&status, dim_style, self.shimmer_state.phase()));
                } else {
                    spans.push(Span::styled(status, dim_style));
                }
            }
        }
        if let Some(hint) = background_hint {
            let used = Line::from(spans.clone()).width() as u16;
            let mut hint_spans = Vec::new();
            let hit = Self::append_background_hint_spans(
                &mut hint_spans,
                &hint,
                self.background_shortcut_label(),
                dim_style,
                key_style,
                label_style,
            );
            if Line::from(hint_spans.clone()).width() <= usize::from(left_budget.saturating_sub(used)) {
                spans.extend(hint_spans);
                if let Some((start, end)) = hit {
                    background_hits.push((used + start, used + end));
                }
            }
        }
        for hint in [
            self.shell_mode_status_hint().map(str::to_owned),
            self.build_scroll_indicator(),
        ]
        .into_iter()
        .flatten()
        {
            let used = Line::from(spans.clone()).width() as u16;
            let separator = u16::from(used > 0);
            if measure_text_width(&hint) + separator * 3 <= left_budget.saturating_sub(used) {
                if separator > 0 {
                    spans.push(Span::raw(" · "));
                }
                spans.push(Span::styled(hint, dim_style));
            }
        }
        if !right_spans.is_empty() {
            let used = Line::from(spans.clone()).width() as u16;
            spans.push(Span::raw(" ".repeat(usize::from(width.saturating_sub(used + right_width)))));
            spans.extend(right_spans);
        }
        (!spans.is_empty()).then_some(InputStatusLine {
            line: Line::from(spans),
            background_hits,
            progress_start,
            progress_columns,
        })
    }

    fn input_uses_shell_prefix(&self) -> bool {
        self.input_manager.content().trim_start().starts_with('!')
    }

    pub(crate) fn input_block_padding(&self) -> Padding {
        if self.input_uses_shell_prefix() {
            Padding::new(0, 0, 0, 0)
        } else {
            Padding::new(
                ui::INLINE_INPUT_PADDING_HORIZONTAL,
                ui::INLINE_INPUT_PADDING_HORIZONTAL,
                ui::INLINE_INPUT_PADDING_VERTICAL,
                ui::INLINE_INPUT_PADDING_VERTICAL,
            )
        }
    }

    pub(crate) fn shell_mode_border_title(&self) -> Option<&'static str> {
        self.input_uses_shell_prefix().then_some(SHELL_MODE_BORDER_TITLE)
    }

    pub(crate) fn active_subagent_input_title(&self) -> Option<Line<'static>> {
        let badge = self.header_context.subagent_badges.first()?;
        let hidden = self.header_context.subagent_badges.len().saturating_sub(1);
        let label = if hidden == 0 {
            badge.text.clone()
        } else {
            format!("{} +{}", badge.text, hidden)
        };

        let mut style = ratatui_style_from_inline(&badge.style, self.theme.foreground);
        if badge.full_background {
            style = style.add_modifier(Modifier::BOLD);
        }

        Some(Line::from(Span::styled(format!(" {label} "), style)).right_aligned())
    }

    fn active_subagent_input_border_style(&self) -> Option<Style> {
        // Use the primary agent color if available, otherwise fall back to badge color.
        if let Some(color_style) =
            super::super::style::agent_color_style(self.header_context.primary_agent_color.as_deref(), Color::Magenta)
                .fg
        {
            return Some(self.styles.accent_style().fg(color_style).add_modifier(Modifier::BOLD));
        }

        let badge = self.header_context.subagent_badges.first()?;
        let mut title_style = ratatui_style_from_inline(&badge.style, self.theme.foreground);
        if badge.full_background {
            title_style = title_style.add_modifier(Modifier::BOLD);
        }

        let color = if badge.full_background {
            title_style.bg.or(title_style.fg)
        } else {
            title_style.fg.or(title_style.bg)
        }?;

        Some(self.styles.accent_style().fg(color).add_modifier(Modifier::BOLD))
    }

    /// Trimmed primary agent mode name, driving the persistent mode border/pill.
    fn primary_mode_name(&self) -> Option<&str> {
        self.header_context
            .primary_agent
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }

    /// Mode pill for the input status line, reusing the header badge color.
    ///
    /// Returns `None` when no primary agent name is set so the default
    /// (modeless) status line is unchanged. The style resolves through the
    /// shared design-system `agent_color_style`, keeping the four mode hues
    /// distinct on both dark and light terminals.
    /// Resolved primary-agent mode color, sharing the header badge source.
    ///
    /// Returns `None` when no mode name is set so modeless chrome is unchanged.
    fn primary_mode_color(&self) -> Option<Color> {
        self.primary_mode_name()?;
        let fallback = self.theme.primary.map(ratatui_color_from_ansi).unwrap_or(Color::LightMagenta);
        super::super::style::agent_color_style(self.header_context.primary_agent_color.as_deref(), fallback).fg
    }

    fn primary_mode_pill(&self) -> Option<(String, Style)> {
        let name = self.primary_mode_name()?;
        let color = self.primary_mode_color()?;
        let label = super::header::primary_agent_header_label(Some(name));
        let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
        Some((format!("• {label}"), style))
    }

    fn shell_mode_status_hint(&self) -> Option<&'static str> {
        self.input_uses_shell_prefix().then_some(SHELL_MODE_STATUS_HINT)
    }

    fn local_agents_input_status_hint(&self) -> Option<String> {
        if self.input_uses_shell_prefix() || !self.input_manager.content().trim().is_empty() {
            // A foreground PTY keeps its background hint visible even with a
            // non-empty composer so users can discover the background
            // shortcut mid-command.
            return self.foreground_pty_background_hint();
        }

        if !self.has_local_agents() {
            return self.foreground_pty_background_hint();
        }

        Some(format!("↓ or Alt+S local agents · {} background", self.background_shortcut_label()))
    }

    /// Appends the local-agents hint spans. Returns the column range of the
    /// `{key} background` span only (relative to the start of the appended
    /// content) so click hit-testing ignores `Alt+S local agents` and separators.
    fn append_background_hint_spans(
        spans: &mut Vec<Span<'static>>,
        hint: &str,
        key_label: &str,
        dim_style: Style,
        key_style: Style,
        label_style: Style,
    ) -> Option<(u16, u16)> {
        let mut appended_width = 0_u16;
        let mut key_hit: Option<(u16, u16)> = None;
        let mut push = |spans: &mut Vec<Span<'static>>, text: &str, style: Style, mark: bool| {
            let width = measure_text_width(text);
            let start = appended_width;
            let end = appended_width.saturating_add(width);
            if mark {
                key_hit = Some(match key_hit {
                    Some((s, e)) => (s.min(start), e.max(end)),
                    None => (start, end),
                });
            }
            appended_width = end;
            spans.push(Span::styled(text.to_owned(), style));
        };

        // PTY-only hint has the exact shape "{key} background".
        if let Some(prefix) = hint.strip_suffix(" background")
            && prefix == key_label
        {
            if !spans.is_empty() {
                push(spans, " · ", dim_style, false);
            }
            push(spans, key_label, key_style, true);
            push(spans, " background", label_style, true);
            return key_hit;
        }
        // Combined drawer hint has the exact shape
        // "↓ or Alt+S local agents · {key} background".
        if let Some(rest) = hint.strip_prefix("↓ or Alt+S local agents · ")
            && let Some(prefix) = rest.strip_suffix(" background")
            && prefix == key_label
        {
            if !spans.is_empty() {
                push(spans, " · ", dim_style, false);
            }
            push(spans, "↓ or ", label_style, false);
            push(spans, "Alt+S", key_style, false);
            push(spans, " local agents", label_style, false);
            push(spans, " · ", dim_style, false);
            push(spans, key_label, key_style, true);
            push(spans, " background", label_style, true);
            return key_hit;
        }
        if !spans.is_empty() {
            push(spans, " · ", dim_style, false);
        }
        // Unrecognized hint shape: render it dim but do not make it a hit
        // target. Only the known `{key} background` shapes are clickable.
        push(spans, hint, dim_style, false);
        key_hit
    }

    /// Builds the footer scroll indicator.
    ///
    /// Uses the inverted scroll model documented on `ScrollManager`:
    /// offset 0 is the bottom (live) edge and increasing offsets move toward
    /// older content at the top of the transcript.
    ///
    /// - At the bottom: no indicator at all.
    /// - While scrolled: `↑ {visible_top}/{total}` shows the top row position.
    /// - When new lines arrived while scrolled: `↓ {N} new` highlights the
    ///   pending content until the user returns to the bottom.
    /// - When scrolled up with a tracked change: appends
    ///   `⤓ Jump to last change [key]` hint.
    fn build_scroll_indicator(&self) -> Option<String> {
        if !self.user_scrolled {
            return None;
        }

        let pending = self.pending_new_messages;
        let total = self.transcript_rows.max(1) as usize;
        let top = self.scroll_manager.offset().saturating_add(1).min(total);

        let mut label = if pending > 0 {
            format!("↓ {} new", pending)
        } else {
            format!("↑ {}/{}", top, total)
        };
        if self.should_show_jump_to_last_change() {
            let key_label = self.primary_binding_label(Action::JumpToLastChange).unwrap_or("Ctrl+End");
            label.push_str(&format!(" · ⤓ Jump to last change [{key_label}]"));
        }
        Some(label)
    }

    fn create_git_status_spans(&self, text: &str, default_style: Style) -> Vec<Span<'static>> {
        let text = text.strip_prefix(ui::HEADER_GIT_PREFIX).unwrap_or(text).trim_start();
        if let Some((branch_part, indicator_part)) = text.rsplit_once(" | ") {
            let mut spans = Vec::new();
            let branch_trim = branch_part.trim_end();
            if !branch_trim.is_empty() {
                // Status values may already include the header's `git: `
                // label. The footer is the compact view, so omit that label.
                spans.push(Span::styled(branch_trim.to_owned(), default_style));
            }
            spans.push(Span::raw(" "));

            let indicator_trim = indicator_part.trim();
            let indicator_style = if indicator_trim == ui::HEADER_GIT_DIRTY_SUFFIX {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else if indicator_trim == ui::HEADER_GIT_CLEAN_SUFFIX {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
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
        self.modal_state().and_then(|modal| modal.secure_prompt.as_ref()).is_some()
    }

    /// Build input render data for external widgets
    pub(crate) fn build_input_widget_data(&self, width: u16, height: u16) -> InputWidgetData {
        let input_render = self.build_input_render_uncached(width, height);
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
    pub(crate) fn build_input_status_widget_data(&self, width: u16) -> Option<Vec<Span<'static>>> {
        self.render_input_status_line(width).map(|line| line.spans)
    }
}

#[cfg(test)]
mod input_highlight_tests {
    use super::*;

    fn kinds(input: &str) -> Vec<(InputTokenKind, String)> {
        tokenize_input(input)
            .into_iter()
            .map(|t| {
                let text: String = input.chars().skip(t.start).take(t.end - t.start).collect();
                (t.kind, text)
            })
            .collect()
    }

    #[test]
    fn slash_command_at_start() {
        let tokens = kinds("/use skill-name");
        assert_eq!(tokens[0].0, InputTokenKind::SlashCommand);
        assert_eq!(tokens[0].1, "/use");
        assert_eq!(tokens[1].0, InputTokenKind::Normal);
    }

    #[test]
    fn slash_command_with_following_text() {
        let tokens = kinds("/checkup hello");
        assert_eq!(tokens[0].0, InputTokenKind::SlashCommand);
        assert_eq!(tokens[0].1, "/checkup");
        assert_eq!(tokens[1].0, InputTokenKind::Normal);
    }

    #[test]
    fn at_file_reference() {
        let tokens = kinds("check @src/main.rs please");
        assert_eq!(tokens[0].0, InputTokenKind::Normal);
        assert_eq!(tokens[1].0, InputTokenKind::FileReference);
        assert_eq!(tokens[1].1, "@src/main.rs");
        assert_eq!(tokens[2].0, InputTokenKind::Normal);
    }

    #[test]
    fn inline_backtick_code() {
        let tokens = kinds("run `cargo test` now");
        assert_eq!(tokens[0].0, InputTokenKind::Normal);
        assert_eq!(tokens[1].0, InputTokenKind::InlineCode);
        assert_eq!(tokens[1].1, "`cargo test`");
        assert_eq!(tokens[2].0, InputTokenKind::Normal);
    }

    #[test]
    fn no_false_slash_mid_word() {
        let tokens = kinds("path/to/file");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, InputTokenKind::Normal);
    }

    #[test]
    fn empty_input() {
        assert!(tokenize_input("").is_empty());
    }

    #[test]
    fn mixed_tokens() {
        let tokens = kinds("/use @file.rs `code`");
        assert_eq!(tokens[0].0, InputTokenKind::SlashCommand);
        assert_eq!(tokens[2].0, InputTokenKind::FileReference);
        assert_eq!(tokens[4].0, InputTokenKind::InlineCode);
    }

    #[test]
    fn agent_reference_has_dedicated_token_kind() {
        let tokens = kinds("use @agent-explorer for this");
        assert_eq!(tokens[1].0, InputTokenKind::AgentReference);
        assert_eq!(tokens[1].1, "@agent-explorer");
    }

    #[test]
    fn byte_index_to_char_index_is_char_boundary_safe() {
        // Multi-byte content: "a→b" is [a, →(3 bytes), b] = 5 bytes.
        let content = "a→b";
        assert_eq!(byte_index_to_char_index(content, 0), 0);
        // Byte indices inside the 3-byte '→' clamp to its start (char 1).
        assert_eq!(byte_index_to_char_index(content, 2), 1);
        // Byte index 4 is the start of 'b' (char 2).
        assert_eq!(byte_index_to_char_index(content, 4), 2);
        // Out-of-range indices clamp to the end (3 chars).
        assert_eq!(byte_index_to_char_index(content, 99), 3);
    }

    #[test]
    fn plugin_agent_reference_has_dedicated_token_kind() {
        let tokens = kinds("use @agent-github:reviewer for this");
        assert_eq!(tokens[1].0, InputTokenKind::AgentReference);
        assert_eq!(tokens[1].1, "@agent-github:reviewer");
    }
}
