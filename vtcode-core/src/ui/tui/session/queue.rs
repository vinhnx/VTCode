use super::Session;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

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
        let needs_rebuild = self.queue_overlay_cache.as_ref().map_or(true, |cache| {
            cache.width != width || cache.version != version
        });

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

    pub(super) fn overlay_queue_lines(
        &mut self,
        visible_lines: &mut Vec<Line<'static>>,
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
        let mut header_style = self.accent_style();
        header_style = header_style.add_modifier(Modifier::BOLD);
        let message_style = self.default_style();

        let header_text = if self.queued_inputs.len() == 1 {
            "Queued message".to_string()
        } else {
            format!("Queued messages ({})", self.queued_inputs.len())
        };

        let mut header_lines = self.wrap_line(
            Line::from(vec![Span::styled(header_text, header_style)]),
            max_width,
        );
        if header_lines.is_empty() {
            header_lines.push(Line::default());
        }
        lines.extend(header_lines);

        const DISPLAY_LIMIT: usize = 2;
        for (index, entry) in self.queued_inputs.iter().take(DISPLAY_LIMIT).enumerate() {
            let label = format!("  {}. ", index + 1);
            let mut message_lines =
                self.wrap_queue_message(&label, entry, max_width, header_style, message_style);
            if message_lines.is_empty() {
                message_lines.push(Line::default());
            }
            lines.append(&mut message_lines);
        }

        let remaining = self.queued_inputs.len().saturating_sub(DISPLAY_LIMIT);
        if remaining > 0 {
            let indicator = format!("  +{}...", remaining);
            let mut indicator_lines = self.wrap_line(
                Line::from(vec![Span::styled(indicator, message_style)]),
                max_width,
            );
            if indicator_lines.is_empty() {
                indicator_lines.push(Line::default());
            }
            lines.extend(indicator_lines);
        }

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
            let mut wrapped_label = self.wrap_line(
                Line::from(vec![Span::styled(label.to_string(), label_style)]),
                max_width,
            );
            if wrapped_label.is_empty() {
                wrapped_label.push(Line::default());
            }
            return wrapped_label;
        }

        let available = max_width - label_width;
        let mut wrapped = self.wrap_line(
            Line::from(vec![Span::styled(message.to_string(), message_style)]),
            available,
        );
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        let mut lines = Vec::with_capacity(wrapped.len());
        for (line_index, mut line) in wrapped.into_iter().enumerate() {
            let prefix = if line_index == 0 {
                label.to_string()
            } else {
                " ".repeat(label_width)
            };
            let mut spans = Vec::new();
            spans.push(Span::styled(prefix, label_style));
            spans.extend(line.spans.drain(..));
            lines.push(Line::from(spans));
        }

        lines
    }
}
