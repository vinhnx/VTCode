use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::state::RatatuiTheme;

pub(crate) struct PtyBlockBuilder {
    indent_text: String,
    inner_width: usize,
    border_style: Style,
    content_style: Style,
    title: Option<String>,
}

impl PtyBlockBuilder {
    pub(crate) fn new(theme: &RatatuiTheme, indent: usize, inner_width: usize) -> Self {
        let border_color = theme
            .secondary
            .or(theme.primary)
            .unwrap_or(Color::LightCyan);
        let content_color = theme.foreground.unwrap_or(Color::Gray);
        Self {
            indent_text: " ".repeat(indent),
            inner_width,
            border_style: Style::default().fg(border_color),
            content_style: Style::default().fg(content_color),
            title: None,
        }
    }

    pub(crate) fn title(mut self, title: String) -> Self {
        if title.is_empty() {
            self.title = None;
        } else {
            self.title = Some(title);
        }
        self
    }

    pub(crate) fn build(self, mut body: Vec<Line<'static>>) -> Vec<Line<'static>> {
        if body.is_empty() {
            body.push(Line::default());
        }
        let mut lines = Vec::with_capacity(body.len() + 2);
        lines.push(self.build_top_line());
        for line in body {
            lines.push(self.build_body_line(line));
        }
        lines.push(self.build_bottom_line());
        lines
    }

    pub(crate) fn build_top_line(&self) -> Line<'static> {
        let fill = self
            .title
            .clone()
            .unwrap_or_else(|| "─".repeat(self.inner_width));
        Line::from(vec![
            Span::raw(self.indent_text.clone()),
            Span::styled("╭".to_string(), self.border_style),
            Span::styled(fill, self.border_style),
            Span::styled("╮".to_string(), self.border_style),
        ])
    }

    pub(crate) fn build_bottom_line(&self) -> Line<'static> {
        Line::from(vec![
            Span::raw(self.indent_text.clone()),
            Span::styled("╰".to_string(), self.border_style),
            Span::styled("─".repeat(self.inner_width), self.border_style),
            Span::styled("╯".to_string(), self.border_style),
        ])
    }

    pub(crate) fn build_body_line(&self, mut line: Line<'static>) -> Line<'static> {
        let width_used = line
            .spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>();
        let mut spans = Vec::with_capacity(line.spans.len() + 4);
        spans.push(Span::raw(self.indent_text.clone()));
        spans.push(Span::styled("│".to_string(), self.border_style));
        spans.append(&mut line.spans);
        if width_used < self.inner_width {
            spans.push(Span::styled(
                " ".repeat(self.inner_width - width_used),
                self.content_style,
            ));
        }
        spans.push(Span::styled("│".to_string(), self.border_style));
        Line::from(spans)
    }
}
