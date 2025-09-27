use std::cmp::min;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::ui::tui::{
    action::ScrollAction,
    types::{RatatuiMessageKind, RatatuiSegment, RatatuiTextStyle, RatatuiTheme},
};

const USER_PREFIX: &str = "> ";
const STATUS_PREFIX: &str = "● ";

#[derive(Clone)]
struct MessageLine {
    kind: RatatuiMessageKind,
    segments: Vec<RatatuiSegment>,
}

#[derive(Clone, Default)]
struct MessageLabels {
    agent: Option<String>,
    user: Option<String>,
}

pub struct TranscriptView {
    lines: Vec<MessageLine>,
    theme: RatatuiTheme,
    labels: MessageLabels,
    scroll_offset: usize,
    viewport_height: usize,
}

impl TranscriptView {
    pub fn new(theme: RatatuiTheme) -> Self {
        Self {
            lines: Vec::new(),
            theme,
            labels: MessageLabels::default(),
            scroll_offset: 0,
            viewport_height: 1,
        }
    }

    pub fn set_theme(&mut self, theme: RatatuiTheme) {
        self.theme = theme;
    }

    pub fn set_labels(&mut self, agent: Option<String>, user: Option<String>) {
        self.labels.agent = agent.filter(|label| !label.is_empty());
        self.labels.user = user.filter(|label| !label.is_empty());
    }

    pub fn push_line(&mut self, kind: RatatuiMessageKind, segments: Vec<RatatuiSegment>) {
        if self.scroll_offset > 0 {
            self.scroll_offset = min(self.scroll_offset + 1, self.lines.len() + 1);
        }
        self.lines.push(MessageLine { kind, segments });
        self.enforce_scroll_bounds();
    }

    pub fn append_inline(&mut self, kind: RatatuiMessageKind, segment: RatatuiSegment) {
        let mut remaining = segment.text.as_str();
        let style = segment.style.clone();

        while !remaining.is_empty() {
            if let Some((index, control)) = remaining
                .char_indices()
                .find(|(_, ch)| matches!(ch, '\n' | '\r'))
            {
                let (text, _) = remaining.split_at(index);
                if !text.is_empty() {
                    self.append_text(kind, text, &style);
                }

                let control_char = control;
                let next_index = index + control_char.len_utf8();
                remaining = &remaining[next_index..];

                match control_char {
                    '\n' => self.start_line(kind),
                    '\r' => {
                        if remaining.starts_with('\n') {
                            remaining = &remaining[1..];
                            self.start_line(kind);
                        } else {
                            self.reset_line(kind);
                        }
                    }
                    _ => {}
                }
            } else {
                if !remaining.is_empty() {
                    self.append_text(kind, remaining, &style);
                }
                break;
            }
        }

        self.enforce_scroll_bounds();
    }

    pub fn replace_last(
        &mut self,
        count: usize,
        kind: RatatuiMessageKind,
        lines: Vec<Vec<RatatuiSegment>>,
    ) {
        let remove_count = min(count, self.lines.len());
        for _ in 0..remove_count {
            self.lines.pop();
        }
        for segments in lines {
            self.lines.push(MessageLine { kind, segments });
        }
        self.enforce_scroll_bounds();
    }

    pub fn scroll(&mut self, action: ScrollAction) {
        match action {
            ScrollAction::LineUp => self.scroll_line_up(),
            ScrollAction::LineDown => self.scroll_line_down(),
            ScrollAction::PageUp => self.scroll_page_up(),
            ScrollAction::PageDown => self.scroll_page_down(),
        }
    }

    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.viewport_height = area.height.max(1) as usize;
        self.enforce_scroll_bounds();

