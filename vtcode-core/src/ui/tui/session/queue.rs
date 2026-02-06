use super::Session;
use ratatui::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) struct QueueOverlay {
    pub(super) width: u16,
    pub(super) version: u64,
    pub(super) lines: Vec<Line<'static>>,
}

impl Session {
    pub(super) fn set_queued_inputs_entries(&mut self, entries: Vec<String>) {
        self.queued_inputs = entries;
        self.invalidate_queue_overlay();
    }

    pub(super) fn push_queued_input(&mut self, entry: String) {
        self.queued_inputs.push(entry);
        self.invalidate_queue_overlay();
    }

    pub(super) fn pop_latest_queued_input(&mut self) -> Option<String> {
        let result = self.queued_inputs.pop();
        if result.is_some() {
            self.invalidate_queue_overlay();
        }
        result
    }

    pub(super) fn queue_input_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 || self.queued_inputs.is_empty() {
            return Vec::new();
        }

        let max_width = width as usize;
        let mut lines = Vec::new();
        let mut prefix_style = self.styles.accent_style();
        prefix_style = prefix_style.add_modifier(Modifier::BOLD);
        let message_style = self.styles.default_style();

        let prefix = "↳ ";
        let prefix_width = UnicodeWidthStr::width(prefix);
        let available = max_width.saturating_sub(prefix_width);

        for entry in self.queued_inputs.iter().rev().take(2) {
            let trimmed = truncate_to_width(entry, available);
            let mut spans = Vec::new();
            spans.push(Span::styled(prefix.to_owned(), prefix_style));
            spans.push(Span::styled(trimmed, message_style));
            lines.push(Line::from(spans));
        }

        let hint = if cfg!(target_os = "macos") {
            "⌥ + ↑ edit"
        } else {
            "Alt + ↑ edit"
        };
        let muted_style = self.styles.default_style().add_modifier(Modifier::DIM);
        lines.push(Line::from(vec![Span::styled(
            hint.to_string(),
            muted_style,
        )]));

        lines
    }

    pub(super) fn invalidate_queue_overlay(&mut self) {
        self.queue_overlay_version = self.queue_overlay_version.wrapping_add(1);
        self.queue_overlay_cache = None;
    }

    pub(super) fn queue_overlay_lines(&mut self, width: u16) -> Option<&[Line<'static>]> {
        if width == 0 || self.queued_inputs.is_empty() {
            self.queue_overlay_cache = None;
            return None;
        }

        let version = self.queue_overlay_version;
        let needs_rebuild = match &self.queue_overlay_cache {
            Some(cache) => cache.width != width || cache.version != version,
            None => true,
        };

        if needs_rebuild {
            let lines = self.reflow_queue_lines(width);
            self.queue_overlay_cache = Some(QueueOverlay {
                width,
                version,
                lines,
            });
        }

        self.queue_overlay_cache.as_ref().and_then(|cache| {
            if cache.lines.is_empty() {
                None
            } else {
                Some(cache.lines.as_slice())
            }
        })
    }

    pub(crate) fn overlay_queue_lines(
        &mut self,
        visible_lines: &mut [Line<'static>],
        content_width: u16,
    ) {
        if visible_lines.is_empty() || content_width == 0 {
            return;
        }

        let Some(queue_lines) = self.queue_overlay_lines(content_width) else {
            return;
        };

        let queue_visible = queue_lines.len().min(visible_lines.len());
        let start = visible_lines.len().saturating_sub(queue_visible);
        let slice_start = queue_lines.len().saturating_sub(queue_visible);
        let overlay = &queue_lines[slice_start..];
        for (target, source) in visible_lines[start..].iter_mut().zip(overlay.iter()) {
            *target = source.clone();
        }
    }

    fn reflow_queue_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 || self.queued_inputs.is_empty() {
            return Vec::new();
        }

        let max_width = width as usize;
        let mut lines = Vec::new();
        let mut prefix_style = self.styles.accent_style();
        prefix_style = prefix_style.add_modifier(Modifier::BOLD);
        let message_style = self.styles.default_style();
        let muted_style = self.styles.default_style().add_modifier(Modifier::DIM);

        const DISPLAY_LIMIT: usize = 5;
        for entry in self.queued_inputs.iter().take(DISPLAY_LIMIT) {
            let label = "  ↳ ";
            let mut message_lines =
                self.wrap_queue_message(label, entry, max_width, prefix_style, message_style);
            if message_lines.is_empty() {
                message_lines.push(Line::default());
            }
            lines.append(&mut message_lines);
        }

        let remaining = self.queued_inputs.len().saturating_sub(DISPLAY_LIMIT);
        if remaining > 0 {
            let indicator = format!("  +{} more", remaining);
            let mut indicator_lines =
                self.wrap_line(Line::from(indicator).style(muted_style), max_width);
            if indicator_lines.is_empty() {
                indicator_lines.push(Line::default());
            }
            lines.extend(indicator_lines);
        }

        let hint = if cfg!(target_os = "macos") {
            "⌥ + ↑ edit"
        } else {
            "Alt + ↑ edit"
        };
        let hint_line = Line::from(vec![Span::styled(format!("  {}", hint), muted_style)]);
        lines.push(hint_line);

        lines
    }

    fn wrap_queue_message(
        &self,
        label: &str,
        message: &str,
        max_width: usize,
        label_style: Style,
        message_style: Style,
    ) -> Vec<Line<'static>> {
        if max_width == 0 {
            return Vec::new();
        }

        let label_width = UnicodeWidthStr::width(label);
        if max_width <= label_width {
            let mut wrapped_label =
                self.wrap_line(Line::from(label.to_owned()).style(label_style), max_width);
            if wrapped_label.is_empty() {
                wrapped_label.push(Line::default());
            }
            return wrapped_label;
        }

        let available = max_width - label_width;
        let mut wrapped = self.wrap_line(
            Line::from(message.to_owned()).style(message_style),
            available,
        );
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        let mut lines = Vec::with_capacity(wrapped.len());
        for (line_index, mut line) in wrapped.into_iter().enumerate() {
            let prefix = if line_index == 0 {
                label.to_owned()
            } else {
                " ".repeat(label_width)
            };
            let mut spans = Vec::new();
            spans.push(Span::styled(prefix, label_style));
            spans.append(&mut line.spans);
            lines.push(Line::from(spans));
        }

        lines
    }
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let text_width = UnicodeWidthStr::width(text);
    if text_width <= max_width {
        return text.to_string();
    }

    let ellipsis = "...";
    let ellipsis_width = 3;
    let target = max_width.saturating_sub(ellipsis_width);
    let mut out = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > target {
            break;
        }
        out.push(ch);
        width += ch_width;
    }

    if width + ellipsis_width <= max_width {
        out.push_str(ellipsis);
    }

    out
}
