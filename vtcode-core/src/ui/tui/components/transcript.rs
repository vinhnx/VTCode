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
        let mut remaining = segment.text.as_str();
        let style = segment.style.clone();

        while !remaining.is_empty() {
            if let Some((index, control)) = remaining
                .char_indices()
                .find(|(_, ch)| *ch == '\n' || *ch == '\r')
            {
                let (text, _) = remaining.split_at(index);
                if !text.is_empty() {
                    self.append_to_current(kind, text, &style);
                }

                let control_char = control;
                let next_index = index + control_char.len_utf8();
                remaining = &remaining[next_index..];

                match control_char {
                    '\n' => {
                        self.start_new_line(kind);
                    }
                    '\r' => {
                        if remaining.starts_with('\n') {
                            remaining = &remaining[1..];
                            self.start_new_line(kind);
                        } else {
                            self.reset_current_line(kind);
                        }
                    }
                    _ => {}
                }
            } else {
                if !remaining.is_empty() {
                    self.append_to_current(kind, remaining, &style);
                }
                break;
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

    fn append_to_current(
        &mut self,
        kind: RatatuiMessageKind,
        text: &str,
        style: &RatatuiTextStyle,
    ) {
        if text.is_empty() {
            return;
        }
        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                if let Some(last_segment) = line.segments.last_mut() {
                    if last_segment.style == *style {
                        last_segment.text.push_str(text);
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

        self.push_line(
            kind,
            vec![RatatuiSegment {
                text: text.to_string(),
                style: style.clone(),
            }],
        );
    }

    fn start_new_line(&mut self, kind: RatatuiMessageKind) {
        self.push_line(kind, Vec::new());
    }

    fn reset_current_line(&mut self, kind: RatatuiMessageKind) {
        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                line.segments.clear();
                return;
            }
        }
        self.push_line(kind, Vec::new());
    }
}
