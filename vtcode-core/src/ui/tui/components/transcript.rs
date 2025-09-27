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

#[derive(Clone)]
struct MessageLine {
    kind: RatatuiMessageKind,
    segments: Vec<RatatuiSegment>,
}

const USER_PREFIX: &str = "> ";
const STATUS_DOT_PREFIX: &str = "● ";

pub struct Transcript {
    lines: Vec<MessageLine>,
    theme: RatatuiTheme,
    scroll_offset: usize,
    viewport_height: usize,
}

impl Transcript {
    pub fn new(theme: RatatuiTheme) -> Self {
        Self {
            lines: Vec::new(),
            theme,
            scroll_offset: 0,
            viewport_height: 1,
        }
    }

    pub fn set_theme(&mut self, theme: RatatuiTheme) {
        self.theme = theme;
    }

    pub fn set_labels(&mut self, _agent: Option<String>, _user: Option<String>) {}

    pub fn push_line(&mut self, kind: RatatuiMessageKind, segments: Vec<RatatuiSegment>) {
        if self.scroll_offset > 0 {
            self.scroll_offset = min(self.scroll_offset + 1, self.lines.len() + 1);
        }
        self.lines.push(MessageLine { kind, segments });
        self.trim_scroll_bounds();
    }

    pub fn append_inline(&mut self, kind: RatatuiMessageKind, segment: RatatuiSegment) {
        let mut pieces = segment.text.split('\n').peekable();
        let style = segment.style.clone();
        let mut first_piece = true;

        while let Some(piece) = pieces.next() {
            let is_last_piece = pieces.peek().is_none();

            if first_piece {
                if let Some(last) = self.lines.last_mut() {
                    if last.kind == kind {
                        if !piece.is_empty() {
                            last.segments.push(RatatuiSegment {
                                text: piece.to_string(),
                                style: style.clone(),
                            });
                        }
                    } else {
                        self.push_line(kind, Self::segments_from_text(&style, piece));
                    }
                } else {
                    self.push_line(kind, Self::segments_from_text(&style, piece));
                }
                first_piece = false;
            } else {
                let mut appended = false;
                if let Some(last) = self.lines.last_mut() {
                    if last.kind == kind && last.segments.is_empty() {
                        if !piece.is_empty() {
                            last.segments.push(RatatuiSegment {
                                text: piece.to_string(),
                                style: style.clone(),
                            });
                        }
                        appended = true;
                    }
                }
                if !appended {
                    self.push_line(kind, Self::segments_from_text(&style, piece));
                }
            }

            if !is_last_piece {
                self.push_line(kind, Vec::new());
            }
        }

        self.trim_scroll_bounds();
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
        self.trim_scroll_bounds();
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
        self.set_viewport_height(area.height as usize);
        let mut paragraph = Paragraph::new(self.visible_lines()).wrap(Wrap { trim: false });
        if let Some(bg) = self.theme.background {
            paragraph = paragraph.style(Style::default().bg(bg));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(paragraph, area);
    }

    fn segments_from_text(style: &RatatuiTextStyle, text: &str) -> Vec<RatatuiSegment> {
        if text.is_empty() {
            Vec::new()
        } else {
            vec![RatatuiSegment {
                text: text.to_string(),
                style: style.clone(),
            }]
        }
    }

    fn visible_lines(&self) -> Vec<Line<'static>> {
        if self.lines.is_empty() {
            return vec![Line::from(String::new())];
        }
        let total = self.lines.len();
        let end = total.saturating_sub(self.scroll_offset);
        let visible_height = self.viewport_height.max(1);
        let start = end.saturating_sub(visible_height);
        self.lines[start..end]
            .iter()
            .map(|line| self.render_line(line))
            .collect()
    }

    fn render_line(&self, line: &MessageLine) -> Line<'static> {
        let mut spans: Vec<Span> = Vec::new();
        let indicator = self.indicator_text(line.kind);
        if !indicator.is_empty() {
            spans.push(Span::styled(
                indicator.to_string(),
                self.indicator_style(line),
            ));
        }
        let fallback = self.fallback_color(line.kind);
        if line.segments.is_empty() {
            spans.push(Span::raw(String::new()));
        } else {
            for segment in &line.segments {
                let style = segment.style.to_style(fallback.or(self.theme.foreground));
                spans.push(Span::styled(segment.text.clone(), style));
            }
        }
        Line::from(spans)
    }

    fn fallback_color(&self, kind: RatatuiMessageKind) -> Option<Color> {
        match kind {
            RatatuiMessageKind::Agent | RatatuiMessageKind::Policy => {
                self.theme.primary.or(self.theme.foreground)
            }
            RatatuiMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            _ => self.theme.foreground,
        }
    }

    fn indicator_text(&self, kind: RatatuiMessageKind) -> &'static str {
        match kind {
            RatatuiMessageKind::User => USER_PREFIX,
            RatatuiMessageKind::Agent | RatatuiMessageKind::Info => "",
            _ => STATUS_DOT_PREFIX,
        }
    }

    fn indicator_style(&self, line: &MessageLine) -> Style {
        let fallback = self
            .fallback_color(line.kind)
            .or(self.theme.foreground)
            .unwrap_or(Color::White);
        let color = line
            .segments
            .iter()
            .find_map(|segment| segment.style.color)
            .unwrap_or(fallback);
        Style::default().fg(color)
    }

    fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height.max(1);
        self.trim_scroll_bounds();
    }

    fn scroll_line_up(&mut self) {
        let max_offset = self.lines.len();
        if self.scroll_offset < max_offset {
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
        let max_offset = self.lines.len();
        self.scroll_offset = min(self.scroll_offset + page, max_offset);
    }

    fn scroll_page_down(&mut self) {
        let page = self.viewport_height.max(1);
        if self.scroll_offset > page {
            self.scroll_offset -= page;
        } else {
            self.scroll_offset = 0;
        }
    }

    fn trim_scroll_bounds(&mut self) {
        let max_offset = self.lines.len();
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }
}
