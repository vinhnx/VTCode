use std::fmt::Write;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use unicode_segmentation::UnicodeSegmentation;

use crate::config::constants::ui;

use super::super::types::{InlineHeaderContext, InlineHeaderHighlight};
use super::{PROMPT_COMMAND_NAME, Session, ratatui_color_from_ansi};

impl Session {
    pub(super) fn render_header(&self, frame: &mut Frame<'_>, area: Rect, lines: &[Line<'static>]) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        let paragraph = self.build_header_paragraph(lines);

        frame.render_widget(paragraph, area);
    }

    pub(super) fn header_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![self.header_title_line(), self.header_meta_line()];

        // Prioritize suggestions when input is empty or starts with /
        if self.should_show_suggestions() {
            if let Some(suggestions) = self.header_suggestions_line() {
                lines.push(suggestions);
            }
        } else if let Some(highlights) = self.header_highlights_line() {
            lines.push(highlights);
        }

        lines.truncate(3);
        lines
    }

    pub(super) fn header_height_from_lines(&self, width: u16, lines: &[Line<'static>]) -> u16 {
        if width == 0 {
            return self.header_rows.max(ui::INLINE_HEADER_HEIGHT);
        }

        let paragraph = self.build_header_paragraph(lines);
        let measured = paragraph.line_count(width);
        let resolved = u16::try_from(measured).unwrap_or(u16::MAX);
        // Limit to max 3 lines to accommodate suggestions
        resolved.min(3).max(ui::INLINE_HEADER_HEIGHT)
    }

    pub(super) fn build_header_paragraph(&self, lines: &[Line<'static>]) -> Paragraph<'static> {
        let block = Block::default()
            .title(self.header_block_title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style());

        Paragraph::new(lines.to_vec())
            .style(self.default_style())
            .wrap(Wrap { trim: true })
            .block(block)
    }

    #[cfg(test)]
    pub(super) fn header_height_for_width(&self, width: u16) -> u16 {
        let lines = self.header_lines();
        self.header_height_from_lines(width, &lines)
    }

    pub fn header_block_title(&self) -> Line<'static> {
        let fallback = InlineHeaderContext::default();
        let version = if self.header_context.version.trim().is_empty() {
            fallback.version
        } else {
            self.header_context.version.clone()
        };

        let prompt = format!(
            "{}{} ",
            ui::HEADER_VERSION_PROMPT,
            ui::HEADER_VERSION_PREFIX
        );
        let version_text = format!(
            "{}{}{}",
            ui::HEADER_VERSION_LEFT_DELIMITER,
            version.trim(),
            ui::HEADER_VERSION_RIGHT_DELIMITER
        );

        let prompt_style = self.section_title_style();
        let version_style = self.header_secondary_style().add_modifier(Modifier::DIM);

        Line::from(vec![
            Span::styled(prompt, prompt_style),
            Span::styled(version_text, version_style),
        ])
    }

    pub fn header_title_line(&self) -> Line<'static> {
        // First line: badge-style provider + model + reasoning summary
        let mut spans = Vec::new();

        let provider = self.header_provider_short_value();
        let model = self.header_model_short_value();
        let reasoning = self.header_reasoning_short_value();

        if !provider.is_empty() {
            let badge = format!("[{}]", provider.to_uppercase());
            let mut style = self.header_primary_style();
            style = style.add_modifier(Modifier::BOLD);
            spans.push(Span::styled(badge, style));
        }

        if !model.is_empty() {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let mut style = self.header_primary_style();
            style = style.add_modifier(Modifier::ITALIC);
            spans.push(Span::styled(model, style));
        }

        if !reasoning.is_empty() {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let mut style = self.header_secondary_style();
            style = style.add_modifier(Modifier::ITALIC | Modifier::DIM);
            spans.push(Span::styled(format!("({})", reasoning), style));
        }

        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }

        Line::from(spans)
    }

    fn header_provider_value(&self) -> String {
        let trimmed = self.header_context.provider.trim();
        if trimmed.is_empty() {
            InlineHeaderContext::default().provider
        } else {
            self.header_context.provider.clone()
        }
    }

    fn header_model_value(&self) -> String {
        let trimmed = self.header_context.model.trim();
        if trimmed.is_empty() {
            InlineHeaderContext::default().model
        } else {
            self.header_context.model.clone()
        }
    }

    fn header_mode_label(&self) -> String {
        let trimmed = self.header_context.mode.trim();
        if trimmed.is_empty() {
            InlineHeaderContext::default().mode
        } else {
            self.header_context.mode.clone()
        }
    }

    pub fn header_mode_short_label(&self) -> String {
        let full = self.header_mode_label();
        let value = full.trim();
        if value.eq_ignore_ascii_case(ui::HEADER_MODE_AUTO) {
            return "Auto".to_string();
        }
        if value.eq_ignore_ascii_case(ui::HEADER_MODE_INLINE) {
            return "Inline".to_string();
        }
        if value.eq_ignore_ascii_case(ui::HEADER_MODE_ALTERNATE) {
            return "Alternate".to_string();
        }
        let compact = value
            .strip_suffix(ui::HEADER_MODE_FULL_AUTO_SUFFIX)
            .unwrap_or(value)
            .trim();
        compact.to_string()
    }

    fn header_reasoning_value(&self) -> Option<String> {
        let trimmed = self.header_context.reasoning.trim();
        let value = if trimmed.is_empty() {
            InlineHeaderContext::default().reasoning
        } else {
            self.header_context.reasoning.clone()
        };
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    }

    pub fn header_provider_short_value(&self) -> String {
        let value = self.header_provider_value();
        Self::strip_prefix(&value, ui::HEADER_PROVIDER_PREFIX)
            .trim()
            .to_string()
    }

    pub fn header_model_short_value(&self) -> String {
        let value = self.header_model_value();
        Self::strip_prefix(&value, ui::HEADER_MODEL_PREFIX)
            .trim()
            .to_string()
    }

    pub fn header_reasoning_short_value(&self) -> String {
        let value = self.header_reasoning_value().unwrap_or_else(String::new);
        Self::strip_prefix(&value, ui::HEADER_REASONING_PREFIX)
            .trim()
            .to_string()
    }

    pub fn header_chain_values(&self) -> Vec<String> {
        let defaults = InlineHeaderContext::default();
        let fields = [
            (
                &self.header_context.workspace_trust,
                defaults.workspace_trust.clone(),
            ),
            (&self.header_context.tools, defaults.tools.clone()),
            (&self.header_context.git, defaults.git.clone()),
            // Removed MCP info from header as requested
        ];

        fields
            .into_iter()
            .filter_map(|(value, fallback)| {
                let mut selected = if value.trim().is_empty() {
                    fallback
                } else {
                    value.clone()
                };
                let trimmed = selected.trim();
                if trimmed.is_empty() {
                    return None;
                }

                if let Some(body) = trimmed.strip_prefix(ui::HEADER_TRUST_PREFIX) {
                    selected = format!("Trust {}", body.trim());
                    return Some(selected);
                }

                if let Some(body) = trimmed.strip_prefix(ui::HEADER_TOOLS_PREFIX) {
                    selected = format!("Tools: {}", body.trim());
                    return Some(selected);
                }

                if let Some(body) = trimmed.strip_prefix(ui::HEADER_GIT_PREFIX) {
                    let body = body.trim();
                    if body.is_empty() {
                        return None;
                    }
                    selected = body.to_string();
                    return Some(selected);
                }

                Some(selected)
            })
            .collect()
    }

    pub fn header_meta_line(&self) -> Line<'static> {
        let mut spans = Vec::new();

        let mut first_section = true;
        let mode_label = self.header_mode_short_label();
        if !mode_label.trim().is_empty() {
            spans.push(Span::styled(
                mode_label,
                self.header_primary_style().add_modifier(Modifier::BOLD),
            ));
            first_section = false;
        }

        for value in self.header_chain_values() {
            if !first_section {
                spans.push(Span::styled(
                    ui::HEADER_MODE_SECONDARY_SEPARATOR.to_string(),
                    self.header_secondary_style(),
                ));
            }
            spans.push(Span::styled(value, self.header_primary_style()));
            first_section = false;
        }

        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }

        Line::from(spans)
    }

    fn header_highlights_line(&self) -> Option<Line<'static>> {
        let mut spans = Vec::new();
        let mut first_section = true;

        for highlight in &self.header_context.highlights {
            let title = highlight.title.trim();
            let summary = self.header_highlight_summary(highlight);

            if title.is_empty() && summary.is_none() {
                continue;
            }

            if !first_section {
                spans.push(Span::styled(
                    ui::HEADER_META_SEPARATOR.to_string(),
                    self.header_secondary_style(),
                ));
            }

            if !title.is_empty() {
                let mut title_style = self.header_secondary_style();
                title_style = title_style.add_modifier(Modifier::BOLD);
                let mut title_text = title.to_string();
                if summary.is_some() {
                    title_text.push(':');
                }
                spans.push(Span::styled(title_text, title_style));
                if summary.is_some() {
                    spans.push(Span::styled(" ".to_string(), self.header_secondary_style()));
                }
            }

            if let Some(body) = summary {
                spans.push(Span::styled(body, self.header_primary_style()));
            }

            first_section = false;
        }

        if spans.is_empty() {
            None
        } else {
            Some(Line::from(spans))
        }
    }

    fn header_highlight_summary(&self, highlight: &InlineHeaderHighlight) -> Option<String> {
        let entries: Vec<String> = highlight
            .lines
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| {
                let stripped = line
                    .strip_prefix("- ")
                    .or_else(|| line.strip_prefix("• "))
                    .unwrap_or(line);
                stripped.trim().to_string()
            })
            .collect();

        if entries.is_empty() {
            return None;
        }

        Some(self.compact_highlight_entries(&entries))
    }

    fn compact_highlight_entries(&self, entries: &[String]) -> String {
        let mut summary =
            self.truncate_highlight_preview(entries.first().map(String::as_str).unwrap_or(""));
        if entries.len() > 1 {
            let remaining = entries.len() - 1;
            if !summary.is_empty() {
                let _ = write!(summary, " (+{} more)", remaining);
            } else {
                summary = format!("(+{} more)", remaining);
            }
        }
        summary
    }

    fn truncate_highlight_preview(&self, text: &str) -> String {
        let max = ui::HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS;
        if max == 0 {
            return String::new();
        }

        let grapheme_count = text.graphemes(true).count();
        if grapheme_count <= max {
            return text.to_string();
        }

        let mut truncated = String::new();
        for grapheme in text.graphemes(true).take(max.saturating_sub(1)) {
            truncated.push_str(grapheme);
        }
        truncated.push_str(ui::INLINE_PREVIEW_ELLIPSIS);
        truncated
    }

    /// Determine if suggestions should be shown in the header
    fn should_show_suggestions(&self) -> bool {
        // Show suggestions when input is empty or starts with /
        self.input.is_empty() || self.input.starts_with('/')
    }

    /// Generate header line with slash command and keyboard shortcut suggestions
    fn header_suggestions_line(&self) -> Option<Line<'static>> {
        let mut spans = Vec::new();

        spans.push(Span::styled(
            "/help",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            " · ",
            self.header_secondary_style().add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(
            format!("/{}", PROMPT_COMMAND_NAME),
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            " · ",
            self.header_secondary_style().add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(
            "/model",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            "  |  ",
            self.header_secondary_style().add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(
            "↑↓",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" Nav · ", self.header_secondary_style()));
        spans.push(Span::styled(
            "Tab",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" Complete", self.header_secondary_style()));

        Some(Line::from(spans))
    }

    pub(super) fn section_title_style(&self) -> Style {
        let mut style = self.default_style().add_modifier(Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    fn header_primary_style(&self) -> Style {
        let mut style = self.default_style();
        if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    pub(super) fn header_secondary_style(&self) -> Style {
        let mut style = self.default_style();
        if let Some(secondary) = self.theme.secondary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(secondary));
        }
        style
    }

    pub(super) fn suggestion_block_title(&self) -> Line<'static> {
        Line::from(vec![Span::styled(
            ui::SUGGESTION_BLOCK_TITLE.to_string(),
            self.section_title_style(),
        )])
    }

    fn strip_prefix<'a>(value: &'a str, prefix: &str) -> &'a str {
        value.strip_prefix(prefix).unwrap_or(value)
    }
}