        let mut paragraph = Paragraph::new(self.visible_lines()).wrap(Wrap { trim: false });
        if let Some(bg) = self.theme.background {
            paragraph = paragraph.style(Style::default().bg(bg));
        }

        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);
    }

    fn visible_lines(&self) -> Vec<Line<'static>> {
        if self.lines.is_empty() {
            return vec![Line::from(String::new())];
        }

        let total = self.lines.len();
        let end = total.saturating_sub(self.scroll_offset);
        let height = self.viewport_height.max(1);
        let start = end.saturating_sub(height);

        self.lines[start..end]
            .iter()
            .map(|line| self.render_line(line))
            .collect()
    }

    fn render_line(&self, line: &MessageLine) -> Line<'static> {
        let mut spans: Vec<Span> = Vec::new();
        if let Some(prefix) = self.prefix_span(line) {
            spans.push(prefix);
        }

        if line.segments.is_empty() {
            spans.push(Span::raw(String::new()));
        } else {
            let fallback = self.text_fallback(line.kind);
            for segment in &line.segments {
                let style = segment.style.to_style(fallback.or(self.theme.foreground));
                spans.push(Span::styled(segment.text.clone(), style));
            }
        }

        Line::from(spans)
    }

    fn prefix_span(&self, line: &MessageLine) -> Option<Span<'static>> {
        let text = self.prefix_text(line.kind)?;
        let style = self.prefix_style(line);
        Some(Span::styled(text, style))
    }

    fn prefix_text(&self, kind: RatatuiMessageKind) -> Option<String> {
        match kind {
            RatatuiMessageKind::User => Some(
                self.labels
                    .user
                    .clone()
                    .unwrap_or_else(|| USER_PREFIX.to_string()),
            ),
            RatatuiMessageKind::Agent | RatatuiMessageKind::Policy => Some(
                self.labels
                    .agent
                    .clone()
                    .unwrap_or_else(|| STATUS_PREFIX.to_string()),
            ),
            RatatuiMessageKind::Tool | RatatuiMessageKind::Pty | RatatuiMessageKind::Error => {
                Some(STATUS_PREFIX.to_string())
            }
            RatatuiMessageKind::Info => None,
        }
    }

    fn prefix_style(&self, line: &MessageLine) -> Style {
        let fallback = self
            .text_fallback(line.kind)
            .or(self.theme.foreground)
            .unwrap_or(Color::White);
        let color = line
            .segments
            .iter()
            .find_map(|segment| segment.style.color)
            .unwrap_or(fallback);
        Style::default().fg(color)
    }

    fn text_fallback(&self, kind: RatatuiMessageKind) -> Option<Color> {
        match kind {
            RatatuiMessageKind::Agent | RatatuiMessageKind::Policy => {
                self.theme.primary.or(self.theme.foreground)
            }
            RatatuiMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            RatatuiMessageKind::Tool | RatatuiMessageKind::Pty | RatatuiMessageKind::Error => {
                self.theme.primary.or(self.theme.foreground)
            }
            RatatuiMessageKind::Info => self.theme.foreground,
        }
    }

    fn append_text(&mut self, kind: RatatuiMessageKind, text: &str, style: &RatatuiTextStyle) {
        if text.is_empty() {
            return;
        }

        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                if let Some(last) = line.segments.last_mut() {
                    if last.style == *style {
                        last.text.push_str(text);
                        return;
                    }
                }
                line.segments.push(RatatuiSegment {
                    text: text.to_string(),
                    style: style.clone(),
                });
                return;
            }
        }

        self.lines.push(MessageLine {
            kind,
            segments: vec![RatatuiSegment {
                text: text.to_string(),
                style: style.clone(),
            }],
        });
    }

    fn start_line(&mut self, kind: RatatuiMessageKind) {
        self.lines.push(MessageLine {
            kind,
            segments: Vec::new(),
        });
    }

    fn reset_line(&mut self, kind: RatatuiMessageKind) {
        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                line.segments.clear();
                return;
            }
        }
        self.start_line(kind);
    }

    fn scroll_line_up(&mut self) {
        if self.scroll_offset < self.lines.len() {
            self.scroll_offset += 1;
        }
    }

    fn scroll_line_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_page_up(&mut self) {
        let page = self.viewport_height.max(1);
        self.scroll_offset = min(self.scroll_offset + page, self.lines.len());
    }

    fn scroll_page_down(&mut self) {
        let page = self.viewport_height.max(1);
        if self.scroll_offset > page {
            self.scroll_offset -= page;
        } else {
            self.scroll_offset = 0;
        }
    }

    fn enforce_scroll_bounds(&mut self) {
        if self.scroll_offset > self.lines.len() {
            self.scroll_offset = self.lines.len();
        }
    }
}
